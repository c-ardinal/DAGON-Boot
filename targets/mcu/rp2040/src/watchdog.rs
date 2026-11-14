//! The RP2040 watchdog behind the liveness-gated [`Guard`] (see
//! `dagon_mcu_common::guard`). The boot core starts it before the jump with
//! the trial period (`[watchdog] trial_ms`) for an image on trial, else the
//! confirmed one (`confirmed_ms`); the app takes it over with
//! [`Guard::install`], and every feed restarts the period of the image's
//! state then, the confirmed one from its confirmation on.

use embassy_rp::peripherals::WATCHDOG;
use embassy_rp::watchdog::Watchdog;
use embassy_rp::Peri;
use embassy_time::{Duration, Instant, Timer};

pub use dagon_mcu_common::guard::{
    levels, Dog, Lease, PeriodDog, Rearm, Trial, Watch, BUSY_LIMIT_MS, BUSY_MARGIN_MS,
};
pub use dagon_mcu_common::liveness::Level;

/// The RP2040 watchdog and the period it runs (embassy-rp's feed loads
/// the period it is given).
pub struct RpWatchdog(Watchdog, Duration);

impl Rearm for RpWatchdog {
    fn set_period(&mut self, period_ms: u32) {
        self.1 = Duration::from_millis(period_ms.into());
    }

    fn feed(&mut self) {
        self.0.feed(self.1);
    }

    fn reset(&mut self) {
        self.0.trigger_reset();
    }
}

/// The RP2040 watchdog as the guard drives it: the trial period while the
/// image is on trial, the confirmed one once it is confirmed.
pub type Wdt = PeriodDog<RpWatchdog>;

/// The watchdog with the definition's periods (`[watchdog] trial_ms`,
/// `confirmed_ms`).
pub fn wdt(p: Peri<'static, WATCHDOG>) -> Wdt {
    PeriodDog::new(
        RpWatchdog(Watchdog::new(p), Duration::from_millis(0)),
        crate::TRIAL_WDT_MS,
        crate::CONFIRMED_WDT_MS,
    )
}

/// The app's guard over `N` tasks.
pub type Guard<const N: usize> = dagon_mcu_common::guard::Guard<Wdt, N>;

/// Milliseconds since boot (the guard's clock).
pub fn now_ms() -> u64 {
    Instant::now().as_millis()
}

/// Feeds every second through the gate, forever.
pub async fn feed_forever(watch: &dyn Watch) -> ! {
    loop {
        watch.feed();
        Timer::after_secs(1).await;
    }
}
