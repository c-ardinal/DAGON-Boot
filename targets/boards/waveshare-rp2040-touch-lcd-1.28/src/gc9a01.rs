//! GC9A01 240x240 round panel over 4-wire SPI (mode 0). The picture is
//! drawn into a frame buffer in RAM ([`Frame`]); [`Panel::flush`] sends the
//! whole frame with one DMA transfer (window 0..239 set, RAMWR, CS held low
//! for the stream). Commands are framed by CS with DC low for the command
//! byte and high for its parameters. Pixels are RGB565, high byte first.
//! The init sequence is the panel vendor's (manufacturer registers, power,
//! gamma), then inversion on, sleep out, display on.

use core::convert::Infallible;

use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Async, Spi};
use embassy_time::Timer;
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

pub const SIZE: u32 = 240;
/// Bytes of one full frame.
pub const FRAME_BYTES: usize = (SIZE * SIZE * 2) as usize;

const CASET: u8 = 0x2A;
const RASET: u8 = 0x2B;
const RAMWR: u8 = 0x2C;
const SLPOUT: u8 = 0x11;
const DISPON: u8 = 0x29;
const INVON: u8 = 0x21;

/// MADCTL: row and column order flipped (upright on this board), BGR panel.
const MADCTL: u8 = 0xC8;
/// COLMOD: 16 bits per pixel.
const COLMOD_16: u8 = 0x05;

/// Command, parameters.
const INIT: &[(u8, &[u8])] = &[
    (0xEF, &[]),
    (0xEB, &[0x14]),
    (0xFE, &[]),
    (0xEF, &[]),
    (0xEB, &[0x14]),
    (0x84, &[0x40]),
    (0x85, &[0xFF]),
    (0x86, &[0xFF]),
    (0x87, &[0xFF]),
    (0x88, &[0x0A]),
    (0x89, &[0x21]),
    (0x8A, &[0x00]),
    (0x8B, &[0x80]),
    (0x8C, &[0x01]),
    (0x8D, &[0x01]),
    (0x8E, &[0xFF]),
    (0x8F, &[0xFF]),
    (0xB6, &[0x00, 0x20]),
    (0x36, &[MADCTL]),
    (0x3A, &[COLMOD_16]),
    (0x90, &[0x08, 0x08, 0x08, 0x08]),
    (0xBD, &[0x06]),
    (0xBC, &[0x00]),
    (0xFF, &[0x60, 0x01, 0x04]),
    (0xC3, &[0x13]),
    (0xC4, &[0x13]),
    (0xC9, &[0x22]),
    (0xBE, &[0x11]),
    (0xE1, &[0x10, 0x0E]),
    (0xDF, &[0x21, 0x0C, 0x02]),
    (0xF0, &[0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),
    (0xF1, &[0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),
    (0xF2, &[0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),
    (0xF3, &[0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),
    (0xED, &[0x1B, 0x0B]),
    (0xAE, &[0x77]),
    (0xCD, &[0x63]),
    (
        0x70,
        &[0x07, 0x07, 0x04, 0x0E, 0x0F, 0x09, 0x07, 0x08, 0x03],
    ),
    (0xE8, &[0x34]),
    (
        0x62,
        &[
            0x18, 0x0D, 0x71, 0xED, 0x70, 0x70, 0x18, 0x0F, 0x71, 0xEF, 0x70, 0x70,
        ],
    ),
    (
        0x63,
        &[
            0x18, 0x11, 0x71, 0xF1, 0x70, 0x70, 0x18, 0x13, 0x71, 0xF3, 0x70, 0x70,
        ],
    ),
    (0x64, &[0x28, 0x29, 0xF1, 0x01, 0xF1, 0x00, 0x07]),
    (
        0x66,
        &[0x3C, 0x00, 0xCD, 0x67, 0x45, 0x45, 0x10, 0x00, 0x00, 0x00],
    ),
    (
        0x67,
        &[0x00, 0x3C, 0x00, 0x00, 0x00, 0x01, 0x54, 0x10, 0x32, 0x98],
    ),
    (0x74, &[0x10, 0x85, 0x80, 0x00, 0x00, 0x4E, 0x00]),
    (0x98, &[0x3E, 0x07]),
    (INVON, &[]),
];

pub struct Panel<'d> {
    spi: Spi<'d, SPI1, Async>,
    cs: Output<'d>,
    dc: Output<'d>,
    /// Held high for as long as the panel is used: dropping the pin would
    /// release it to the pad default (pull-down) and reset the panel.
    rst: Output<'d>,
}

impl<'d> Panel<'d> {
    /// Hardware reset and the init sequence; the frame memory is left as
    /// it is (send a frame before turning the backlight on).
    pub async fn new(
        spi: Spi<'d, SPI1, Async>,
        cs: Output<'d>,
        dc: Output<'d>,
        rst: Output<'d>,
    ) -> Self {
        let mut p = Panel { spi, cs, dc, rst };
        p.cs.set_high();
        p.rst.set_high();
        Timer::after_millis(10).await;
        p.rst.set_low();
        Timer::after_millis(10).await;
        p.rst.set_high();
        Timer::after_millis(120).await;
        for &(cmd, args) in INIT {
            p.command(cmd, args).await;
        }
        p.command(SLPOUT, &[]).await;
        Timer::after_millis(120).await;
        p.command(DISPON, &[]).await;
        Timer::after_millis(20).await;
        p
    }

    /// One command with its parameters under a single CS frame.
    async fn command(&mut self, cmd: u8, args: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.write(&[cmd]).await;
        self.dc.set_high();
        if !args.is_empty() {
            let _ = self.spi.write(args).await;
        }
        self.cs.set_high();
    }

    /// Sends a whole frame: window 0..239 x 0..239, RAMWR, then the pixels
    /// in one DMA transfer with CS low throughout.
    pub async fn flush(&mut self, frame: &Frame) {
        let last = (SIZE - 1) as u8;
        self.command(CASET, &[0, 0, 0, last]).await;
        self.command(RASET, &[0, 0, 0, last]).await;
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.write(&[RAMWR]).await;
        self.dc.set_high();
        let _ = self.spi.write(&frame.0[..]).await;
        self.cs.set_high();
    }
}

/// The picture in RAM: RGB565, high byte first, row by row.
pub struct Frame(pub [u8; FRAME_BYTES]);

impl Frame {
    fn put(&mut self, p: Point, c: Rgb565) {
        let i = (p.y as usize * SIZE as usize + p.x as usize) * 2;
        self.0[i..i + 2].copy_from_slice(&RawU16::from(c).into_inner().to_be_bytes());
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(SIZE, SIZE)
    }
}

impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let screen = self.bounding_box();
        for Pixel(p, c) in pixels {
            if screen.contains(p) {
                self.put(p, c);
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let area = area.intersection(&self.bounding_box());
        let px = RawU16::from(color).into_inner().to_be_bytes();
        for y in area.rows() {
            let row = y as usize * SIZE as usize;
            for x in area.columns() {
                let i = (row + x as usize) * 2;
                self.0[i..i + 2].copy_from_slice(&px);
            }
        }
        Ok(())
    }
}
