//! The relay ([`dagon_relay`]) on an RP2040: the PC link over USB CDC, an
//! I2C bus (DAGON mailbox, and I2C_RAW to an STM32 ROM loader) and a PIO
//! UART as its links, and the NRST / BOOT0 lines of an STM32 node. This
//! module holds only what is RP2040 hardware: the [`Links`] built from its
//! peripherals, [`start`], the embassy tasks that run the relay with these
//! concrete types, and the embassy-time [`RelayClock`]. A display, if the board
//! has one, reads the [`State`] it passes to [`start`] in [`Setup`].
//!
//! Tasks: the PC task and one task per present link run on DAGON's
//! high-priority executor ([`crate::exec`]), as guard tasks
//! `dagon_relay::task::PC` and `dagon_relay::task::link(i)` for the
//! `i`-th entry of [`RELAY_LINKS`] (declare them `Preempting`); the app's
//! own tasks take guard tasks from [`TASKS`] on, in thread mode
//! (`Background`). [`start`] also spawns the watchdog feeder on the app's
//! executor.

use dagon_port::NodeLink;
/// The role values a node reports ([`Setup::role`]).
pub use dagon_proto::payload::role;
use dagon_proto::payload::StatusResp;
use dagon_relay::nodes::Identity;
use dagon_relay::{
    run_link, serve, task, Config, LinkRoute, Liveness, OwnAgent, Port, Recovery, RelayClock, State,
};
use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C0, PIO0, USB};
use embassy_rp::Peri;
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;

use crate::links::{I2cLink, PioStream, RpI2c};
use crate::node::{self, App, AppAgent, AppEvents, BoardDef, DagonParts, SharedFlash};
use crate::sub::SubLines;
use crate::uart::UartLink;
use crate::watchdog::{feed_forever, now_ms, Guard, Level, Watch};
use crate::RELAY_LINKS;

const LINKS: usize = RELAY_LINKS.len();

/// Guard tasks the relay uses; the app's tasks are numbered from here.
pub const TASKS: usize = task::count(LINKS);

/// Liveness levels for a guard with the relay and `N - TASKS` app tasks in
/// thread mode.
pub const fn levels<const N: usize>() -> [Level; N] {
    crate::watchdog::levels::<N>(TASKS)
}

/// The links the board wires (any may be absent; requests for an absent
/// one answer E_UNSUPPORTED).
pub struct Links {
    pub i2c: Option<I2cLink<RpI2c<'static, I2C0>>>,
    pub uart: Option<PioStream<'static, PIO0, 0, 1>>,
    /// NRST / BOOT0 of an STM32 node on the I2C link (SUBCTL, I2C_RAW).
    pub lines: Option<SubLines<'static>>,
}

/// What the relay node is and how its relay runs.
pub struct Setup {
    /// Image target tag of this node (as in the topology); names it.
    pub tag: &'static str,
    /// Its node index in the topology (`wiring::NODE.index`), which says
    /// which topology links are its own.
    pub index: u8,
    /// The role it reports ([`role`]), from its definition.
    pub role: u8,
    /// USB product name shown on the host.
    pub name: &'static str,
    /// The app's self-check, extensions and who confirms.
    pub app: App,
    /// The relay's state, from [`new_state`]; a display reads the same one.
    pub state: &'static State,
    pub relay: Config,
}

/// The relay's state, kept here for the life of the app. Call once.
pub fn new_state() -> &'static State {
    static STATE: StaticCell<State> = StaticCell::new();
    STATE.init(State::new())
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

/// One request port per entry of [`RELAY_LINKS`].
static PORTS: [Port; LINKS] = [const { Port::new() }; LINKS];

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
    fn status(&self) -> &StatusResp {
        self.0.images().status()
    }
    fn before_reboot(&mut self) {
        self.0.before_reboot()
    }
}

type RpRelay =
    dagon_relay::Relay<'static, RpAgent, SharedFlash, SubLines<'static>, RpWatch, RpClock>;
type RpUart = UartLink<PioStream<'static, PIO0, 0, 1>, fn() -> u64>;

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
        flash,
        events,
    } = node::open::<B, N>(
        parts.flash,
        parts.watchdog,
        guard,
        task::PC,
        setup.tag,
        setup.role,
        setup.app,
    );
    let watch = RpWatch(guard);
    let uart = links
        .uart
        .map(|s| UartLink::new(s, now_ms as fn() -> u64, crate::RELAY_LINK_PIO_UART));
    let wired = [
        links.i2c.as_ref().map(NodeLink::link_id),
        uart.as_ref().map(NodeLink::link_id),
    ];
    // One entry per link of this package; a port only where the board
    // wires the link.
    static ROUTES: StaticCell<[LinkRoute<'static>; LINKS]> = StaticCell::new();
    let routes = ROUTES.init(core::array::from_fn(|i| LinkRoute {
        link: RELAY_LINKS[i],
        port: wired
            .contains(&Some(RELAY_LINKS[i].id))
            .then_some(&PORTS[i]),
    }));
    let relay = RpRelay {
        agent: RpAgent(agent),
        flash,
        topology: crate::layout::TOPOLOGY,
        identity: Identity {
            index: setup.index,
            tag: setup.tag,
        },
        links: routes,
        // The STM32's ROM loader answers on the I2C link.
        recovery: links.lines.map(|lines| Recovery {
            lines,
            link: crate::RELAY_LINK_I2C0,
        }),
        state: setup.state,
        watch,
        clock: RpClock,
        config: setup.relay,
    };
    let high = crate::exec::start();
    high.spawn(pc_task(parts.usb, setup.name, serial, relay).expect("task pool"));
    let (state, config) = (setup.state, setup.relay);
    if let Some(i2c) = links.i2c {
        high.spawn(i2c_task(i2c, watch, state, config).expect("task pool"));
    }
    if let Some(uart) = uart {
        high.spawn(uart_task(uart, watch, state, config).expect("task pool"));
    }
    spawner.spawn(feed(guard).expect("task pool"));
    events
}

/// Runs inside the high-priority executor: builds the USB device there
/// (its types stay in that executor), starts it, then serves the PC.
#[embassy_executor::task]
async fn pc_task(
    usb: Peri<'static, USB>,
    product: &'static str,
    serial: &'static str,
    relay: RpRelay,
) -> ! {
    let (device, link) = crate::usb::new(usb, product, serial);
    // SAFETY: awaited directly inside this embassy task, whose context
    // comes from the interrupt executor running it.
    let spawner = unsafe { Spawner::for_current_executor() }.await;
    spawner.spawn(crate::usb::run(device).expect("task pool"));
    serve(relay, link).await
}

/// The worker of `link`, with its port and guard task by its place in
/// [`RELAY_LINKS`].
async fn work<L: NodeLink>(link: &mut L, watch: RpWatch, state: &State, config: Config) -> ! {
    let i = slot(link.link_id());
    run_link(
        link,
        &PORTS[i],
        RpClock,
        watch,
        task::link(i),
        state,
        &config,
    )
    .await
}

#[embassy_executor::task]
async fn i2c_task(
    mut link: I2cLink<RpI2c<'static, I2C0>>,
    watch: RpWatch,
    state: &'static State,
    config: Config,
) -> ! {
    work(&mut link, watch, state, config).await
}

#[embassy_executor::task]
async fn uart_task(mut link: RpUart, watch: RpWatch, state: &'static State, config: Config) -> ! {
    work(&mut link, watch, state, config).await
}

#[embassy_executor::task]
async fn feed(watch: &'static dyn Watch) -> ! {
    feed_forever(watch).await
}
