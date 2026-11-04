//! Your application. DAGON receives updates, runs a new version on trial and
//! confirms it; dagon gen never rewrites this file.

use crate::board::Io;
use dagon_mcu_stm32f401re::watchdog::{Guard, Lease};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

/// How many watchdog guard tasks your application uses.
pub const TASKS: usize = 1;

/// Longest one piece of work may take before the board is reset.
const WORK_BUDGET_MS: u64 = 500;

/// Whether a version on trial may be confirmed (your health check).
pub fn self_check() -> bool {
    true
}

/// Starts your tasks; each takes one guard task from `first` on. `io`
/// holds what board.rs gives the app.
pub fn start<const N: usize>(spawner: Spawner, io: Io, guard: &'static Guard<N>, first: usize) {
    spawner.spawn(work(io, guard.lease(first)).expect("task pool"));
}

/// Announces a deadline before each piece of work: a stuck task resets
/// the board, and a version on trial then goes back to the previous one.
#[embassy_executor::task]
async fn work(io: Io, lease: Lease) {
    // The pins and peripherals board.rs gives the app are in `io`.
    let _ = io;
    loop {
        lease.busy(WORK_BUDGET_MS);
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
