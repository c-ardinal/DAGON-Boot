//! M5StopWatch factory recovery agent: the same agent, HELLO mode agent. The
//! 2nd stage boots it when otadata selects nothing bootable.
#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    dagon_esp32s3_agent::node::run(dagon_proto::payload::mode::AGENT)
}
