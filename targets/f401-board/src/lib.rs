//! NUCLEO-F401RE board support shared by the boot core and the apps: the
//! flash layout and the internal flash behind the `dagon_port::Flash` port.
#![no_std]
#![forbid(unsafe_code)]

mod layout;

pub use layout::*;

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_stm32::flash::{Blocking, Flash as Hal, FLASH_BASE, FLASH_SIZE};
use embassy_stm32::peripherals::FLASH;
use embassy_stm32::Peri;

/// Regions DAGON may erase and write: the two log faces and the two APP
/// slots. Reads are allowed anywhere in flash; the boot core sectors are
/// never erased or written through this port.
const WRITABLE: [FlashRegion; 2] = [
    FlashRegion {
        start: 0x0800_8000,
        len: 0x8000,
        erase_size: 0x4000,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: 0x0802_0000,
        len: 0x4_0000,
        erase_size: 0x2_0000,
        write_size: 4,
        erased_value: 0xFF,
    },
];

pub struct BoardFlash<'d>(Hal<'d, Blocking>);

impl<'d> BoardFlash<'d> {
    pub fn new(p: Peri<'d, FLASH>) -> Self {
        BoardFlash(Hal::new_blocking(p))
    }
}

fn region(addr: u32, len: u32) -> Result<&'static FlashRegion, FlashError> {
    WRITABLE
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

impl Flash for BoardFlash<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        &WRITABLE
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
