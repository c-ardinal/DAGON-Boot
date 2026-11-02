//! Your application. DAGON receives updates, runs a new version on trial and
//! confirms it; dagon gen never rewrites this file.

use dagon_board_waveshare_rp2040_touch_lcd_1_28::Io;
use dagon_mcu_rp2040::watchdog::Lease;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

/// Longest one piece of work may take before the board is reset.
const WORK_BUDGET_MS: u64 = 500;

/// Whether a version on trial may be confirmed (your health check).
pub fn self_check() -> bool {
    true
}

/// Starts the application's task. `io` holds the board's display, touch
/// reset and header pins; `lease` is the task's handle on the watchdog.
pub fn start(spawner: Spawner, io: Io, lease: Lease) {
    spawner.spawn(work(io, lease).expect("task pool"));
}

/// Announces a deadline before each piece of work: a stuck task resets the
/// board, and a version on trial then goes back to the previous one.
#[embassy_executor::task]
async fn work(_io: Io, lease: Lease) {
    loop {
        lease.busy(WORK_BUDGET_MS);
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
