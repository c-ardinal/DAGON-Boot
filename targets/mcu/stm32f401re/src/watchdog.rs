//! The independent watchdog (IWDG) behind the liveness-gated [`Guard`]
//! (see `dagon_mcu_common::guard`). It runs from start-up on: the boot
//! core starts it ([`TRIAL_WDT_MS`](crate::layout::TRIAL_WDT_MS) on
//! trial, else [`CONFIRMED_WDT_MS`](crate::layout::CONFIRMED_WDT_MS)) and
//! the app takes it over with the same period. Nodes on this MCU never feed
//! on trial ([`Trial::NeverFeed`]): an image without CONFIRM resets when
//! the trial period runs out and the boot core counts the attempt.

use embassy_stm32::peripherals::IWDG;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_time::Instant;

pub use dagon_mcu_common::guard::{
    levels, Dog, Lease, Trial, Watch, BUSY_LIMIT_MS, BUSY_MARGIN_MS,
};
pub use dagon_mcu_common::liveness::Level;

/// The IWDG as the guard drives it.
pub struct Iwdg(pub IndependentWatchdog<'static, IWDG>);

impl Dog for Iwdg {
    fn feed(&mut self) {
        self.0.pet();
    }

    fn reset(&mut self) {
        cortex_m::peripheral::SCB::sys_reset();
    }
}

/// The app's guard over `N` tasks.
pub type Guard<const N: usize> = dagon_mcu_common::guard::Guard<Iwdg, N>;

/// Milliseconds since boot (the guard's clock).
pub fn now_ms() -> u64 {
    Instant::now().as_millis()
}
