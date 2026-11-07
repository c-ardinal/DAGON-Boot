//! The internal flash behind the `dagon_port::Flash` port
//! (erase and write only inside [`FLASH_REGIONS`]).

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_stm32::flash::{Blocking, Flash as Hal, FLASH_BASE, FLASH_SIZE};
use embassy_stm32::peripherals::FLASH;
use embassy_stm32::Peri;

use crate::layout::FLASH_REGIONS;

pub struct F401Flash<'d>(Hal<'d, Blocking>);

impl<'d> F401Flash<'d> {
    pub fn new(p: Peri<'d, FLASH>) -> Self {
        F401Flash(Hal::new_blocking(p))
    }
}

fn region(addr: u32, len: u32) -> Result<&'static FlashRegion, FlashError> {
    FLASH_REGIONS
        .iter()
        .find(|r| addr >= r.start && addr.saturating_add(len) <= r.start + r.len)
        .ok_or(FlashError::Range)
}

fn offset(addr: u32, len: usize) -> Result<u32, FlashError> {
    let off = addr
        .checked_sub(FLASH_BASE as u32)
        .ok_or(FlashError::Range)?;
    (off as usize + len <= FLASH_SIZE)
        .then_some(off)
        .ok_or(FlashError::Range)
}

impl Flash for F401Flash<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        &FLASH_REGIONS
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let off = offset(addr, buf.len())?;
        self.0.blocking_read(off, buf).map_err(|_| FlashError::Hw)
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let r = region(addr, len)?;
        if !(addr - r.start).is_multiple_of(r.erase_size) || !len.is_multiple_of(r.erase_size) {
            return Err(FlashError::Align);
        }
        let off = offset(addr, len as usize)?;
        self.0
            .blocking_erase(off, off + len)
            .map_err(|_| FlashError::Hw)
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let r = region(addr, data.len() as u32)?;
        if !addr.is_multiple_of(r.write_size) || !(data.len() as u32).is_multiple_of(r.write_size) {
            return Err(FlashError::Align);
        }
        let off = offset(addr, data.len())?;
        self.0.blocking_write(off, data).map_err(|_| FlashError::Hw)
    }
}
