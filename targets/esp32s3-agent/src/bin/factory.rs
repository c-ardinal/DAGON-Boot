//! M5StopWatch factory recovery agent: the DAGON link task alone, HELLO mode
//! agent. The 2nd stage boots it when otadata selects nothing bootable.
#![no_std]
#![no_main]

use dagon_esp32s3_agent::liveness::Liveness;
use dagon_esp32s3_agent::node::{self, NodeConfig, NodePeripherals};
use embassy_executor::Spawner;
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

static LIVENESS: Liveness = Liveness::new();

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    let p = esp_hal::init(esp_hal::Config::default());
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    let dagon = NodePeripherals {
        rtc_timer: p.RTC_TIMER,
        uart0: p.UART0,
        gpio43: p.GPIO43,
        gpio44: p.GPIO44,
        usb_device: p.USB_DEVICE,
        flash: p.FLASH,
        i2c0: p.I2C0,
        gpio47: p.GPIO47,
        gpio48: p.GPIO48,
    };
    node::start(dagon, p.FROM_CPU_INTR1, NodeConfig::factory(), &LIVENESS);
}
