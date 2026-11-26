//! DAGON over USB CDC: the device (DAGON's VID:PID, so the host tools find
//! it), a frame link over its serial class, and [`serve_agent`], the whole
//! serving loop of an app whose only DAGON link is USB.
//!
//! Build the device and link inside the executor that serves them, run the
//! device with [`run`], then serve. Call [`new`] once. This module binds
//! the USB interrupt itself.

use dagon_agent::Transition;
use dagon_agent::{Agent, AppChain};
use dagon_port::SlotStore;
use dagon_proto::{max_payload, MAX_FRAME};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Handler, UsbDevice};
use portable_atomic::{AtomicU32, Ordering};
use static_cell::StaticCell;

use crate::watchdog::Watch;
use dagon_format::SigVerifier;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_mcu_common::packets::{self, PacketFrames, PacketPort};
use dagon_node::serve::{serve, Exit, ServeHost};

// This package owns the USB interrupt: apps must not bind USBCTRL_IRQ.
bind_interrupts!(struct UsbIrqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// DAGON's USB vendor and product ID (the host tools look for these).
pub const USB_VID: u16 = 0xc0de;
pub const USB_PID: u16 = 0xcafe;
pub const USB_PACKET: u16 = 64;

const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// Lets the REBOOT reply leave before the reset drops USB.
pub const REBOOT_DELAY: Duration = Duration::from_millis(50);

pub type UsbDriver = Driver<'static, USB>;
type Cdc = CdcAcmClass<'static, UsbDriver>;

static USB_BUFS: StaticCell<([u8; 256], [u8; 256], [u8; 64])> = StaticCell::new();
static USB_STATE: StaticCell<State> = StaticCell::new();
static E5: StaticCell<E5Fix> = StaticCell::new();

/// Bus resets the device has seen, and how many ran the RP2040-E5 fix
/// (the board's `diag` display shows both).
pub static BUS_RESETS: AtomicU32 = AtomicU32::new(0);
pub static E5_FIXES: AtomicU32 = AtomicU32::new(0);

/// The USB device (run it with [`run`]) and the DAGON frame link over its
/// CDC serial class. `product` and `serial` show on the host.
pub fn new(
    usb: Peri<'static, USB>,
    product: &'static str,
    serial: &'static str,
) -> (UsbDevice<'static, UsbDriver>, CdcLink) {
    let mut cfg = embassy_usb::Config::new(USB_VID, USB_PID);
    cfg.manufacturer = Some("DAGON");
    cfg.product = Some(product);
    cfg.serial_number = Some(serial);
    let (config_desc, bos_desc, control_buf) = USB_BUFS.init(([0; 256], [0; 256], [0; 64]));
    disconnect_and_reset();
    let mut builder = embassy_usb::Builder::new(
        Driver::new(usb, UsbIrqs),
        cfg,
        config_desc,
        bos_desc,
        &mut [],
        control_buf,
    );
    builder.handler(E5.init(E5Fix));
    let class = CdcAcmClass::new(&mut builder, USB_STATE.init(State::new()), USB_PACKET);
    let link = CdcLink {
        class,
        rx: PacketFrames::new(max_payload::USB),
    };
    (builder.build(), link)
}

/// A reset that keeps power (the watchdog, the self-update reboot) leaves
/// the pull-up on and USBCTRL configured, so the host never sees the device
/// leave and does not enumerate it again. Drop off the bus long enough for
/// the host to see a disconnect, then start USBCTRL from its reset state.
fn disconnect_and_reset() {
    use embassy_rp::pac;
    pac::USB.sie_ctrl().modify(|w| {
        w.set_pullup_en(false);
        w.set_transceiver_pd(true);
    });
    embassy_time::block_for(Duration::from_millis(100));
    pac::RESETS.reset().modify(|w| w.set_usbctrl(true));
    embassy_time::block_for(Duration::from_micros(10));
    pac::RESETS.reset().modify(|w| w.set_usbctrl(false));
    while !pac::RESETS.reset_done().read().usbctrl() {}
}

/// RP2040-E5 (B0/B1 silicon): after a bus reset the device waits for
/// 800 us of J before it counts as connected, which a busy bus may never
/// give it. As pico-sdk's rp2040_usb_device_enumeration.c: on each bus
/// reset (pull-up on, B0/B1 only) wait for SE0 to end, force J on DP
/// through GPIO15's USB debug mux for 1 ms, then hand DP back to the PHY
/// and restore GPIO15. The waits are bounded so a wedged line cannot
/// stall the high-priority executor until the watchdog fires.
struct E5Fix;

const E5_SE0_LIMIT: Duration = Duration::from_millis(50);
const E5_FORCE_J: Duration = Duration::from_millis(1);
const E5_CONNECT_LIMIT: Duration = Duration::from_millis(10);
const E5_DP_GPIO: usize = 15;
const LINE_SE0: u8 = 0;
/// GPIO function 8: USB debug muxing.
const FUNC_USB_MUX: u8 = 8;

impl Handler for E5Fix {
    fn reset(&mut self) {
        use embassy_rp::pac;
        use embassy_rp::pac::io::vals::{Inover, Oeover};
        BUS_RESETS.fetch_add(1, Ordering::Relaxed);
        let usb = pac::USB;
        let b0_b1 = pac::SYSINFO.chip_id().read().revision() == 1;
        if !b0_b1 || !usb.sie_ctrl().read().pullup_en() {
            return;
        }
        E5_FIXES.fetch_add(1, Ordering::Relaxed);
        wait_until(E5_SE0_LIMIT, || {
            usb.sie_status().read().line_state() != LINE_SE0
        });
        let ctrl = pac::IO_BANK0.gpio(E5_DP_GPIO).ctrl();
        let pad = pac::PADS_BANK0.gpio(E5_DP_GPIO);
        let (ctrl_prev, pad_prev) = (ctrl.read(), pad.read());
        pad.modify(|w| {
            w.set_pue(true);
            w.set_pde(true);
        });
        ctrl.modify(|w| {
            w.set_oeover(Oeover::DISABLE);
            w.set_funcsel(FUNC_USB_MUX);
            w.set_inover(Inover::HIGH);
        });
        usb.usbphy_direct().modify(|w| w.set_dp_pullup_en(true));
        usb.usbphy_direct_override()
            .modify(|w| w.set_dp_pullup_en_override_en(true));
        usb.usb_muxing().write(|w| {
            w.set_to_digital_pad(true);
            w.set_softcon(true);
        });
        embassy_time::block_for(E5_FORCE_J);
        wait_until(E5_CONNECT_LIMIT, || usb.sie_status().read().connected());
        usb.usb_muxing().write(|w| {
            w.set_to_phy(true);
            w.set_softcon(true);
        });
        usb.usbphy_direct_override()
            .modify(|w| w.set_dp_pullup_en_override_en(false));
        ctrl.write_value(ctrl_prev);
        pad.write_value(pad_prev);
    }
}

/// Spins until `done` holds or `limit` has passed.
fn wait_until(limit: Duration, done: impl Fn() -> bool) {
    let end = embassy_time::Instant::now() + limit;
    while !done() && embassy_time::Instant::now() < end {}
}

/// Runs the USB device, and re-attaches it while the host does not reset
/// the bus ([`dagon_mcu_common::reattach`]).
#[embassy_executor::task]
pub async fn run(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    // SAFETY: awaited directly inside this embassy task, whose context
    // comes from the executor running it.
    let spawner = unsafe { embassy_executor::Spawner::for_current_executor() }.await;
    spawner.spawn(reattach().expect("task pool"));
    usb.run().await
}

const _: () = assert!(dagon_mcu_common::reattach::SPAN_MS <= crate::TRIAL_WDT_MS as u64);

/// Runs the USB device and its re-attach together, without an executor
/// (a boot core polls it among its other futures).
pub async fn run_here(mut usb: UsbDevice<'static, UsbDriver>) {
    embassy_futures::join::join(usb.run(), reattach_loop()).await;
}

#[embassy_executor::task]
async fn reattach() {
    reattach_loop().await
}

async fn reattach_loop() {
    let resets = || BUS_RESETS.load(Ordering::Relaxed);
    let now = || embassy_time::Instant::now().as_millis();
    let mut r = dagon_mcu_common::reattach::Reattach::new(now(), resets());
    while let Some(at) = r.step(now(), resets(), &mut SiePullup) {
        Timer::at(embassy_time::Instant::from_millis(at)).await;
    }
}

/// The pull-up through SIE_CTRL.PULLUP_EN: embassy-rp's `Bus::disable`
/// does nothing, so embassy-usb cannot take the device off the bus.
struct SiePullup;

impl dagon_mcu_common::reattach::Pullup for SiePullup {
    fn attach(&mut self) {
        embassy_rp::pac::USB
            .sie_ctrl()
            .modify(|w| w.set_pullup_en(true));
    }
    fn detach(&mut self) {
        embassy_rp::pac::USB
            .sie_ctrl()
            .modify(|w| w.set_pullup_en(false));
    }
}

/// The CDC data interface as the packet pipe the frames go through.
struct CdcPort<'a>(&'a mut Cdc);

