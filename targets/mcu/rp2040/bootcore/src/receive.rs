//! Update and recovery mode: the update session (dagon-agent with the boot
//! core writer, an app chain without app extensions) served by
//! `dagon_mcu_rp2040::boot_receive::serve` on every link the boot core
//! opens: its I2C and UART links, and USB on a clock setup with the USB
//! clock (the device the app presents: same IDs and serial, so the host
//! finds the node again). One task on a thread-mode executor serves; the
//! USB device runs in its own task. Both sleep between events.
//!
//! The watchdog is fed by the serving loop each time it comes round (at
//! least every [`SERVE_TICK`]), at every request, every erased sector and
//! every step of a signature check; a loop that stops coming round resets
//! the node.

use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog as Periods};
use dagon_format::SigVerifier;
use dagon_mcu_common::guard::Watch;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_mcu_common::serve::ServeHost;
use dagon_mcu_common::shared::{driver, FlashDriver, SharedFlash};
use dagon_mcu_rp2040::boot_receive::{self, Entry, Links};
use dagon_mcu_rp2040::flash::{RpFlash, REGIONS};
use dagon_mcu_rp2040::layout::watchdog_ms;
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::store::AppStore;
use dagon_mcu_rp2040::usb::{self, CdcLink};
use dagon_mcu_rp2040::{ERASE_REGIONS, TARGET_TAG};
use dagon_proto::payload::role;
use embassy_executor::{Executor, Spawner};
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::Peri;
use embassy_time::{Duration, Instant, Timer};
use static_cell::StaticCell;

use crate::{feed, reset, I2cLink, UartLink, Verifier, CLOCK, VERIFIER};

/// How long a read waits before the serving loop comes round: well inside
/// the watchdog's period.
const SERVE_TICK: Duration = Duration::from_secs(1);

/// The boot core's links: USB, I2C, UART.
pub type BootLinks = Links<CdcLink, I2cLink, UartLink>;

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Session = Agent<AppStore, Chain, Verifier, BootCore>;

/// Milliseconds since HAL init.
pub fn now_ms() -> u64 {
    Instant::now().as_millis()
}

/// Serves the update session in `entry`'s mode on `links` (USB added
/// here on a setup with the USB clock) until a REBOOT's reply has left or,
/// entered by STAY or the hand-over, no request came for 60 s; then
/// resets.
pub fn serve(usb: Peri<'static, USB>, mut flash: RpFlash, links: BootLinks, entry: Entry) -> ! {
    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    let serial = node::serial(&mut flash);
    EXECUTOR.init(Executor::new()).run(|spawner| {
        spawner.spawn(session(usb, flash, serial, links, entry).expect("task pool"));
    })
}

#[embassy_executor::task]
async fn session(
    usb: Peri<'static, USB>,
    flash: RpFlash,
    serial: &'static str,
    mut links: BootLinks,
    entry: Entry,
) {
    static CHIP: StaticCell<FlashDriver<RpFlash>> = StaticCell::new();
    static EVENTS: StaticCell<Published<(), 4>> = StaticCell::new();
    if CLOCK.has_usb() {
        let (device, link) = usb::new(usb, TARGET_TAG, serial);
        // SAFETY: awaited directly inside this embassy task, whose context
        // comes from the executor running it.
        let spawner = unsafe { Spawner::for_current_executor() }.await;
        spawner.spawn(usb::run(device).expect("task pool"));
        links.usb = Some(link);
    }
    let store = AppStore::new(
        SharedFlash::new(CHIP.init(driver(flash)), &REGIONS),
        &CORE,
        0,
        &ERASE_REGIONS,
    );
    let now: fn() -> u64 = || Instant::now().as_micros();
    let periods = Periods::new(watchdog_ms(true), watchdog_ms(false));
    let chain = compose(
        Layer::App,
        EVENTS.init(Published::new()),
        Nop,
        Nop,
        Bounds::new(now, periods),
    );
    let cfg = dagon_agent::Config {
        mode: entry.mode(),
        max_payload: links.max_payload() as u16,
        ..node::config(TARGET_TAG, role::NODE)
    };
    let mut agent: Session = Agent::for_writer(cfg, VERIFIER, store, chain);
    let links = Ticked::new(links, SERVE_TICK);
    boot_receive::serve(links, &mut agent, &mut CoreHost, entry, now_ms).await;
    reset()
}

/// `L` with reads that give up after `tick`, so the loop comes round to
/// feed the watchdog and to count the time.
pub struct Ticked<L> {
    link: L,
    tick: Duration,
}

impl<L> Ticked<L> {
    pub fn new(link: L, tick: Duration) -> Self {
        Ticked { link, tick }
    }

    pub fn into_inner(self) -> L {
        self.link
    }
}

impl<L: Link> Link for Ticked<L> {
    fn max_payload(&self) -> usize {
        self.link.max_payload()
    }
    fn reconnect(&self) -> Reconnect {
        self.link.reconnect()
    }
    fn kind(&self) -> Kind {
        self.link.kind()
    }
    fn addr(&self) -> u8 {
        self.link.addr()
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        match select(self.link.read_frame(), Timer::after(self.tick)).await {
            Either::First(r) => r,
            Either::Second(()) => Err(LinkError::Timeout),
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.link.write_frame(frame).await
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.link.no_reply().await
    }
    async fn flush(&mut self) -> Result<(), LinkError> {
        self.link.flush().await
    }
}

/// The boot core's watchdog as the slot store and the serving loop see
/// it: one task, fed directly (the boot core runs nothing else).
struct Core;

static CORE: Core = Core;

impl Watch for Core {
    fn busy(&self, _: usize) {}
    fn busy_for(&self, _: usize, _: u64) {}
    fn idle(&self, _: usize) {}
    fn feed(&self) {
        feed();
    }
    fn set_trial(&self, _: bool) {}
    fn reboot(&self) -> ! {
        reset()
    }
}

struct CoreHost;

impl<S: SlotStore, X: AppChain, V: SigVerifier> ServeHost<S, X, V, BootCore> for CoreHost {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
    /// The loop came round.
    fn idle(&mut self) {
        feed();
    }
    fn busy(&mut self) {}
    fn feed(&mut self) {
        feed();
    }
    async fn wait_ms(&mut self, ms: u32) {
        Timer::after_millis(ms.into()).await;
    }
}
