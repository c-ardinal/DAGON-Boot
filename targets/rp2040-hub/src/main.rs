//! DAGON hub app (RP2040-Touch-LCD-1.28).
//!
//! USB CDC carries DAGON frames from the PC. RELAY goes to the F401 on I2C0
//! or the watch on the PIO UART, SHOW drives the LCD progress screen, SUBCTL
//! and I2C_RAW drive the F401 ROM path; everything else (HELLO, STATUS,
//! XFER_*, SET_PENDING, REBOOT, CONFIRM) is the hub's own agent, which
//! updates the inactive app slot. While the PC is quiet, the hub polls each
//! node with a HELLO every 2 s and shows the result on the LCD home screen.
#![no_std]
#![no_main]

mod gc9a01;
mod hub;
mod lcd;
mod nodes;
mod relay;
mod store;
mod sub;
mod wdt;

use dagon_agent::{Agent, Config, DefaultHooks, SlotStore};
use dagon_format::header::{mcu_id, port_kind, target_tag};
use dagon_proto::payload::{mode, role, SlotState, StatusResp};
use dagon_proto::{max_payload, MAX_FRAME};
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C0, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::spi::{self, Spi};
use embassy_rp::usb::{self, Driver};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Instant;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use rp2040_common::arm::step;
use rp2040_common::layout::HUB_TAG;
use rp2040_common::rp::RpFlash;

use hub::{Hub, USB_PACKET};
use lcd::Home;
use relay::{Links, PioStream, RpI2c};
use store::HubStore;

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

const USB_VID: u16 = 0xc0de;
const USB_PID: u16 = 0xcafe;
const PIO_UART_BAUD: u32 = 115_200;
const I2C_HZ: u32 = 400_000;
/// Conservative for the panel wiring (the controller allows more).
const LCD_SPI_HZ: u32 = 20_000_000;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Panic blink codes: 10 before HAL init, 11 after it, 12 at USB setup,
    // 13 polling nodes, 14 answering the PC, 15 drawing.
    step(10);
    let p = embassy_rp::init(Default::default());
    step(11);

    let mut store = HubStore {
        flash: RpFlash::new(p.FLASH),
    };
    let st = store.status();
    wdt::install(Watchdog::new(p.WATCHDOG), !on_trial(&st));
    let serial_hex = hex(&store.flash.unique_id());
    let serial = core::str::from_utf8(&serial_hex).unwrap_or("0");

    let sub = sub::SubLines::new(p.PIN_18, p.PIN_28);

    let Pio {
        mut common,
        sm0,
        sm1,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let tx_prog = PioUartTxProgram::new(&mut common);
    let rx_prog = PioUartRxProgram::new(&mut common);
    let tx = PioUartTx::new(PIO_UART_BAUD, &mut common, sm0, p.PIN_26, &tx_prog);
    let rx = PioUartRx::new(PIO_UART_BAUD, &mut common, sm1, p.PIN_27, &rx_prog);
    let mut i2c_cfg = i2c::Config::default();
    i2c_cfg.frequency = I2C_HZ;
    let i2c = I2c::new_async(p.I2C0, p.PIN_17, p.PIN_16, Irqs, i2c_cfg);

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = LCD_SPI_HZ;
    let lcd_pins = lcd::Pins {
        spi: Spi::new_blocking_txonly(p.SPI1, p.PIN_10, p.PIN_11, spi_cfg),
        cs: Output::new(p.PIN_9, Level::High),
        dc: Output::new(p.PIN_8, Level::Low),
        rst: Output::new(p.PIN_13, Level::High),
        // Off until the panel is reset, set up and cleared.
        backlight: Output::new(p.PIN_25, Level::Low),
    };

    step(12);
    let mut usb_cfg = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_cfg.manufacturer = Some("DAGON");
    usb_cfg.product = Some("DAGON hub");
    usb_cfg.serial_number = Some(serial);
    let mut config_desc = [0; 256];
    let mut bos_desc = [0; 256];
    let mut control_buf = [0; 64];
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        Driver::new(p.USB, Irqs),
        usb_cfg,
        &mut config_desc,
        &mut bos_desc,
        &mut [],
        &mut control_buf,
    );
    let mut class = CdcAcmClass::new(&mut builder, &mut state, USB_PACKET);
    let mut usb = builder.build();

    let agent = Agent::new(
        Config {
            pubkey: PUBKEY,
            mcu_id: mcu_id::RP2040,
            port_kind: port_kind::SELF_BOOT,
            target_tag: target_tag(HUB_TAG),
            role: role::HUB,
            mode: mode::APP,
            max_payload: max_payload::USB as u16,
        },
        store,
        DefaultHooks,
    );
    let mut hub = Hub {
        agent,
        links: Links::new(RpI2c(i2c), PioStream { tx, rx }),
        sub,
        resp: [0; MAX_FRAME],
        quiet_until: Instant::now(),
        poll_seq: 0x80,
        home: Home::default(),
    };

    join4(
        wdt::run(),
        usb.run(),
        hub.serve(&mut class),
        lcd::run(lcd_pins),
    )
    .await;
}

fn on_trial(st: &StatusResp) -> bool {
    st.slots.get(st.active as usize).map(|s| s.state) == Some(SlotState::Trying)
}

fn hex(id: &[u8; 8]) -> [u8; 16] {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; 16];
    for (i, b) in id.iter().enumerate() {
        out[2 * i] = DIGITS[(b >> 4) as usize];
        out[2 * i + 1] = DIGITS[(b & 0xF) as usize];
    }
    out
}
