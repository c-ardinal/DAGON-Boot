//! `dagon_port::Flash` over the embassy-rp blocking flash driver, addressed
//! by XIP address.

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_rp::flash::{Blocking, Flash as RpDriver, ERASE_SIZE, WRITE_SIZE};
use embassy_rp::peripherals::FLASH;
use embassy_rp::Peri;

use crate::layout::{MIN_FLASH_SIZE, SECTOR, XIP_BASE};

// The driver erases whole `ERASE_SIZE` sectors and writes whole
// `WRITE_SIZE` units.
const _: () = assert!(
    (SECTOR as usize).is_multiple_of(ERASE_SIZE)
        && (crate::WRITE_UNIT as usize).is_multiple_of(WRITE_SIZE),
    "[execution] erase or write unit is not a multiple of the flash driver's"
);

/// Bytes of the flash chip's unique ID (64 bits).
const UNIQUE_ID_LEN: usize = (u64::BITS / u8::BITS) as usize;

/// The part of the flash the DAGON layout spans (the first
/// [`MIN_FLASH_SIZE`] bytes; a larger chip's rest stays the app's own).
pub struct RpFlash(RpDriver<'static, FLASH, Blocking, MIN_FLASH_SIZE>);

pub const REGIONS: [FlashRegion; 1] = [FlashRegion {
    start: XIP_BASE,
    len: MIN_FLASH_SIZE as u32,
    erase_size: SECTOR,
    write_size: crate::WRITE_UNIT,
    erased_value: crate::ERASED_VALUE,
}];

impl RpFlash {
    pub fn new(flash: Peri<'static, FLASH>) -> Self {
        RpFlash(RpDriver::new_blocking(flash))
    }

    /// 64-bit unique ID of the flash chip (per-board serial number).
    pub fn unique_id(&mut self) -> [u8; UNIQUE_ID_LEN] {
        let mut id = [0u8; UNIQUE_ID_LEN];
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

/// A handle on the one flash driver, held by the slot store and used by the
/// serving task only. Flash operations run from RAM with XIP
/// off and interrupts masked by the driver, one 4 KiB sector (or page) per
/// operation (`ERASE_REGIONS`, worst case `ERASE_UNIT_MAX_MS`); DAGON masks
/// nothing around them.
pub type SharedFlash = dagon_mcu_common::shared::SharedFlash<RpFlash>;
