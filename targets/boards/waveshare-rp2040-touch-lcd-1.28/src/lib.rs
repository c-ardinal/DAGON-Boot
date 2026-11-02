//! DAGON board package for the Waveshare RP2040-Touch-LCD-1.28 (topology
//! board id `waveshare-rp2040-touch-lcd-1.28`): 4 MiB flash, a 240x240
//! GC9A01 round display on SPI1, USB to the host. It is DAGON's hub board:
//! its header carries the wiring to sub nodes (I2C0, a PIO UART, and the
//! reset/boot lines of an STM32 sub node).
//!
//! | Use | Pins |
//! |---|---|
//! | Display SPI1 (CLK, MOSI), CS, DC, RST, backlight | GPIO10, 11, 9, 8, 13, 25 |
//! | Touch controller reset (held high) | GPIO22 |
//! | Sub-node I2C0 (SDA, SCL), 400 kHz | GPIO16, 17 |
//! | Sub-node PIO0 UART (TX, RX), 115200 8N1 | GPIO26, 27 |
//! | Sub-node NRST (open drain), BOOT0 | GPIO18, 28 |
//! | Diagnostic blink codes | GPIO25 (backlight) |
//!
//! [`Board`] implements [`BoardDef`]: DAGON takes the flash, the watchdog
//! and USB; the app gets [`Io`]. The `Io` parts have helpers that set them
//! up the way DAGON's hub does.
#![no_std]

pub mod gc9a01;

use dagon_mcu_rp2040::links::{I2cLink, PioStream, RpI2c};
use dagon_mcu_rp2040::node::DagonParts;
use dagon_mcu_rp2040::sub::SubLines;
pub use dagon_mcu_rp2040::BoardDef;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::interrupt::typelevel::{Binding, DMA_IRQ_0, I2C0_IRQ, PIO0_IRQ_0};
use embassy_rp::peripherals::*;
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::spi::{self, Async, Spi};
use embassy_rp::{dma, Peri, Peripherals};

pub const BOARD_ID: &str = "waveshare-rp2040-touch-lcd-1.28";
pub const FLASH_SIZE: usize = 4 * 1024 * 1024;
/// The backlight doubles as the diagnostic LED.
pub const DIAG_LED: u8 = 25;

pub const I2C_HZ: u32 = 400_000;
pub const UART_BAUD: u32 = 115_200;
/// Conservative for the panel wiring (the controller allows more).
pub const LCD_SPI_HZ: u32 = 20_000_000;

pub struct Board;

impl BoardDef for Board {
    const BOARD_ID: &'static str = BOARD_ID;
    const FLASH_SIZE: usize = FLASH_SIZE;
    const DIAG_LED: Option<u8> = Some(DIAG_LED);
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        let io = Io {
            display: Display {
                spi: p.SPI1,
                clk: p.PIN_10,
                mosi: p.PIN_11,
                dma: p.DMA_CH0,
                cs: p.PIN_9,
                dc: p.PIN_8,
                rst: p.PIN_13,
                backlight: p.PIN_25,
            },
            touch_rst: p.PIN_22,
            sub_i2c: SubI2c {
                bus: p.I2C0,
                sda: p.PIN_16,
                scl: p.PIN_17,
            },
            sub_uart: SubUart {
                pio: p.PIO0,
                tx: p.PIN_26,
                rx: p.PIN_27,
            },
            sub_ctl: SubCtl {
                nrst: p.PIN_18,
                boot0: p.PIN_28,
            },
        };
        (dagon, io)
    }
}

/// What the app gets on this board.
pub struct Io {
    pub display: Display,
    /// Touch controller reset; see [`hold_touch`].
    pub touch_rst: Peri<'static, PIN_22>,
    pub sub_i2c: SubI2c,
    pub sub_uart: SubUart,
    pub sub_ctl: SubCtl,
}

