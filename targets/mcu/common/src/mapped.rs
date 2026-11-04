//! Read-only `Flash` over memory mapped at a fixed address: the XIP window
//! on the board, a byte buffer standing in for it on the host, so stage0's
//! reader and the host check run the same address translation.

use dagon_port::{Flash, FlashError, FlashRegion};

pub struct Mapped {
    base: *const u8,
    /// Address `base` corresponds to.
    start: u32,
    len: usize,
}

// SAFETY: a `Mapped` only ever reads, and its constructor's contract keeps
// the range readable while it lives, from any context.
unsafe impl Send for Mapped {}
// SAFETY: as for `Send`; `&Mapped` offers no access at all.
unsafe impl Sync for Mapped {}

impl Mapped {
    /// # Safety
    /// `base..base + len` must stay readable while this lives.
    pub const unsafe fn new(base: *const u8, start: u32, len: usize) -> Self {
        Mapped { base, start, len }
    }

    /// Host stand-in: `mem` holds the bytes of addresses `start..`.
    pub fn over(mem: &'static [u8], start: u32) -> Self {
        // SAFETY: a 'static slice stays readable.
        unsafe { Mapped::new(mem.as_ptr(), start, mem.len()) }
    }
}

impl Flash for Mapped {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let off = addr.checked_sub(self.start).ok_or(FlashError::Range)? as usize;
        if off.checked_add(buf.len()).is_none_or(|end| end > self.len) {
            return Err(FlashError::Range);
        }
        // SAFETY: `off..off + len` lies inside the mapped range (checked).
        let src = unsafe { core::slice::from_raw_parts(self.base.add(off), buf.len()) };
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

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    /// An XIP-like window: flash mapped at 0x1000_0000.
    const XIP_BASE: u32 = 0x1000_0000;
    const BL_LOG: [u32; 1] = [XIP_BASE + 0x4000];
    use std::boxed::Box;
    use std::vec;

    #[test]
    fn reads_xip_addresses_from_the_mapped_window() {
        let mut mem = vec![0xFFu8; 0x9000];
        mem[0x4000..0x4004].copy_from_slice(b"DGSL");
        let mut f = Mapped::over(Box::leak(mem.into_boxed_slice()), XIP_BASE);
        let mut b = [0u8; 4];
        f.read(BL_LOG[0], &mut b).unwrap();
        assert_eq!(&b, b"DGSL");
        assert_eq!(f.read(0x4000, &mut b), Err(FlashError::Range));
        assert_eq!(f.read(XIP_BASE + 0x8FFE, &mut b), Err(FlashError::Range));
    }
}
