//! Your application. DAGON receives updates, runs a new version on trial and
//! confirms it; dagon gen never rewrites this file.

use dagon_board_nucleo_f401re::{running_version, seg, Io};
use dagon_mcu_stm32f401re::watchdog::{Guard, Lease};
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
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
/// holds LD2, the X-NUCLEO-53L0A1's 7-segment display (I2C1) and
/// `io.spare` (USART2, B1, ADC1 and their pins).
pub fn start<const N: usize>(spawner: Spawner, io: Io, guard: &'static Guard<N>, first: usize) {
    spawner.spawn(work(io, guard.lease(first)).expect("task pool"));
}

/// Shows the running version on the display, then blinks LD2, announcing a
/// deadline before each piece of work: a stuck task resets the board, and a
/// version on trial then goes back to the previous one.
#[embassy_executor::task]
async fn work(io: Io, lease: Lease) {
    // For example, the user button B1: `Input::new(io.spare.pc13, Pull::None)`.
    let mut led = Output::new(io.led, Level::Low, Speed::Low);
    let mut display = io.display.open();
    lease.busy(WORK_BUDGET_MS);
    // Without the shield the display stays dark; the node runs on.
    let _ = display.show(seg::version(running_version())).await;
    lease.idle();
    loop {
        lease.busy(WORK_BUDGET_MS);
        led.toggle();
        lease.idle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
