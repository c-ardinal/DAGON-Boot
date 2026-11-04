//! Your application. DAGON receives updates, runs a new version on trial and
//! confirms it; dagon gen never rewrites this file.

use dagon_mcu_esp32s3::node::App;
use dagon_mcu_esp32s3::liveness::{Lease, Liveness};
use dagon_mcu_esp32s3::node;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

/// Longest one piece of work may take before the watch is reset.
const WORK_BUDGET_MS: u32 = 500;

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

/// Starts the application's tasks. Each task takes a lease and announces a
/// deadline before each piece of work; a stuck task resets the watch, and a
/// version on trial then goes back to the previous one.
pub fn start(spawner: Spawner, live: &'static Liveness) {
    if let Some(lease) = live.register() {
        if let Ok(task) = work(lease) {
            spawner.spawn(task);
        }
    }
}

#[embassy_executor::task]
async fn work(lease: Lease) {
    loop {
        lease.busy(node::now_ms(), WORK_BUDGET_MS);
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
