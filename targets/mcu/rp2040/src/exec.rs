//! DAGON's high-priority executor: an interrupt executor on SWI_IRQ_0 at
//! priority P1. DAGON's link and relay tasks run here, so they preempt the
//! app's tasks in thread mode and never wait on them. Apps must not use
//! SWI_IRQ_0 themselves.

use embassy_executor::{InterruptExecutor, SendSpawner};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};

static EXECUTOR: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_0() {
    // SAFETY: the executor's own interrupt; `start` starts it before use.
    unsafe { EXECUTOR.on_interrupt() }
}

/// Starts the executor (once; `node::start` and `relay::start` call it).
pub fn start() -> SendSpawner {
    interrupt::SWI_IRQ_0.set_priority(Priority::P1);
    EXECUTOR.start(interrupt::SWI_IRQ_0)
}