/// Holds the touch controller out of reset and otherwise leaves it alone.
pub fn hold_touch(pin: Peri<'static, PIN_22>) -> Output<'static> {
    Output::new(pin, Level::High)
}

pub struct Display {
    pub spi: Peri<'static, SPI1>,
    pub clk: Peri<'static, PIN_10>,
    pub mosi: Peri<'static, PIN_11>,
    pub dma: Peri<'static, DMA_CH0>,
    pub cs: Peri<'static, PIN_9>,
    pub dc: Peri<'static, PIN_8>,
    pub rst: Peri<'static, PIN_13>,
    pub backlight: Peri<'static, PIN_25>,
}

/// The display's bus and lines, ready for [`gc9a01::Panel::new`].
pub struct DisplayPins {
    pub spi: Spi<'static, SPI1, Async>,
    pub cs: Output<'static>,
    pub dc: Output<'static>,
    pub rst: Output<'static>,
    /// Off: switch it on once a full frame is on the panel.
    pub backlight: Output<'static>,
}

impl Display {
    pub fn pins(
        self,
        irqs: impl Binding<DMA_IRQ_0, dma::InterruptHandler<DMA_CH0>> + 'static,
    ) -> DisplayPins {
        let mut cfg = spi::Config::default();
        cfg.frequency = LCD_SPI_HZ;
        DisplayPins {
            spi: Spi::new_txonly(self.spi, self.clk, self.mosi, self.dma, irqs, cfg),
            cs: Output::new(self.cs, Level::High),
            dc: Output::new(self.dc, Level::Low),
            rst: Output::new(self.rst, Level::High),
            backlight: Output::new(self.backlight, Level::Low),
        }
    }
}

/// I2C0 to sub nodes.
pub struct SubI2c {
    pub bus: Peri<'static, I2C0>,
    pub sda: Peri<'static, PIN_16>,
    pub scl: Peri<'static, PIN_17>,
}

impl SubI2c {
    /// I2C0 as controller at [`I2C_HZ`], with the DAGON mailbox link.
    pub fn link(
        self,
        irqs: impl Binding<I2C0_IRQ, i2c::InterruptHandler<I2C0>>,
    ) -> I2cLink<RpI2c<'static, I2C0>> {
        let mut cfg = i2c::Config::default();
        cfg.frequency = I2C_HZ;
        I2cLink::new(RpI2c(I2c::new_async(self.bus, self.scl, self.sda, irqs, cfg)))
    }
}

/// A UART to sub nodes, on PIO0 (state machines 0 and 1).
pub struct SubUart {
    pub pio: Peri<'static, PIO0>,
    pub tx: Peri<'static, PIN_26>,
    pub rx: Peri<'static, PIN_27>,
}

impl SubUart {
    /// The PIO UART at [`UART_BAUD`] as a byte stream.
    pub fn stream(
        self,
        irqs: impl Binding<PIO0_IRQ_0, pio::InterruptHandler<PIO0>>,
    ) -> PioStream<'static, PIO0, 0, 1> {
        let Pio {
            mut common,
            sm0,
            sm1,
            ..
        } = Pio::new(self.pio, irqs);
        let tx_prog = PioUartTxProgram::new(&mut common);
        let rx_prog = PioUartRxProgram::new(&mut common);
        let tx = PioUartTx::new(UART_BAUD, &mut common, sm0, self.tx, &tx_prog);
        let rx = PioUartRx::new(UART_BAUD, &mut common, sm1, self.rx, &rx_prog);
        // The UART runs for the life of the app: never release the PIO.
        core::mem::forget(common);
        PioStream { tx, rx }
    }
}

/// Reset and boot-mode lines of an STM32 sub node (ROM recovery path).
pub struct SubCtl {
    pub nrst: Peri<'static, PIN_18>,
    pub boot0: Peri<'static, PIN_28>,
}

impl SubCtl {
    pub fn lines(self) -> SubLines<'static> {
        SubLines::new(self.nrst, self.boot0)
    }
}
