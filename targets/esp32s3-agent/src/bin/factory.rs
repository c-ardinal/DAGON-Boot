//! M5StopWatch factory recovery agent: the DAGON link task alone, HELLO mode
//! agent. The 2nd stage boots it when otadata selects nothing bootable.
#![no_std]
#![no_main]

use dagon_board_m5stopwatch::{self as board, dagon_pins};
use dagon_mcu_esp32s3::liveness::Liveness;
use dagon_mcu_esp32s3::node::{self, NodeConfig};
use embassy_executor::Spawner;
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

static LIVENESS: Liveness = Liveness::new();

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    let mut p = esp_hal::init(esp_hal::Config::default());
    let armed = node::arm_watchdog(p.RTC_TIMER.reborrow());
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    board::start(armed, dagon_pins!(p), NodeConfig::factory(), &LIVENESS);
}
