//! DAGON board package for the Waveshare RP2040-Touch-LCD-1.28 (topology
//! board id `waveshare-rp2040-touch-lcd-1.28`): 4 MiB flash, a 240x240
//! round display on SPI1, USB to the host. Its header carries the wiring to
//! other nodes (I2C0, a PIO UART, and the reset/boot lines of an STM32
//! node).
//!
//! Every board fact (pins, buses and their speeds, the boot LED) is read
//! from `dagon-board.toml` when the package builds ([`def`]).
//!
//! | Use | Pins |
//! |---|---|
//! | Display SPI1 (CLK, MOSI), CS, DC, RST, backlight | GPIO10, 11, 9, 8, 13, 25 |
//! | Touch controller reset (held high) | GPIO22 |
//! | Node I2C0 (SDA, SCL), 400 kHz | GPIO16, 17 |
//! | Node PIO0 UART (TX, RX), 8N1 at [`UART_BAUD`] | GPIO26, 27 |
//! | Node NRST (open drain), BOOT0 | GPIO18, 28 |
//! | Diagnostic blink codes | GPIO25 (backlight) |
//!
//! With `hal` (on by default): `Board` implements `BoardCore` and
//! `BoardDef`: DAGON takes the flash, the watchdog and USB; the app gets
//! `Io`. `start` runs the relay (`dagon_relay`, started by
//! `dagon_mcu_rp2040::relay`) over the board's links and hands the app
//! everything else, the display's pins among them (`Spare::display`).
//! Guard tasks: the relay's, then the app's from
//! `dagon_mcu_rp2040::relay::TASKS`. This package binds the I2C0, PIO0 and
//! DMA_IRQ_0 interrupts (`Irqs`).
#![no_std]

/// The board's facts from `dagon-board.toml`.
#[macro_use]
pub mod def {
    include!(concat!(env!("OUT_DIR"), "/def.rs"));

    /// Pins and blocks by their definition names, and `take!`.
    #[cfg(feature = "hal")]
    #[macro_use]
    pub mod pins {
        include!(concat!(env!("OUT_DIR"), "/pins.rs"));
    }
}

#[cfg(feature = "hal")]
mod hal;

#[cfg(feature = "hal")]
pub use hal::*;

pub const BOARD_ID: &str = def::BOARD_ID;
pub const FLASH_SIZE: usize = def::FLASH_BYTES as usize;
/// The backlight doubles as the diagnostic LED (`[boot] led`).
pub const DIAG_LED: u8 = match def::BOOT_LED_GPIO {
    Some(g) => g,
    None => panic!("dagon-board.toml: [boot] led"),
};
/// The level that lights it (`[boot] led_active`).
pub const DIAG_LED_ACTIVE_HIGH: bool = def::BOOT_LED_ACTIVE_HIGH;

pub const I2C_HZ: u32 = def::BUS_CHILD_I2C_HZ;
/// The bit rate of the PIO UART to other nodes: the topology's link speed
/// when dagon builds the node, else bus `child-uart`'s `baud`.
pub const UART_BAUD: u32 = match def::LINK_BAUD {
    Some(b) => b,
    None => panic!("dagon-board.toml: bus child-uart baud"),
};
/// The display bus's speed (bus `lcd`'s `hz`), for `Display::pins`.
pub const LCD_SPI_HZ: u32 = def::BUS_LCD_HZ;
