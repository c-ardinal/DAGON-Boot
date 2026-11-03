//! Your board, for DAGON: its flash size and the 親機's links to the 子機.
//! Written once by `dagon new` with a common wiring (downlink `i2c`: I2C0
//! on GPIO16/17; downlink `uart`: a PIO0 UART on GPIO26/27); edit it for
//! your board (dagon gen never rewrites it).

use dagon_mcu_rp2040::links::{I2cLink, Kind, PioStream, RpI2c};
use dagon_mcu_rp2040::node::{BoardDef, DagonParts};
use dagon_mcu_rp2040::relay::Links;
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C0, PIN_16, PIN_17, PIN_25, PIN_26, PIN_27, PIO0};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::{bind_interrupts, Peri, Peripherals};

bind_interrupts!(struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

/// I2C speed to the 子機 (as in the topology).
const I2C_HZ: u32 = 400_000;
/// UART speed to the 子機 (as in the topology).
const UART_BAUD: u32 = 115_200;

pub struct Board;

/// What DAGON's relay and your app get besides DAGON's own peripherals.
pub struct Io {
    pub i2c: Peri<'static, I2C0>,
    pub sda: Peri<'static, PIN_16>,
    pub scl: Peri<'static, PIN_17>,
    pub pio: Peri<'static, PIO0>,
    pub tx: Peri<'static, PIN_26>,
    pub rx: Peri<'static, PIN_27>,
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
            pio: p.PIO0,
            tx: p.PIN_26,
            rx: p.PIN_27,
            spare: Spare { pin_25: p.PIN_25 },
        };
        (dagon, io)
    }
}

/// The relay's links for the 構成's hops (the uplink is the USB CDC
/// device, built by the relay; no STM32 recovery lines on this wiring),
/// and what is left for the app.
pub fn links(io: Io, uplink: Kind, downlinks: &[Kind]) -> (Links, Spare) {
    debug_assert!(uplink == Kind::UsbCdc);
    let i2c = downlinks.contains(&Kind::I2c).then(|| {
        let mut cfg = i2c::Config::default();
        cfg.frequency = I2C_HZ;
        I2cLink::new(RpI2c(I2c::new_async(io.i2c, io.scl, io.sda, Irqs, cfg)))
    });
    let uart = downlinks.contains(&Kind::Uart).then(|| {
        let Pio {
            mut common,
            sm0,
            sm1,
            ..
        } = Pio::new(io.pio, Irqs);
        let tx_prog = PioUartTxProgram::new(&mut common);
        let rx_prog = PioUartRxProgram::new(&mut common);
        let tx = PioUartTx::new(UART_BAUD, &mut common, sm0, io.tx, &tx_prog);
        let rx = PioUartRx::new(UART_BAUD, &mut common, sm1, io.rx, &rx_prog);
        // The UART runs for the life of the app: never release the PIO.
        core::mem::forget(common);
        PioStream { tx, rx }
    });
    let links = Links {
        i2c,
        uart,
        sub: None,
    };
    (links, io.spare)
}
