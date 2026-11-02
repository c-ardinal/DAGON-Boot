//! GC9A01 240x240 round panel over 4-wire SPI (mode 0): DC low for the
//! command byte, high for its parameters and pixel data, CS held low for a
//! whole command or a whole pixel stream. Pixels are RGB565, big-endian on
//! the wire. The init sequence is the panel vendor's (manufacturer
//! registers, power, gamma), then inversion on, sleep out, display on.

use core::convert::Infallible;

use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Blocking, Spi};
use embassy_time::{block_for, Duration};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

pub const SIZE: u32 = 240;

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

/// Bytes per SPI burst of a solid fill.
const FILL_CHUNK: usize = 64;

pub struct Panel<'d> {
    spi: Spi<'d, SPI1, Blocking>,
    cs: Output<'d>,
    dc: Output<'d>,
    /// Held high for as long as the panel is used: dropping the pin would
    /// release it to the pad default (pull-down) and reset the panel.
    rst: Output<'d>,
}

impl<'d> Panel<'d> {
    /// Hardware reset, init sequence, then the whole frame cleared to black.
    pub fn new(
        spi: Spi<'d, SPI1, Blocking>,
        cs: Output<'d>,
        dc: Output<'d>,
        rst: Output<'d>,
    ) -> Self {
        let mut p = Panel { spi, cs, dc, rst };
        p.cs.set_high();
        p.rst.set_high();
        block_for(Duration::from_millis(10));
        p.rst.set_low();
        block_for(Duration::from_millis(10));
        p.rst.set_high();
        block_for(Duration::from_millis(120));
        for &(cmd, args) in INIT {
            p.command(cmd, args);
        }
        p.command(SLPOUT, &[]);
        block_for(Duration::from_millis(120));
        p.command(DISPON, &[]);
        block_for(Duration::from_millis(20));
        let _ = p.clear(Rgb565::BLACK);
        p
    }

    /// One command with its parameters under a single CS frame.
    fn command(&mut self, cmd: u8, args: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.blocking_write(&[cmd]);
        self.dc.set_high();
        if !args.is_empty() {
            let _ = self.spi.blocking_write(args);
        }
        self.cs.set_high();
    }

    /// Sets the window and starts RAMWR; leaves CS low and DC high for the
    /// pixel stream that follows. Call [`Panel::end`] after it.
    fn begin(&mut self, area: &Rectangle) {
        let (x0, y0) = (area.top_left.x as u16, area.top_left.y as u16);
        let (x1, y1) = (
            x0 + area.size.width as u16 - 1,
            y0 + area.size.height as u16 - 1,
        );
        let [a, b] = x0.to_be_bytes();
        let [c, d] = x1.to_be_bytes();
        self.command(CASET, &[a, b, c, d]);
        let [a, b] = y0.to_be_bytes();
        let [c, d] = y1.to_be_bytes();
        self.command(RASET, &[a, b, c, d]);
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.blocking_write(&[RAMWR]);
        self.dc.set_high();
    }

    fn end(&mut self) {
        self.cs.set_high();
    }
}

fn be(c: Rgb565) -> [u8; 2] {
    RawU16::from(c).into_inner().to_be_bytes()
}

impl OriginDimensions for Panel<'_> {
    fn size(&self) -> Size {
        Size::new(SIZE, SIZE)
    }
}

impl DrawTarget for Panel<'_> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let screen = self.bounding_box();
        for Pixel(p, c) in pixels {
            if screen.contains(p) {
                self.begin(&Rectangle::new(p, Size::new(1, 1)));
                let _ = self.spi.blocking_write(&be(c));
                self.end();
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let clipped = area.intersection(&self.bounding_box());
        if clipped.size != area.size {
            // Partly off screen: per pixel, clipped.
            let pixels = area.points().zip(colors).map(|(p, c)| Pixel(p, c));
            return self.draw_iter(pixels);
        }
        if area.is_zero_sized() {
            return Ok(());
        }
        self.begin(area);
        let mut buf = [0u8; FILL_CHUNK];
        let mut n = 0;
        for c in colors
            .into_iter()
            .take(area.size.width as usize * area.size.height as usize)
        {
            buf[n..n + 2].copy_from_slice(&be(c));
            n += 2;
            if n == FILL_CHUNK {
                let _ = self.spi.blocking_write(&buf);
                n = 0;
            }
        }
        if n > 0 {
            let _ = self.spi.blocking_write(&buf[..n]);
        }
        self.end();
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let area = area.intersection(&self.bounding_box());
        if area.is_zero_sized() {
            return Ok(());
        }
        let mut buf = [0u8; FILL_CHUNK];
        for px in buf.chunks_exact_mut(2) {
            px.copy_from_slice(&be(color));
        }
        let mut left = area.size.width as usize * area.size.height as usize * 2;
        self.begin(&area);
        while left > 0 {
            let n = left.min(FILL_CHUNK);
            let _ = self.spi.blocking_write(&buf[..n]);
            left -= n;
        }
        self.end();
        Ok(())
    }
}
