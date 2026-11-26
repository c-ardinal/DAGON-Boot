//! The relay firmware of this MCU: a node with every relay link of the
//! package wired (both I2C buses and both PIO UARTs) and no app tasks of its
//! own, started as a generated node starts it (template/src/main.rs).
//! scripts/relay_ram.py builds it and checks its static RAM (`.bss` and
//! `.data`) against the definition's `[relay] ram_bytes`.
#![no_std]
#![no_main]

use dagon_mcu_rp2040::layout::MIN_FLASH_SIZE;
use dagon_mcu_rp2040::links::{I2cLink, PioStream, RpI2c};
use dagon_mcu_rp2040::node::{App, BoardCore, BoardDef, DagonParts};
use dagon_mcu_rp2040::relay::{self, Links, Setup};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
use dagon_mcu_rp2040::{EraseRegion, ERASE_REGIONS};
use embassy_executor::Spawner;
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{
    I2C0, I2C1, PIN_10, PIN_11, PIN_4, PIN_5, PIN_6, PIN_7, PIN_8, PIN_9, PIO0, PIO1,
};
use embassy_rp::pio::Instance;
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::{bind_interrupts, Peri, Peripherals};
use panic_halt as _;

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
});

const UART_BAUD: u32 = 115_200;

static GUARD: Guard<{ relay::TASKS }> = Guard::new(
    relay::levels::<{ relay::TASKS }>(),
    Trial::FeedUntilMs(60_000),
    now_ms,
);

struct Board;

struct Io {
    i2c: (
        Peri<'static, I2C0>,
        Peri<'static, PIN_4>,
        Peri<'static, PIN_5>,
    ),
    i2c1: (
        Peri<'static, I2C1>,
        Peri<'static, PIN_6>,
        Peri<'static, PIN_7>,
    ),
    uart: (
        Peri<'static, PIO0>,
        Peri<'static, PIN_8>,
        Peri<'static, PIN_9>,
    ),
    uart1: (
        Peri<'static, PIO1>,
        Peri<'static, PIN_10>,
        Peri<'static, PIN_11>,
    ),
}

impl BoardCore for Board {
    const BOARD_ID: &'static str = "relay";
    const FLASH_SIZE: usize = MIN_FLASH_SIZE;
}

impl BoardDef for Board {
    const ERASE_REGIONS: &'static [EraseRegion] = &ERASE_REGIONS;
    const DIAG_LED: Option<u8> = None;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        let io = Io {
            i2c: (p.I2C0, p.PIN_4, p.PIN_5),
            i2c1: (p.I2C1, p.PIN_6, p.PIN_7),
            uart: (p.PIO0, p.PIN_8, p.PIN_9),
            uart1: (p.PIO1, p.PIN_10, p.PIN_11),
        };
        (dagon, io)
    }
}

/// A PIO UART on state machines 0 (TX) and 1 (RX) of `pio`.
fn pio_uart<P: Instance>(
    pio: Pio<'static, P>,
    tx: Peri<'static, impl pio::PioPin>,
    rx: Peri<'static, impl pio::PioPin>,
) -> PioStream<'static, P, 0, 1> {
    let Pio {
        mut common,
        sm0,
        sm1,
        ..
    } = pio;
    let tx_prog = PioUartTxProgram::new(&mut common);
    let rx_prog = PioUartRxProgram::new(&mut common);
    let tx = PioUartTx::new(UART_BAUD, &mut common, sm0, tx, &tx_prog);
    let rx = PioUartRx::new(UART_BAUD, &mut common, sm1, rx, &rx_prog);
    core::mem::forget(common);
    PioStream { tx, rx }
}

fn links(io: Io) -> Links {
    let (bus, sda, scl) = io.i2c;
    let i2c = I2cLink::new(
        RpI2c(I2c::new_async(bus, scl, sda, Irqs, i2c::Config::default())),
        dagon_mcu_rp2040::RELAY_LINK_I2C0,
    );
    let (bus, sda, scl) = io.i2c1;
    let i2c1 = I2cLink::new(
        RpI2c(I2c::new_async(bus, scl, sda, Irqs, i2c::Config::default())),
        dagon_mcu_rp2040::RELAY_LINK_I2C1,
    );
    let (pio, tx, rx) = io.uart;
    let uart = pio_uart(Pio::new(pio, Irqs), tx, rx);
    let (pio, tx, rx) = io.uart1;
    let uart1 = pio_uart(Pio::new(pio, Irqs), tx, rx);
    Links {
        i2c: Some(i2c),
        i2c1: Some(i2c1),
        uart: Some(uart),
        uart1: Some(uart1),
        ..Links::none()
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    let setup = Setup {
        tag: dagon_mcu_rp2040::target_tag!(),
        role: relay::role::NODE,
        name: "relay",
        app: App::new(|| true),
        relay: Default::default(),
    };
    relay::start::<Board, { relay::TASKS }>(spawner, dagon, links(io), &GUARD, setup);
}
