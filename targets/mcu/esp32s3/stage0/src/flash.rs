//! The flash through the ROM's SPI flash driver, as the ROM left it set up
//! (no mode or clock change), and the body hash on the SHA peripheral.

use crate::rom;
use dagon_port::{Flash, FlashError, FlashRegion};

/// The flash the ESP32-S3 definition gives (`[[flash_map]]` esp32s3-16m).
const BYTES: u32 = dagon_mcu_esp32s3::DEFAULT_FLASH.1;
const SECTOR: u32 = 0x1000;
/// The ROM's reads and writes: whole words from a word-aligned buffer.
const WORDS: usize = 64;

const REGIONS: [FlashRegion; 1] = [FlashRegion {
    start: 0,
    len: BYTES,
    erase_size: SECTOR,
    write_size: 4,
    erased_value: 0xFF,
}];

/// SHA-256 for `ets_sha_init` (ESP-IDF `rom/sha.h` `SHA_TYPE`, ESP32-S3).
const SHA2_256: u32 = 2;

/// Sets the ROM's driver up for the definition's flash and lets it write.
pub fn attach() {
    // SAFETY: the ROM's driver, called once before any flash access.
    unsafe {
        rom::esp_rom_spiflash_config_param(0, BYTES, 0x1_0000, SECTOR, 0x100, 0xFFFF);
        rom::spi_flash_attach(rom::ets_efuse_get_spiconfig(), false);
        rom::esp_rom_spiflash_unlock();
    }
}

/// The flash, read and written through the ROM.
pub struct RomFlash;

impl Flash for RomFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let end = addr
            .checked_add(buf.len() as u32)
            .ok_or(FlashError::Range)?;
        if end > BYTES {
            return Err(FlashError::Range);
        }
        let mut words = [0u32; WORDS];
        let mut done = 0usize;
        while done < buf.len() {
            let at = addr + done as u32;
            let base = at & !3;
            let skip = (at - base) as usize;
            let n = (buf.len() - done).min(WORDS * 4 - 4);
            let len = ((skip + n + 3) & !3) as u32;
            // SAFETY: `len` bytes fit `words` (at most WORDS * 4).
            if unsafe { rom::esp_rom_spiflash_read(base, words.as_mut_ptr(), len) } != 0 {
                return Err(FlashError::Hw);
            }
            let bytes = words.map(u32::to_le_bytes);
            let bytes = bytes.as_flattened();
            buf[done..done + n].copy_from_slice(&bytes[skip..skip + n]);
            done += n;
        }
        Ok(())
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        if !addr.is_multiple_of(SECTOR) || !len.is_multiple_of(SECTOR) {
            return Err(FlashError::Align);
        }
        let end = addr.checked_add(len).ok_or(FlashError::Range)?;
        if end > BYTES {
            return Err(FlashError::Range);
        }
        for s in (addr..end).step_by(SECTOR as usize) {
            // A slot's erase outlasts the trial period (a sector takes up to
            // 400 ms): the boot core, the only image that erases, feeds at
            // every sector.
            crate::wdt::feed();
            // SAFETY: a sector inside the flash.
            if unsafe { rom::esp_rom_spiflash_erase_sector(s / SECTOR) } != 0 {
                return Err(FlashError::Hw);
            }
        }
        Ok(())
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let end = addr
            .checked_add(data.len() as u32)
            .ok_or(FlashError::Range)?;
        if end > BYTES {
            return Err(FlashError::Range);
        }
        let mut words = [0u32; WORDS];
        for (i, chunk) in data.chunks(WORDS * 4).enumerate() {
            for (w, b) in words.iter_mut().zip(chunk.chunks_exact(4)) {
                *w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            let at = addr + (i * WORDS * 4) as u32;
            // SAFETY: `chunk.len()` bytes of `words`, a multiple of 4.
            if unsafe { rom::esp_rom_spiflash_write(at, words.as_ptr(), chunk.len() as u32) } != 0 {
                return Err(FlashError::Hw);
            }
        }
        Ok(())
    }

    /// The SHA peripheral through the ROM's `ets_sha_*` (a body of a few
    /// MiB takes about a twentieth of the software rounds' time).
    fn sha256(&mut self, addr: u32, len: u32) -> Result<[u8; 32], FlashError> {
        // The ROM's SHA_CTX, with room to spare.
        let mut ctx = [0u32; 64];
        let mut out = [0u8; 32];
        let mut buf = [0u8; 256];
        // SAFETY: `ctx` outlives every call that takes it.
        unsafe {
            rom::ets_sha_enable();
            rom::ets_sha_init(ctx.as_mut_ptr(), SHA2_256);
            rom::ets_sha_starts(ctx.as_mut_ptr(), 0);
        }
        let mut done = 0u32;
        let read = loop {
            if done == len {
                break Ok(());
            }
            let n = (len - done).min(buf.len() as u32);
            let at = addr.checked_add(done).ok_or(FlashError::Range);
            if let Err(e) = at.and_then(|at| self.read(at, &mut buf[..n as usize])) {
                break Err(e);
            }
            // SAFETY: `n` bytes of `buf`.
            unsafe { rom::ets_sha_update(ctx.as_mut_ptr(), buf.as_ptr(), n, false) };
            done += n;
        };
        // SAFETY: as above; the peripheral is released on every path.
        unsafe {
            rom::ets_sha_finish(ctx.as_mut_ptr(), out.as_mut_ptr());
            rom::ets_sha_disable();
        }
        read.map(|()| out)
    }
}
