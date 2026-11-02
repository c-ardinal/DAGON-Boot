//! Your application on the 親機. DAGON relays updates to the 子機,
//! updates this board too, runs a new version on trial and confirms it;
//! dagon gen never rewrites this file.

use dagon_board_waveshare_rp2040_touch_lcd_1_28::Spare;
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

/// Starts your tasks; each takes one guard task from `first` on. `spare`
/// holds the pins and peripherals DAGON does not use (GPIOs, I2C1, SPI0,
/// UART0/1, PIO1, ADC, DMA channels).
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
    // For example, a GPIO as an output:
    //   let mut out = embassy_rp::gpio::Output::new(spare.pin_0, embassy_rp::gpio::Level::Low);
    let _ = spare;
    loop {
        lease.busy(WORK_BUDGET_MS);
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
