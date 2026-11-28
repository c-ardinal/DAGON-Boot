//! The relay ([`dagon_relay`]) on an RP2040: the PC link ([`Uplink`]: USB
//! CDC on a coordinator, a UART on a node that relays as a node), its
//! I2C buses (DAGON mailbox, and I2C_RAW to an STM32 ROM loader) and PIO
//! UARTs as its links, as many of them as the board wires, and the NRST /
//! BOOT0 lines of an STM32 node. This
//! module holds only what is RP2040 hardware: the [`Links`] built from its
//! peripherals, [`start`], the embassy tasks that run the relay with these
//! concrete types, and the embassy-time [`RelayClock`].
//!
//! Tasks: the PC task and one task per present link run on DAGON's
//! high-priority executor ([`crate::exec`]), as guard tasks
//! `dagon_relay::task::PC` and `dagon_relay::task::link(i)` for the
//! `i`-th entry of [`RELAY_LINKS`] (declare them `Preempting`); the app's
//! own tasks take guard tasks from [`TASKS`] on, in thread mode
//! (`Background`). [`start`] also spawns the watchdog feeder on the app's
//! executor.

/// The role values a node reports ([`Setup::role`]).
pub use dagon_proto::payload::role;
use dagon_relay::{
    run_link, serve, task, Config, LinkRoute, Liveness, OwnAgent, Port, Recovery, RelayClock,
};
use embassy_executor::Spawner;
use embassy_rp::interrupt::typelevel::Binding;
use embassy_rp::peripherals::{I2C0, I2C1, PIO0, PIO1, USB};
use embassy_rp::Peri;
use embassy_time::{Instant, Timer};
use heapless::Vec;
use static_cell::StaticCell;

use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_proto::max_payload;

use crate::boot_links::{HwUart, UartTarget};
use crate::links::{I2cLink, PioStream, RpI2c};
use crate::node::{self, App, AppAgent, AppEvents, BoardDef, DagonParts};
use crate::sub::SubLines;
use crate::uart::UartLink;
use crate::watchdog::{feed_forever, now_ms, Guard, Level, Watch};
use crate::{
    RECOVERY_TARGETS, RELAY_LINKS, RELAY_LINK_BUFFERS, RELAY_LINK_I2C0, RELAY_LINK_I2C1,
    RELAY_LINK_PIO_UART, RELAY_LINK_PIO_UART1,
};

const LINKS: usize = RELAY_LINKS.len();

/// Guard tasks the relay uses; the app's tasks are numbered from here.
pub const TASKS: usize = task::count(LINKS);

/// Liveness levels for a guard with the relay and `N - TASKS` app tasks in
/// thread mode.
pub const fn levels<const N: usize>() -> [Level; N] {
    crate::watchdog::levels::<N>(TASKS)
}

