//! Your application. DAGON receives updates, runs a new version on trial and
//! confirms it; dagon gen never rewrites this file.

use dagon_board_nucleo_f401re::Io;
use dagon_mcu_stm32f401re::watchdog::Lease;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Duration, Timer};

/// Longest one piece of work may take before the board is reset.
const WORK_BUDGET_MS: u64 = 500;

/// Whether a version on trial may be confirmed (your health check).
pub fn self_check() -> bool {
    true
}

/// Starts the application's task. `io` holds LD2 and the header pins;
/// `lease` is the task's handle on the watchdog.
pub fn start(spawner: Spawner, io: Io, lease: Lease) {
    spawner.spawn(work(io, lease).expect("task pool"));
}

/// Blinks LD2, announcing a deadline before each piece of work: a stuck
/// task resets the board, and a version on trial then goes back to the
/// previous one.
#[embassy_executor::task]
async fn work(io: Io, lease: Lease) {
    let mut led = Output::new(io.led, Level::Low, Speed::Low);
    loop {
        lease.busy(WORK_BUDGET_MS);
        led.toggle();
        lease.idle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
