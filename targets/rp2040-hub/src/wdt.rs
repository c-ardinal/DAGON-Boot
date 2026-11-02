//! Watchdog: the boot core armed it for 8 s. The app feeds it while
//! confirmed, and on trial only during the first 60 s after boot, so an
//! unconfirmed app resets and the boot core counts another attempt.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_rp::watchdog::Watchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use rp2040_common::layout::WDT_MS;

/// Self-feeding window on trial; longer than the host's 45 s reconnect limit.
const TRIAL_FEED: Duration = Duration::from_secs(60);
const FEED_EVERY: Duration = Duration::from_secs(1);

static WDT: Mutex<CriticalSectionRawMutex, RefCell<Option<Watchdog>>> =
    Mutex::new(RefCell::new(None));
static CONFIRMED: AtomicBool = AtomicBool::new(false);

pub fn install(wdt: Watchdog, confirmed: bool) {
    CONFIRMED.store(confirmed, Ordering::Relaxed);
    WDT.lock(|w| *w.borrow_mut() = Some(wdt));
    keepalive();
}

pub fn set_confirmed() {
    CONFIRMED.store(true, Ordering::Relaxed);
}

/// Feeds unless the trial window is over. Also called between flash sectors.
pub fn keepalive() {
    let trial_open = Instant::now() < Instant::from_ticks(0) + TRIAL_FEED;
    if CONFIRMED.load(Ordering::Relaxed) || trial_open {
        WDT.lock(|w| {
            if let Some(w) = w.borrow_mut().as_mut() {
                w.feed(Duration::from_millis(WDT_MS as u64));
            }
        });
    }
}

/// Full chip reset (USB drops, the boot chain runs again).
pub fn reboot() -> ! {
    WDT.lock(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.trigger_reset();
        }
    });
    loop {
        cortex_m::asm::nop();
    }
}

pub async fn run() -> ! {
    loop {
        keepalive();
        Timer::after(FEED_EVERY).await;
    }
}
