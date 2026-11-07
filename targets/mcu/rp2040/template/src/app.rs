//! Your application on the 親機. DAGON relays updates to the 子機,
//! updates this board too, runs a new version on trial and confirms it;
//! dagon gen never rewrites this file.

use dagon_mcu_rp2040::node::App;
use crate::board::Spare;
use dagon_mcu_rp2040::watchdog::{Guard, Lease};
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

/// What your application hands DAGON's start: the self-check above. Add
/// your extensions (`exts: Some(EXTS.init(extensions![..]))`, with a
/// `static EXTS: StaticCell<..>`) to observe every update event (transfer,
/// written, trial, confirmed, rolled back) and answer the decision points;
/// `confirm: Confirm::Own` confirms a version on trial at start, without
/// the 親機. The start returns the node's events for your display or log
/// tasks.
pub fn dagon() -> App {
    App::new(self_check)
}

/// Starts your tasks; each takes one guard task from `first` on. `spare`
/// holds what board.rs leaves for the app.
pub fn start<const N: usize>(
    spawner: Spawner,
    spare: Spare,
    guard: &'static Guard<N>,
    first: usize,
) {
    spawner.spawn(work(spare, guard.lease(first)).expect("task pool"));
}

/// Announces a deadline before each piece of work: a stuck task resets the
/// board, and a version on trial then goes back to the previous one.
#[embassy_executor::task]
async fn work(spare: Spare, lease: Lease) {
    // The pins and peripherals board.rs leaves for the app are in `spare`.
    let _ = spare;
    loop {
        lease.busy(WORK_BUDGET_MS);
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
