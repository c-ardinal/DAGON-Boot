//! The whole picture in RAM, as the app sends it to the panel in one
//! transfer: RGB565, high byte first, row by row.

use core::convert::Infallible;

use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use crate::gc9a01::SIZE;

/// Bytes of one full frame.
pub const FRAME_BYTES: usize = SIZE as usize * SIZE as usize * 2;

pub struct Frame(pub [u8; FRAME_BYTES]);

impl Frame {
    pub const fn new() -> Self {
        Frame([0; FRAME_BYTES])
    }

    fn put(&mut self, p: Point, c: Rgb565) {
        let i = (p.y as usize * SIZE as usize + p.x as usize) * 2;
        self.0[i..i + 2].copy_from_slice(&RawU16::from(c).into_inner().to_be_bytes());
    }

    /// The colour of pixel `p` (inside the frame).
    pub fn at(&self, p: Point) -> Rgb565 {
        let i = (p.y as usize * SIZE as usize + p.x as usize) * 2;
        RawU16::new(u16::from_be_bytes([self.0[i], self.0[i + 1]])).into()
    }
}

impl Default for Frame {
    fn default() -> Self {
        Frame::new()
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(u32::from(SIZE), u32::from(SIZE))
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
