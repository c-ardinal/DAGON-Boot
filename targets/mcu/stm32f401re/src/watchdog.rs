//! The independent watchdog (IWDG) behind the liveness-gated [`Guard`]
//! (see `dagon_node::guard`). It runs from start-up on: the boot
//! core starts it ([`TRIAL_WDT_MS`](crate::layout::TRIAL_WDT_MS) on
//! trial, else [`CONFIRMED_WDT_MS`](crate::layout::CONFIRMED_WDT_MS)) and
//! the app takes it over and sets the confirmed period once the image is
//! confirmed. Nodes on this MCU never feed
//! on trial ([`Trial::NeverFeed`]): an image without CONFIRM resets when
//! the trial period runs out and the boot core counts the attempt.

use embassy_stm32::peripherals::IWDG;
use embassy_stm32::Peri;
use embassy_time::Instant;

use dagon_port::{Reset, ResetCause, Watchdog};

pub use dagon_mcu_common::liveness::Level;
pub use dagon_node::guard::{
    levels, Dog, Lease, PeriodDog, Trial, Watch, BUSY_LIMIT_MS, BUSY_MARGIN_MS,
};

/// The running IWDG (owning its peripheral), programmed by [`crate::iwdg`].
pub struct Iwdg(Peri<'static, IWDG>);

impl Iwdg {
    /// The IWDG, already started (by the boot core or [`start`]).
    pub fn new(p: Peri<'static, IWDG>) -> Self {
        Iwdg(p)
    }
}

impl Watchdog for Iwdg {
    /// Starts the IWDG with `timeout_ms` (on a running IWDG: sets the
    /// period and reloads it).
    fn start(&mut self, timeout_ms: u32) {
        crate::iwdg::start(timeout_ms);
    }

    fn feed(&mut self) {
        crate::iwdg::feed();
    }
}

impl Reset for Iwdg {
    fn reset(&mut self) -> ! {
        cortex_m::peripheral::SCB::sys_reset()
    }

    /// The cause the reset flags name, left as they are (the app's start
    /// reads and clears them with [`crate::reset::take`]).
    fn reset_cause(&self) -> ResetCause {
        crate::reset::peek()
    }
}

/// The app's guard over `N` tasks.
pub type Guard<const N: usize> = dagon_node::guard::Guard<PeriodDog<Iwdg>, N>;

/// Milliseconds since boot (the guard's clock).
pub fn now_ms() -> u64 {
    Instant::now().as_millis()
}
