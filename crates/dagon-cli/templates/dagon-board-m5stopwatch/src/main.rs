//! Start-up written by `dagon gen`: the board, the DAGON node (link, update
//! receiver, trial and confirm) and your application in app.rs. You do not
//! need to change this file.
#![no_std]
#![no_main]

mod app;
mod wiring;

use dagon_board_m5stopwatch::{self as board, dagon_pins};
use dagon_mcu_esp32s3::liveness::Liveness;
use dagon_mcu_esp32s3::node::{self, NodeConfig};
use embassy_executor::Spawner;
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

/// Every task of the application registers here; the watchdog is fed only
/// while none of them is overdue.
static LIVENESS: Liveness = Liveness::new();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let mut p = esp_hal::init(esp_hal::Config::default());
    let armed = node::arm_watchdog(p.RTC_TIMER.reborrow());
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    board::start(
        armed,
        dagon_pins!(p),
        NodeConfig::app(app::self_check),
        &LIVENESS,
    );
    app::start(spawner, &LIVENESS);
}
