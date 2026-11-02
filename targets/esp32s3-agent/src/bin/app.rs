//! M5StopWatch app for ota_0 / ota_1: the DAGON agent, HELLO mode app.
#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    dagon_esp32s3_agent::node::run(dagon_proto::payload::mode::APP)
}
