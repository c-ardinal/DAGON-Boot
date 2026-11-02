//! `dagon_port::Flash` over the embassy-rp blocking flash driver, addressed
//! by XIP address.

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_rp::flash::{Blocking, Flash as RpDriver, ERASE_SIZE};
use embassy_rp::peripherals::FLASH;
use embassy_rp::Peri;

use crate::layout::{MIN_FLASH_SIZE, XIP_BASE};

/// The part of the flash the DAGON layout spans (the first
/// [`MIN_FLASH_SIZE`] bytes; a larger chip's rest stays the app's own).
pub struct RpFlash(RpDriver<'static, FLASH, Blocking, MIN_FLASH_SIZE>);

const REGIONS: [FlashRegion; 1] = [FlashRegion {
    start: XIP_BASE,
    len: MIN_FLASH_SIZE as u32,
    erase_size: ERASE_SIZE as u32,
    write_size: 1,
    erased_value: 0xFF,
}];

impl RpFlash {
    pub fn new(flash: Peri<'static, FLASH>) -> Self {
        RpFlash(RpDriver::new_blocking(flash))
    }

    /// 64-bit unique ID of the flash chip (per-board serial number).
    pub fn unique_id(&mut self) -> [u8; 8] {
        let mut id = [0u8; 8];
        let _ = self.0.blocking_unique_id(&mut id);
        id
    }
}

/// Flash offset of XIP address `addr`.
fn off(addr: u32) -> Result<u32, FlashError> {
    addr.checked_sub(XIP_BASE).ok_or(FlashError::Range)
}

fn hw(e: embassy_rp::flash::Error) -> FlashError {
    match e {
        embassy_rp::flash::Error::OutOfBounds => FlashError::Range,
        embassy_rp::flash::Error::Unaligned => FlashError::Align,
        _ => FlashError::Hw,
    }
}

impl Flash for RpFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.0.blocking_read(off(addr)?, buf).map_err(hw)
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let o = off(addr)?;
        self.0.blocking_erase(o, o + len).map_err(hw)
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.0.blocking_write(off(addr)?, data).map_err(hw)
    }
}
