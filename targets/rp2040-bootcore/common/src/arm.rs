//! Target-only pieces: read flash through XIP, and start an image.

use dagon_port::{Flash, FlashError, FlashRegion};

use crate::layout::{FLASH_SIZE, XIP_BASE};

/// Read-only flash through the XIP window (stage0 never writes).
pub struct XipFlash;

impl Flash for XipFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let end = addr as usize + buf.len();
        if end > FLASH_SIZE {
            return Err(FlashError::Range);
        }
        // SAFETY: the range lies inside the memory-mapped XIP window, which is
        // always readable and not written while stage0 runs.
        let src = unsafe { core::slice::from_raw_parts((XIP_BASE + addr) as *const u8, buf.len()) };
        buf.copy_from_slice(src);
        Ok(())
    }

    fn erase(&mut self, _addr: u32, _len: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }

    fn write(&mut self, _addr: u32, _data: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

/// Starts the image whose vector table is at flash offset `body`.
/// Interrupts must not be in use (NVIC enables and pendings are cleared).
pub fn jump(body: u32) -> ! {
    let vtor = XIP_BASE + body;
    // SAFETY: `body` holds a verified image linked for this address; the
    // caller has no live interrupt sources, so switching VTOR and the stack
    // pointer here hands the core over cleanly.
    unsafe {
        let nvic = &*cortex_m::peripheral::NVIC::PTR;
        nvic.icer[0].write(u32::MAX);
        nvic.icpr[0].write(u32::MAX);
        (*cortex_m::peripheral::SCB::PTR).vtor.write(vtor);
        cortex_m::asm::bootload(vtor as *const u32)
    }
}
