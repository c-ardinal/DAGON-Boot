//! One flash driver, several handles: [`SharedFlash`]. The slot store holds
//! one handle, the MCU package keeps another for its own reads (a topology
//! copy, a conformance build's scratch). A handle takes the driver for one
//! operation only and never masks interrupts to do it: the lock is a
//! `try_lock` (a few instructions in a critical section to take it), and a
//! handle that finds the driver taken answers [`FlashError::Hw`] instead of
//! waiting. Both handles of a package are used from the same task, so that
//! answer means a handle was moved to another task: a programming error the
//! caller reports, never a stall or a masked flash operation.

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

/// The flash driver behind [`SharedFlash`] handles.
pub type FlashDriver<F> = Mutex<CriticalSectionRawMutex, F>;

/// `flash` as a driver handles can share (keep it in a `static`).
pub const fn driver<F>(flash: F) -> FlashDriver<F> {
    Mutex::new(flash)
}

/// A handle on a shared flash driver; copies are further handles.
pub struct SharedFlash<F: 'static> {
    driver: &'static FlashDriver<F>,
    regions: &'static [FlashRegion],
}

impl<F> Clone for SharedFlash<F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for SharedFlash<F> {}

impl<F: Flash> SharedFlash<F> {
    /// A handle on `driver`, whose geometry is `regions` (the driver's own,
    /// as a static the handle can lend out).
    pub const fn new(driver: &'static FlashDriver<F>, regions: &'static [FlashRegion]) -> Self {
        SharedFlash { driver, regions }
    }

    fn with<T>(&self, op: impl FnOnce(&mut F) -> Result<T, FlashError>) -> Result<T, FlashError> {
        let mut driver = self.driver.try_lock().map_err(|_| FlashError::Hw)?;
        op(&mut driver)
    }
}

impl<F: Flash> Flash for SharedFlash<F> {
    fn geometry(&self) -> &[FlashRegion] {
        self.regions
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.with(|f| f.read(addr, buf))
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.with(|f| f.erase(addr, len))
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.with(|f| f.write(addr, data))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    use super::*;

    const REGIONS: [FlashRegion; 1] = [FlashRegion {
        start: 0,
        len: 64,
        erase_size: 16,
        write_size: 1,
        erased_value: 0xFF,
    }];

    struct Ram(Vec<u8>);

    impl Flash for Ram {
        fn geometry(&self) -> &[FlashRegion] {
            &REGIONS
        }
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            buf.copy_from_slice(&self.0[addr as usize..][..buf.len()]);
            Ok(())
        }
        fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
            self.0[addr as usize..][..len as usize].fill(0xFF);
            Ok(())
        }
        fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
            self.0[addr as usize..][..data.len()].copy_from_slice(data);
            Ok(())
        }
    }

    fn driver() -> &'static FlashDriver<Ram> {
        Box::leak(Box::new(Mutex::new(Ram(vec![0; 64]))))
    }

    #[test]
    fn handles_share_one_driver() {
        let d = driver();
        let (mut a, mut b) = (SharedFlash::new(d, &REGIONS), SharedFlash::new(d, &REGIONS));
        a.erase(16, 16).unwrap();
        a.write(16, &[1, 2, 3]).unwrap();
        let mut back = [0u8; 4];
        b.read(16, &mut back).unwrap();
        assert_eq!(back, [1, 2, 3, 0xFF]);
        assert_eq!(b.geometry(), &REGIONS);
    }

    /// A handle never waits for the driver: while it is taken, every
    /// operation answers at once and leaves the flash as it was.
    #[test]
    fn a_taken_driver_is_refused_without_waiting() {
        let d = driver();
        let mut h = SharedFlash::new(d, &REGIONS);
        let held = d.try_lock().unwrap();
        let mut back = [0u8; 1];
        assert_eq!(h.read(0, &mut back), Err(FlashError::Hw));
        assert_eq!(h.erase(0, 16), Err(FlashError::Hw));
        assert_eq!(h.write(0, &[9]), Err(FlashError::Hw));
        drop(held);
        h.read(0, &mut back).unwrap();
        assert_eq!(back, [0]);
    }
}
