//! DAGON's high-priority executor: an interrupt executor on the USART6
//! interrupt (USART6 itself stays unused) at priority P6. DAGON's serving
//! task runs here, so it preempts the app's tasks in thread mode and the
//! hub is answered whatever they do. Apps must not use USART6.

use embassy_executor::{InterruptExecutor, SendSpawner};
use embassy_stm32::interrupt;
use embassy_stm32::interrupt::{InterruptExt, Priority};

static EXECUTOR: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn USART6() {
    // SAFETY: the executor's own interrupt (USART6 is unused); `start`
    // starts it before use.
    unsafe { EXECUTOR.on_interrupt() }
}

/// Starts the executor (once; `node::start` calls it).
pub fn start() -> SendSpawner {
    interrupt::USART6.set_priority(Priority::P6);
    EXECUTOR.start(interrupt::USART6)
}
