//! `Flash` over the regions in [`crate::layout::REGIONS`], backed by esp-storage.

use core::cell::RefCell;

use dagon_port::{Flash, FlashError, FlashRegion};
use esp_bootloader_esp_idf::partitions::FlashStorage;

use crate::layout::{region_for, ERASE_UNIT, REGIONS, WRITE_UNIT};

/// Chunk used to check erased state and read back writes.
const CHECK_CHUNK: usize = 256;

/// Shares the one `FlashStorage` with [`crate::boot::EspBootSelect`].
pub struct EspFlash<'a, 'd> {
    storage: &'a RefCell<FlashStorage<'d>>,
}

impl<'a, 'd> EspFlash<'a, 'd> {
    pub fn new(storage: &'a RefCell<FlashStorage<'d>>) -> Self {
        Self { storage }
    }

    /// Calls `f` on each chunk of `addr..addr + len` as read from flash.
    fn for_each_chunk(
        &mut self,
        addr: u32,
        len: usize,
        mut f: impl FnMut(usize, &[u8]) -> bool,
    ) -> Result<(), FlashError> {
        let mut storage = self.storage.borrow_mut();
        let mut chunk = [0u8; CHECK_CHUNK];
        let mut done = 0;
        while done < len {
            let n = (len - done).min(CHECK_CHUNK);
            storage
                .read(addr + done as u32, &mut chunk[..n])
                .map_err(|_| FlashError::Hw)?;
            if !f(done, &chunk[..n]) {
                return Err(FlashError::Hw);
            }
            done += n;
        }
        Ok(())
    }
}

fn check(addr: u32, len: usize) -> Result<&'static FlashRegion, FlashError> {
    let len = u32::try_from(len).map_err(|_| FlashError::Range)?;
    region_for(addr, len).ok_or(FlashError::Range)
}

impl Flash for EspFlash<'_, '_> {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        check(addr, buf.len())?;
        self.storage
            .borrow_mut()
            .read(addr, buf)
            .map_err(|_| FlashError::Hw)
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        check(addr, len as usize)?;
        if !addr.is_multiple_of(ERASE_UNIT) || !len.is_multiple_of(ERASE_UNIT) {
            return Err(FlashError::Align);
        }
        self.storage
            .borrow_mut()
            .erase(addr, addr + len)
            .map_err(|_| FlashError::Hw)
    }

    /// Refuses (`Hw`) a write into bytes that do not read as erased, and
    /// reads every write back.
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        check(addr, data.len())?;
        if !addr.is_multiple_of(WRITE_UNIT) || !data.len().is_multiple_of(WRITE_UNIT as usize) {
            return Err(FlashError::Align);
        }
        self.for_each_chunk(addr, data.len(), |_, c| c.iter().all(|&b| b == 0xFF))?;
        self.storage
            .borrow_mut()
            .write_nor(addr, data)
            .map_err(|_| FlashError::Hw)?;
        self.for_each_chunk(addr, data.len(), |at, c| c == &data[at..at + c.len()])
    }
}
