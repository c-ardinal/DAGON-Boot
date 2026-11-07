//! The RP2040 watchdog behind the liveness-gated [`Guard`] (see
//! `dagon_mcu_common::guard`). The boot core starts it for
//! [`WDT_MS`](crate::layout::WDT_MS) before the jump; the app takes it
//! over with [`Guard::install`] and every feed restarts that period.

use embassy_rp::peripherals::WATCHDOG;
use embassy_rp::watchdog::Watchdog;
use embassy_rp::Peri;
use embassy_time::{Duration, Instant, Timer};

pub use dagon_mcu_common::guard::{
    levels, Dog, Lease, Trial, Watch, BUSY_LIMIT_MS, BUSY_MARGIN_MS,
};
pub use dagon_mcu_common::liveness::Level;

use crate::layout::WDT_MS;

/// The RP2040 watchdog as the guard drives it.
pub struct Wdt(pub Watchdog);

impl Wdt {
    pub fn new(p: Peri<'static, WATCHDOG>) -> Self {
        Wdt(Watchdog::new(p))
    }
}

impl Dog for Wdt {
    fn feed(&mut self) {
        self.0.feed(Duration::from_millis(WDT_MS as u64));
    }

    fn reset(&mut self) {
        self.0.trigger_reset();
    }
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
