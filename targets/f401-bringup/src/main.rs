//! NUCLEO-F401RE bring-up: blink LD2 (PA5) and log a heartbeat over RTT.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Duration, Timer};
use panic_probe as _;

const BLINK: Duration = Duration::from_millis(250);

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let mut led = Output::new(p.PA5, Level::Low, Speed::Low);
    defmt::info!("f401 bring-up running");
    let mut n: u32 = 0;
    loop {
        led.toggle();
        Timer::after(BLINK).await;
        n = n.wrapping_add(1);
        if n.is_multiple_of(8) {
            defmt::info!("tick {}", n / 8);
        }
    }
}
