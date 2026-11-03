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
//! With `hal` (on by default): `Board` implements `BoardDef`: DAGON takes
//! the flash, the watchdog and USB; the app gets `Io`. `start` runs the
//! DAGON 親機 on this board: the relay (`dagon_mcu_rp2040::relay`) over the
//! board's links and, with `display`, the home screen ([`screen`]). Guard
//! tasks: the relay's 0..3, the display `DISPLAY_TASK`, the app's from
//! `APP_TASKS`. This package binds the I2C0, PIO0 and DMA_IRQ_0 interrupts
//! (`Irqs`). Without `hal` only the screen layout builds (host tests).
#![no_std]

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

pub const BOARD_ID: &str = "waveshare-rp2040-touch-lcd-1.28";
pub const FLASH_SIZE: usize = 4 * 1024 * 1024;
/// The backlight doubles as the diagnostic LED.
pub const DIAG_LED: u8 = 25;

pub const I2C_HZ: u32 = 400_000;
pub const UART_BAUD: u32 = 115_200;
/// Conservative for the panel wiring (the controller allows more).
pub const LCD_SPI_HZ: u32 = 20_000_000;