/// The link to the node nearer the PC, which the relay serves as its PC
/// link (`dagon_relay::serve` takes any [`Link`]).
pub enum Uplink {
    /// USB CDC: the relay builds the device from [`DagonParts::usb`] (a
    /// coordinator, and a node reached over USB).
    Usb,
    /// A UART from the node nearer the PC (a node that relays as a node,
    /// `[relay] relays_as_node`): requests arrive on it and their replies
    /// leave on it; [`DagonParts::usb`] stays unused. Made by
    /// [`Uplink::uart`], which keeps the link in a static (its frame buffer
    /// is 2 KiB), so only a node wired this way holds one and the PC task
    /// holds a reference.
    Uart(&'static mut UartUp),
}

/// The UART of an [`Uplink::Uart`], by the port its link end names: the
/// hardware UART (`uart0`, `uart1`), or a PIO's state machines 0 (TX) and
/// 1 (RX) (`pio-uart` on PIO0, `pio-uart1` on PIO1). A PIO taken for the
/// uplink is not the downlink of that port too: the board wires one of
/// them.
pub enum UartUp {
    Hw(UartTarget<HwUart>),
    Pio0(UartTarget<PioStream<'static, PIO0, 0, 1>>),
    Pio1(UartTarget<PioStream<'static, PIO1, 0, 1>>),
}

impl Link for &'static mut UartUp {
    fn max_payload(&self) -> usize {
        max_payload::UART
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }
    fn kind(&self) -> Kind {
        Kind::Uart
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        match &mut **self {
            UartUp::Hw(l) => l.read_frame().await,
            UartUp::Pio0(l) => l.read_frame().await,
            UartUp::Pio1(l) => l.read_frame().await,
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        match &mut **self {
            UartUp::Hw(l) => l.write_frame(frame).await,
            UartUp::Pio0(l) => l.write_frame(frame).await,
            UartUp::Pio1(l) => l.write_frame(frame).await,
        }
    }
}

impl UartUp {
    /// The hardware UART `uart` (its pins and speed set by the board).
    pub fn hw(uart: embassy_rp::uart::BufferedUart) -> Self {
        UartUp::Hw(UartTarget::new(HwUart(uart)))
    }
    /// The hardware UART `uart` on pins `tx`, `rx` at `baud` bit/s, its
    /// buffers in this package's static (a frame's worth to receive). Call
    /// once.
    pub fn hw_on<T: embassy_rp::uart::Instance>(
        uart: Peri<'static, T>,
        tx: Peri<'static, impl embassy_rp::uart::TxPin<T>>,
        rx: Peri<'static, impl embassy_rp::uart::RxPin<T>>,
        irq: impl Binding<T::Interrupt, embassy_rp::uart::BufferedInterruptHandler<T>>,
        baud: u32,
    ) -> Self {
        static BUFS: StaticCell<([u8; 64], [u8; 2048])> = StaticCell::new();
        let (tb, rb) = BUFS.init(([0; 64], [0; 2048]));
        let mut c = embassy_rp::uart::Config::default();
        c.baudrate = baud;
        Self::hw(embassy_rp::uart::BufferedUart::new(
            uart, tx, rx, irq, tb, rb, c,
        ))
    }
    /// PIO0's PIO UART (port `pio-uart`).
    pub fn pio0(stream: PioStream<'static, PIO0, 0, 1>) -> Self {
        UartUp::Pio0(UartTarget::new(stream))
    }
    /// PIO1's PIO UART (port `pio-uart1`).
    pub fn pio1(stream: PioStream<'static, PIO1, 0, 1>) -> Self {
        UartUp::Pio1(UartTarget::new(stream))
    }
}

impl Uplink {
    /// The uplink `uart`, kept in this package's static. Call once: a
    /// second call panics (there is one uplink).
    pub fn uart(uart: UartUp) -> Self {
        static UP: StaticCell<UartUp> = StaticCell::new();
        Uplink::Uart(UP.init(uart))
    }

    /// The largest payload a request on it carries: what the node's agent
    /// tells the host in HELLO, so the host's chunks fit the link.
    const fn max_payload(&self) -> usize {
        match self {
            Uplink::Usb => max_payload::USB,
            Uplink::Uart(_) => max_payload::UART,
        }
    }
}

/// The links the board wires: the [`Uplink`] (USB from [`Links::none`]),
/// and one field per relay link of the MCU
/// definition (any may be absent; requests for an absent one answer
/// E_UNSUPPORTED). A board names the links it wires and takes the rest
/// from [`Links::none`].
pub struct Links {
    /// The link to the node nearer the PC.
    pub up: Uplink,
    /// Port `i2c0`.
    pub i2c: Option<I2cLink<RpI2c<'static, I2C0>>>,
    /// Port `i2c1`.
    pub i2c1: Option<I2cLink<RpI2c<'static, I2C1>>>,
    /// Port `pio-uart`: PIO0's state machines 0 (TX) and 1 (RX).
    pub uart: Option<PioStream<'static, PIO0, 0, 1>>,
    /// Port `pio-uart1`: PIO1's state machines 0 (TX) and 1 (RX).
    pub uart1: Option<PioStream<'static, PIO1, 0, 1>>,
    /// NRST / BOOT0 of the STM32 nodes the board wires (SUBCTL, I2C_RAW):
    /// one per I2C link at most. The relay recovers the nodes its topology
    /// names (`RECOVERY_TARGETS`), each through the lines wired for its
    /// link ([`dagon_port::RecoveryTargetDef::wired_by`]).
    pub lines: [Option<RecoveryLines>; RECOVERY_LINES],
}

/// The most nodes a board wires recovery lines for: one per I2C link.
pub const RECOVERY_LINES: usize = 2;

/// NRST / BOOT0 of one STM32 node, and the relay link that node answers
/// on, its ROM loader included: the id of `i2c0` or of `i2c1`.
pub struct RecoveryLines {
    pub lines: SubLines<'static>,
    pub link: u8,
}

impl RecoveryLines {
    /// Lines of the node on `i2c0`.
    pub fn on_i2c0(lines: SubLines<'static>) -> Self {
        RecoveryLines {
            lines,
            link: RELAY_LINK_I2C0,
        }
    }

    /// Lines of the node on `i2c1`.
    pub fn on_i2c1(lines: SubLines<'static>) -> Self {
        RecoveryLines {
            lines,
            link: RELAY_LINK_I2C1,
        }
    }
}

impl Links {
    /// No link wired.
    pub const fn none() -> Self {
        Links {
            up: Uplink::Usb,
            i2c: None,
            i2c1: None,
            uart: None,
            uart1: None,
            lines: [None, None],
        }
    }
}

/// What the relay node is and how its relay runs.
pub struct Setup {
    /// Image target tag of this node (as in the topology); names it.
    pub tag: &'static str,
    /// The role it reports ([`role`]), from its definition.
    pub role: u8,
    /// USB product name shown on the host (an [`Uplink::Usb`]'s).
    pub name: &'static str,
    /// The app's self-check, extensions and who confirms.
    pub app: App,
    pub relay: Config,
}

/// Index of the link `id` in [`RELAY_LINKS`].
const fn slot(id: u8) -> usize {
    let mut i = 0;
    while i < LINKS {
        if RELAY_LINKS[i].id == id {
            return i;
        }
        i += 1;
    }
    panic!("not a relay link of this package")
}

type RpUart<P> = UartLink<PioStream<'static, P, 0, 1>, fn() -> u64>;

/// One relay link of this package: its place in [`RELAY_LINKS`] by the id
/// the definition gives its port, its request port of the generated buffer
/// size ([`RELAY_LINK_BUFFERS`]; a size that is not the link's stops the
/// build), and the worker task that runs it under the guard task of that
/// place.
macro_rules! relay_link {
    ($at:ident, $port:ident, $task:ident, $id:expr, $link:ty) => {
        const $at: usize = slot($id);
        static $port: Port<[u8; RELAY_LINK_BUFFERS[$at]]> = Port::new(&RELAY_LINKS[$at]);

        #[embassy_executor::task]
        async fn $task(mut link: $link, watch: RpWatch) -> ! {
            run_link(&mut link, &$port, watch, task::link($at)).await
        }
    };
}

relay_link!(
    I2C,
    I2C0_PORT,
    i2c0_task,
    RELAY_LINK_I2C0,
    I2cLink<RpI2c<'static, I2C0>>
);
relay_link!(
    I2C_1,
    I2C1_PORT,
    i2c1_task,
    RELAY_LINK_I2C1,
    I2cLink<RpI2c<'static, I2C1>>
);
relay_link!(
    UART,
    PIO_UART_PORT,
    pio_uart_task,
    RELAY_LINK_PIO_UART,
    RpUart<PIO0>
);
relay_link!(
    UART_1,
    PIO_UART1_PORT,
    pio_uart1_task,
    RELAY_LINK_PIO_UART1,
    RpUart<PIO1>
);

/// Embassy time as the relay's clock.
#[derive(Clone, Copy)]
pub struct RpClock;

impl RelayClock for RpClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }

