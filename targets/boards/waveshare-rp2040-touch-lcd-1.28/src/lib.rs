//! DAGON board package for the Waveshare RP2040-Touch-LCD-1.28 (topology
//! board id `waveshare-rp2040-touch-lcd-1.28`): 4 MiB flash, a 240x240
//! GC9A01 round display on SPI1, USB to the host. It is DAGON's hub board:
//! its header carries the wiring to sub nodes (I2C0, a PIO UART, and the
//! reset/boot lines of an STM32 sub node).
//!
//! Every board fact (pins, buses and their speeds, the boot LED) is read
//! from `dagon-board.toml` when the package builds ([`def`]).
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
//! With `hal` (on by default): `Board` implements `BoardDef`: DAGON takes
//! the flash, the watchdog and USB; the app gets `Io`. `start` runs the
//! DAGON 親機 on this board: the relay (`dagon_mcu_rp2040::relay`) over the
//! board's links and, with `display`, the home screen ([`screen`]). Guard
//! tasks: the relay's 0..3, the display `DISPLAY_TASK`, the app's from
//! `APP_TASKS`. This package binds the I2C0, PIO0 and DMA_IRQ_0 interrupts
//! (`Irqs`). Without `hal` only the screen layout builds (host tests).
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

#[cfg(feature = "display")]
pub mod display;
#[cfg(feature = "hal")]
pub mod gc9a01;
mod glyphs;
#[cfg(feature = "hal")]
mod hal;
pub mod screen;

#[cfg(feature = "hal")]
pub use hal::*;

pub const BOARD_ID: &str = def::BOARD_ID;
pub const FLASH_SIZE: usize = def::FLASH_BYTES as usize;
/// The backlight doubles as the diagnostic LED (`[boot] led`).
pub const DIAG_LED: u8 = match def::BOOT_LED_GPIO {
    Some(g) => g,
    None => panic!("dagon-board.toml: [boot] led"),
};

pub const I2C_HZ: u32 = def::BUS_CHILD_I2C_HZ;
pub const UART_BAUD: u32 = def::BUS_CHILD_UART_BAUD;
/// Conservative for the panel wiring (the controller allows more).
pub const LCD_SPI_HZ: u32 = def::BUS_LCD_HZ;