impl PacketPort for CdcPort<'_> {
    type Error = EndpointError;
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, EndpointError> {
        self.0.wait_connection().await;
        self.0.read_packet(buf).await
    }
    async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        self.0.write_packet(data).await
    }
}

/// DAGON frames over the CDC serial class (cut and written by
/// `dagon_mcu_common::packets`, which holds the host tests).
pub struct CdcLink {
    class: Cdc,
    rx: PacketFrames<{ MAX_FRAME + USB_PACKET as usize }, { USB_PACKET as usize }>,
}

impl CdcLink {
    /// The USB device (run it with [`run`]) and the link; as [`new`].
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        usb: Peri<'static, USB>,
        product: &'static str,
        serial: &'static str,
    ) -> (UsbDevice<'static, UsbDriver>, CdcLink) {
        new(usb, product, serial)
    }
}

impl Link for CdcLink {
    fn max_payload(&self) -> usize {
        max_payload::USB
    }

    fn reconnect(&self) -> Reconnect {
        Reconnect::Reenumerates
    }

    fn kind(&self) -> Kind {
        Kind::UsbCdc
    }

    /// Waits for the next complete frame from the host (never times out).
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        Ok(self.rx.read_frame(&mut CdcPort(&mut self.class)).await)
    }

    /// Writes one frame (a zero-length packet ends a frame that fills its
    /// last packet); gives up if the host stops reading.
    async fn write_frame(&mut self, data: &[u8]) -> Result<(), LinkError> {
        let mut port = CdcPort(&mut self.class);
        let write = packets::write_frame(&mut port, data, USB_PACKET as usize);
        match with_timeout(USB_WRITE_TIMEOUT, write).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(LinkError::Closed),
        }
    }

    /// Lets the last reply leave before a reset drops USB.
    async fn flush(&mut self) -> Result<(), LinkError> {
        Timer::after(REBOOT_DELAY).await;
        Ok(())
    }
}