    async fn wait_until(&self, at_ms: u64) {
        Timer::at(Instant::from_millis(at_ms)).await
    }
}

/// The relay's tasks under the app's watchdog guard.
#[derive(Clone, Copy)]
struct RpWatch(&'static dyn Watch);

impl Liveness for RpWatch {
    fn busy(&self, task: usize) {
        self.0.busy(task)
    }
    fn busy_for(&self, task: usize, bound_ms: u64) {
        self.0.busy_for(task, bound_ms)
    }
    fn idle(&self, task: usize) {
        self.0.idle(task)
    }
    fn progress(&self, task: usize) {
        // Diagnostic panic blink code: answering the PC.
        if task == task::PC {
            crate::arm::step(14);
        }
        self.0.progress(task)
    }
    fn reboot(&self) -> ! {
        self.0.reboot()
    }
}

/// The node's agent as the relay drives it.
struct RpAgent(AppAgent);

impl OwnAgent for RpAgent {
    fn link_up(&mut self, link: u8, addr: u8) {
        self.0.link_up(link, addr)
    }
    fn link_down(&mut self, link: u8, addr: u8) {
        self.0.link_down(link, addr)
    }
    fn request(&mut self, frame: &[u8], via: u8) {
        self.0.request(frame, via)
    }
    fn replied(&mut self, frame: &[u8], ms: u32) {
        if let Some(status) = dagon_agent::reply_status(frame) {
            self.0.replied(status, ms)
        }
    }
    fn handle(&mut self, request: &[u8]) -> Option<(&[u8], Option<u32>)> {
        let r = self.0.handle(request)?;
        Some((r.frame, r.reboot.then_some(r.reboot_delay_ms)))
    }
    fn before_reboot(&mut self) {
        self.0.before_reboot()
    }
    fn also_serves(&mut self, front: dagon_proto::payload::CmdSet) {
        self.0.also_serves(front)
    }
    fn ceiling(&self) -> dagon_proto::handler::Ceiling {
        self.0.ceiling()
    }
    fn pc_up(&mut self, link: u8, addr: u8) {
        self.0.pc_up(link, addr)
    }
    fn pc_down(&mut self, link: u8, addr: u8) {
        self.0.pc_down(link, addr)
    }
    fn relayed(&mut self, link: u8, addr: u8, cmd: u8) {
        self.0.relayed(link, addr, cmd)
    }
    fn child_control(&mut self, link: u8, nrst: u8, boot0: u8) {
        self.0.child_control(link, nrst, boot0)
    }
    fn child_raw(&mut self, addr: u8, len: u16, read_len: u16) {
        self.0.child_raw(addr, len, read_len)
    }
    fn peer_answered(&mut self, link: u8, addr: u8) -> bool {
        self.0.peer_answered(link, addr)
    }
    fn peers(&mut self) -> Option<dagon_proto::payload::PeerLog> {
        self.0.peers()
    }
}

type RpRelay = dagon_relay::Relay<'static, RpAgent, SubLines<'static>, RpWatch, RpClock>;

/// Starts the relay: opens the store and the agent (taking over the
/// watchdog, running the board's bring-up, see `node::open`), spawns the
/// PC and link tasks on DAGON's high-priority executor and the watchdog
/// feeder on `spawner` (the app's executor). Returns the node's events.
pub fn start<B: BoardDef, const N: usize>(
    spawner: Spawner,
    parts: DagonParts,
    links: Links,
    guard: &'static Guard<N>,
    setup: Setup,
) -> AppEvents {
    const { assert!(N >= TASKS, "the relay needs its guard tasks 0..TASKS") };
    let node::Opened {
        agent,
        serial,
        events,
    } = node::open::<B, N>(
        parts.flash,
        parts.watchdog,
        guard,
        task::PC,
        node::config(setup.tag, setup.role, links.up.max_payload()),
        setup.app,
    );
    let watch = RpWatch(guard);
    let uart = links
        .uart
        .map(|s| UartLink::new(s, now_ms as fn() -> u64, RELAY_LINK_PIO_UART));
    let uart1 = links
        .uart1
        .map(|s| UartLink::new(s, now_ms as fn() -> u64, RELAY_LINK_PIO_UART1));
    // One entry per link of this package; a port only where the board
    // wires the link.
    let mut ports: [Option<&'static Port>; LINKS] = [None; LINKS];
    ports[I2C] = links.i2c.is_some().then_some(&I2C0_PORT as &Port);
    ports[I2C_1] = links.i2c1.is_some().then_some(&I2C1_PORT as &Port);
    ports[UART] = uart.is_some().then_some(&PIO_UART_PORT as &Port);
    ports[UART_1] = uart1.is_some().then_some(&PIO_UART1_PORT as &Port);
    static ROUTES: StaticCell<[LinkRoute<'static>; LINKS]> = StaticCell::new();
    let routes = ROUTES.init(core::array::from_fn(|i| LinkRoute {
        link: RELAY_LINKS[i],
        port: ports[i],
    }));
    // One recovery target per node the topology gives this relay
    // (generated: the link and address its ROM loader answers on), each
    // with the lines the board wires for it; a target the board wires no
    // lines for is not served.
    static RECOVERY: StaticCell<Vec<Recovery<SubLines<'static>>, RECOVERY_LINES>> =
        StaticCell::new();
    let mut wired = links.lines;
    let recovery = RECOVERY.init(Vec::new());
    for t in RECOVERY_TARGETS {
        let lines = wired.iter_mut().find_map(|w| {
            let (nrst, boot) = w.as_ref()?.lines.pins();
            t.wired_by(w.as_ref()?.link, nrst, boot)
                .then(|| w.take())
                .flatten()
        });
        if let Some(l) = lines {
            // One entry per set of lines: the list holds them all.
            let _ = recovery.push(Recovery {
                lines: l.lines,
                link: t.link,
                rom_addr: t.rom_addr,
            });
        }
    }
    let relay = RpRelay {
        agent: RpAgent(agent),
        links: routes,
        recovery: recovery.as_mut_slice(),
        watch,
        clock: RpClock,
        config: setup.relay,
    };
    let high = crate::exec::start();
    let up = Up {
        usb: parts.usb,
        link: links.up,
        product: setup.name,
        serial,
    };
    high.spawn(pc_task(up, relay).expect("task pool"));
    if let Some(i2c) = links.i2c {
        high.spawn(i2c0_task(i2c, watch).expect("task pool"));
    }
    if let Some(i2c) = links.i2c1 {
        high.spawn(i2c1_task(i2c, watch).expect("task pool"));
    }
    if let Some(uart) = uart {
        high.spawn(pio_uart_task(uart, watch).expect("task pool"));
    }
    if let Some(uart) = uart1 {
        high.spawn(pio_uart1_task(uart, watch).expect("task pool"));
    }
    spawner.spawn(feed(guard).expect("task pool"));
    #[cfg(feature = "masked-time")]
    crate::node::spawn_masked_readout::<B>(spawner);
    events
}

/// What the PC task serves the PC on: the [`Uplink`], and the USB
/// peripheral with the device's names for an [`Uplink::Usb`].
struct Up {
    usb: Peri<'static, USB>,
    link: Uplink,
    product: &'static str,
    serial: &'static str,
}

/// Runs inside the high-priority executor and serves the PC on the
/// uplink. USB: builds the device there (its types stay in that executor)
/// and starts it first. One task for either uplink, so the relay's state
/// is held once whichever the board wires.
#[embassy_executor::task]
async fn pc_task(up: Up, relay: RpRelay) -> ! {
    match up.link {
        Uplink::Usb => {
            let (device, link) = crate::usb::new(up.usb, up.product, up.serial);
            // SAFETY: awaited directly inside this embassy task, whose
            // context comes from the interrupt executor running it.
            let spawner = unsafe { Spawner::for_current_executor() }.await;
            spawner.spawn(crate::usb::run(device).expect("task pool"));
            serve(relay, link).await
        }
        Uplink::Uart(link) => serve(relay, link).await,
    }
}

#[embassy_executor::task]
async fn feed(watch: &'static dyn Watch) -> ! {
    feed_forever(watch).await
}
