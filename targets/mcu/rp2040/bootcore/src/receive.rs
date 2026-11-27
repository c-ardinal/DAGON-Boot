//! Receiving: the listen window and update or recovery mode of
//! `dagon_agent::update_mode`, over the boot core's links read as one by
//! `dagon_mcu_rp2040::boot_receive::pump`: its I2C and UART links, and USB
//! on a clock setup with the USB clock (the device the app presents: same
//! IDs and serial, so the host finds the node again). Everything runs on
//! the boot core's one thread: the port polls the links' futures and
//! sleeps on WFE between them, woken by any interrupt that becomes pending
//! (SEVONPEND: the links', the USB controller's, the timer's).
//!
//! The watchdog is fed by the session loop at every wake (at least every
//! [`FEED_EVERY_MS`]), at every request, every erased sector and every
//! step of a signature check; a loop that stops coming round resets the
//! node.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use dagon_agent::update_mode::Stay;
use dagon_agent::writer::BootCore;
use dagon_agent::Agent;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog as Periods};
use dagon_mcu_common::shared::{driver, FlashDriver, SharedFlash};
use dagon_mcu_rp2040::boot_receive::{self, Entry, Links, Mailbox, PumpPort, Sleep, BOOT_LINKS};
use dagon_mcu_rp2040::flash::{RpFlash, REGIONS};
use dagon_mcu_rp2040::layout::watchdog_ms;
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::store::AppStore;
use dagon_mcu_rp2040::usb::{self, CdcLink};
use dagon_mcu_rp2040::{ERASE_REGIONS, TARGET_TAG};
use dagon_node::guard::Watch;
use dagon_proto::payload::role;
use embassy_rp::peripherals::USB;
use embassy_rp::Peri;
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;

use crate::{feed, reset, I2cLink, UartLink, Verifier, CLOCK, VERIFIER};

/// The longest the loop sleeps between feeds: a quarter of the watchdog's
/// period.
pub const FEED_EVERY_MS: u32 = watchdog_ms(true) / 4;

/// The boot core's links: USB, I2C, UART.
pub type BootLinks = Links<CdcLink, I2cLink, UartLink>;

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
/// The session: one requester per boot core link.
type Session = Agent<AppStore, Chain, Verifier, BootCore, BOOT_LINKS>;

/// Microseconds since HAL init.
pub fn now_us() -> u64 {
    Instant::now().as_micros()
}

/// The boot core's sleep: a timer for the wait's limit, WFE between polls.
pub struct Wfe {
    timer: Option<Timer>,
}

impl Wfe {
    pub fn new() -> Self {
        // SAFETY: the boot core's thread-mode code is the only user of SCB.
        unsafe { cortex_m::Peripherals::steal() }
            .SCB
            .set_sevonpend();
        Wfe { timer: None }
    }
}

impl Sleep for Wfe {
    fn arm(&mut self, max_us: u32) {
        self.timer = Some(Timer::after_micros(max_us.into()));
    }
    fn done(&mut self) -> bool {
        let Some(t) = self.timer.as_mut() else {
            return true;
        };
        let mut cx = Context::from_waker(Waker::noop());
        matches!(core::pin::Pin::new(t).poll(&mut cx), Poll::Ready(()))
    }
    fn idle(&mut self) {
        cortex_m::asm::wfe();
    }
}

/// The step before the boot decision (`boot_receive::receive_step`) on
/// the I2C and UART links: the app handed over, else a STAY in the listen
/// window, with the STAY the session answers first.
pub fn step(
    flash: &mut RpFlash,
    links: &mut BootLinks,
    window_ms: u16,
) -> Option<(Entry, Option<Stay>)> {
    let receives = CLOCK.has_usb() || links.any();
    // No link opened (no address for the I2C target): no window.
    let window_ms = if links.any() { window_ms } else { 0 };
    let mb = Mailbox::new();
    let pump = pin!(boot_receive::pump(links, &mb));
    let mut port = PumpPort::new(pump, &mb, Wfe::new());
    let mut clock = now_us;
    boot_receive::receive_step(
        flash,
        &mut port,
        receives,
        &mut clock,
        window_ms,
        FEED_EVERY_MS,
        &mut feed,
    )
}

/// Serves the update session in `entry`'s mode on `links` (USB added
/// here on a setup with the USB clock), answering `stay` first, until a
/// REBOOT's reply has left or, entered by STAY or the hand-over, no
/// request came for 60 s; then resets.
pub fn serve(
    usb: Peri<'static, USB>,
    mut flash: RpFlash,
    mut links: BootLinks,
    entry: Entry,
    stay: Option<Stay>,
) -> ! {
    static CHIP: StaticCell<FlashDriver<RpFlash>> = StaticCell::new();
    static EVENTS: StaticCell<Published<(), 4>> = StaticCell::new();
    let serial = node::serial(&mut flash);
    let device = if CLOCK.has_usb() {
        let (device, link) = usb::new(usb, TARGET_TAG, serial);
        links.usb = Some(link);
        Some(device)
    } else {
        None
    };
    let store = AppStore::new(
        SharedFlash::new(CHIP.init(driver(flash)), &REGIONS),
        &CORE,
        0,
        &ERASE_REGIONS,
        // No app runs while the boot core receives.
        dagon_format::statelog::slot::NONE,
    );
    let us: fn() -> u64 = now_us;
    let periods = Periods::new(watchdog_ms(true), watchdog_ms(false));
    let chain = compose(
        Layer::App,
        EVENTS.init(Published::new()),
        Nop,
        Nop,
        Bounds::new(us, periods),
    );
    let cfg = dagon_agent::Config {
        mode: entry.mode(),
        max_payload: links.max_payload() as u16,
        ..node::config(TARGET_TAG, role::NODE)
    };
    let mut agent: Session = Agent::with_links(cfg, VERIFIER, store, chain);
    // The registers log each reset anew, so this is the reset that
    // started the boot core.
    agent.note_reset(dagon_mcu_rp2040::reset::read());
    let mb = if stay.is_some() {
        Mailbox::taken()
    } else {
        Mailbox::new()
    };
    let run = async {
        let usb = async {
            match device {
                Some(d) => usb::run_here(d).await,
                None => core::future::pending().await,
            }
        };
        embassy_futures::join::join(boot_receive::pump(&mut links, &mb), usb).await;
    };
    let run = pin!(run);
    let mut port = PumpPort::new(run, &mb, Wfe::new());
    let mut clock = now_us;
    let first = stay.as_ref();
    boot_receive::serve(
        &mut port,
        &mut agent,
        &mut clock,
        FEED_EVERY_MS,
        &mut feed,
        entry,
        first,
    );
    reset()
}

/// The boot core's watchdog as the slot store sees it: one task, fed
/// directly (the boot core runs nothing else).
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