/// The serving loop's view of the guard: guard task `task` is idle while
/// waiting for the host and busy while answering.
pub struct GuardHost<'a> {
    pub watch: &'a dyn Watch,
    pub task: usize,
}

impl<S: SlotStore<Transition = Transition>, X: AppChain, V: SigVerifier> ServeHost<S, X, V>
    for GuardHost<'_>
{
    fn now_ms(&self) -> u64 {
        embassy_time::Instant::now().as_millis()
    }
    fn idle(&mut self) {
        self.watch.idle(self.task);
    }
    fn busy(&mut self) {
        self.watch.busy(self.task);
    }
    fn feed(&mut self) {
        self.watch.feed();
    }
    async fn wait_ms(&mut self, ms: u32) {
        Timer::after_millis(ms.into()).await;
    }
}

/// Serves `agent` over `link` (any [`Link`]) forever; REBOOT resets the
/// chip after the reply has left and the delay `allow_reboot_now` asked
/// for.
pub async fn serve_agent<
    L: Link,
    S: SlotStore<Transition = Transition>,
    X: AppChain,
    V: SigVerifier,
>(
    mut link: L,
    mut agent: Agent<S, X, V>,
    guard: &dyn Watch,
    task: usize,
) -> ! {
    let mut host = GuardHost { watch: guard, task };
    loop {
        if serve(&mut link, &mut agent, &mut host).await == Exit::Reboot {
            guard.reboot();
        }
    }
}
