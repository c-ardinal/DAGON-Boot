//! Start-up written by `dagon gen` for an ESP32-S3 board chosen by MCU: the
//! DAGON node (link, update receiver, trial and confirm) on the UART your
//! board wires to the hub, and your application in app.rs. Set the link pins
//! below once; the rest needs no change.
#![no_std]
#![no_main]

mod app;
mod wiring;

use dagon_mcu_esp32s3::links::{self, Kind};
use dagon_mcu_esp32s3::liveness::Liveness;
use dagon_mcu_esp32s3::link;
use dagon_mcu_esp32s3::node::{self, NodeConfig, NodePeripherals};
use embassy_executor::Spawner;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::uart::Uart;

esp_bootloader_esp_idf::esp_app_desc!();

/// The hops of this node, from the 構成 (dagon gen writes them): the
/// build stops here, naming the hop, if a kind is not available yet. The
/// node serves UART and USB Serial/JTAG at once, whichever is the uplink.
#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);

/// The bit rate of this node's UART link, from the 構成 (wiring.rs).
const LINK_BAUD: u32 = match wiring::NODE.link {
    wiring::Link::Uart { baud } => baud,
    _ => panic!("this start-up serves a UART link to the 親機"),
};

/// Every task of the application registers here; the watchdog is fed only
/// while none of them is overdue.
static LIVENESS: Liveness = Liveness::new();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let mut p = esp_hal::init(esp_hal::Config::default());
    let armed = node::arm_watchdog(p.RTC_TIMER.reborrow());
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    // The DAGON link: UART0 on its IO MUX pins, U0TXD = GPIO43 and
    // U0RXD = GPIO44 (function 0; Espressif's esp-metadata for the
    // ESP32-S3), at the bit rate of this node's link in the 構成. A board
    // that wires the hub to other pins moves them here.
    let uart = Uart::new(p.UART0, link::uart_config(LINK_BAUD))
        .unwrap_or_else(|_| esp_hal::system::software_reset())
        .with_tx(p.GPIO43)
        .with_rx(p.GPIO44);
    let dagon = NodePeripherals {
        rtc_timer: p.RTC_TIMER,
        uart,
        usb_device: p.USB_DEVICE,
        flash: p.FLASH,
        target_tag: wiring::NODE.tag,
        board: node::plain_board(),
        board_extension: None,
    };
    node::start(
        armed,
        dagon,
        p.FROM_CPU_INTR1,
        NodeConfig::app(app::dagon()),
        &LIVENESS,
    );
    app::start(spawner, &LIVENESS);
}
