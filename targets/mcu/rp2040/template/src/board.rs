//! Your board, for DAGON: its flash size and the 親機's links to the 子機.
//! Written once by `dagon new` with a common wiring (I2C0 on GPIO16/17 to
//! the 子機); edit it for your board (dagon gen never rewrites it).

use dagon_mcu_rp2040::links::{I2cLink, RpI2c};
use dagon_mcu_rp2040::node::{BoardDef, DagonParts};
use dagon_mcu_rp2040::relay::Links;
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C0, PIN_16, PIN_17, PIN_25};
use embassy_rp::{bind_interrupts, Peri, Peripherals};

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
});

/// I2C speed to the 子機 (as in the topology).
const I2C_HZ: u32 = 400_000;

pub struct Board;

/// What DAGON's relay and your app get besides DAGON's own peripherals.
pub struct Io {
    pub i2c: Peri<'static, I2C0>,
    pub sda: Peri<'static, PIN_16>,
    pub scl: Peri<'static, PIN_17>,
    pub spare: Spare,
}

/// What your app gets. Add the pins and peripherals your app uses.
pub struct Spare {
    /// GPIO25 (the LED on Raspberry Pi Pico-style boards).
    pub pin_25: Peri<'static, PIN_25>,
}

impl BoardDef for Board {
    const BOARD_ID: &'static str = "{{node}}";
    /// Your board's flash; DAGON's layout needs at least 4 MiB.
    const FLASH_SIZE: usize = 4 * 1024 * 1024;
    /// An LED for DAGON's diagnostic blink codes, if any.
    const DIAG_LED: Option<u8> = None;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        let io = Io {
            i2c: p.I2C0,
            sda: p.PIN_16,
            scl: p.PIN_17,
            spare: Spare { pin_25: p.PIN_25 },
        };
        (dagon, io)
    }
}

/// The relay's links: I2C0 to the 子機 (no PIO UART, no STM32 recovery
/// lines on this wiring; add them like the Waveshare board package does),
/// and what is left for the app.
pub fn links(io: Io) -> (Links, Spare) {
    let mut cfg = i2c::Config::default();
    cfg.frequency = I2C_HZ;
    let bus = I2c::new_async(io.i2c, io.scl, io.sda, Irqs, cfg);
    let links = Links {
        i2c: Some(I2cLink::new(RpI2c(bus))),
        uart: None,
        sub: None,
    };
    (links, io.spare)
}
