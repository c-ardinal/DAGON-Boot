//! Board resources several parties need: a shared bus, the devices on it
//! and the bus port that reaches them, and the `BoardBringUp` port.
//!
//! A board package exports a bus its bring-up and recovery need and the
//! app needs for its own devices as a [`Shared`] static (`SYS_I2C`), and
//! each device of its definition's `[[device]]` table as a [`Device`].
//! Everyone takes the bus through [`Shared::with`], bounded and
//! synchronous; a device's registers are written through
//! [`Device::write`], which refuses a register the definition does not
//! list as writable.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_hal::i2c::{I2c, Operation};

/// A board resource shared, not owned: put in once by the board package,
/// then used by anyone through [`Shared::with`].
pub struct Shared<T>(Mutex<CriticalSectionRawMutex, RefCell<Option<T>>>);

impl<T> Shared<T> {
    pub const fn new() -> Self {
        Shared(Mutex::new(RefCell::new(None)))
    }

    /// Puts the resource in; returns the one it replaces.
    pub fn put(&self, resource: T) -> Option<T> {
        self.0.lock(|c| c.borrow_mut().replace(resource))
    }

    /// Runs `f` with the resource, inside a critical section: `None` while
    /// nothing is put in, or when `f` of an outer `with` holds it.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.0.lock(|c| {
            let mut slot = c.try_borrow_mut().ok()?;
            slot.as_mut().map(f)
        })
    }
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A device on a board bus, as the board definition's `[[device]]`
/// declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    /// 7-bit address.
    pub addr: u8,
    /// The registers anyone outside DAGON may write.
    pub writable: &'static [u8],
}

/// Why the bus port refused or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError<E> {
    /// The bus is not put in yet, or in use further up the call stack.
    Busy,
    /// The definition does not list the register as writable.
    NotWritable(u8),
    /// The driver's error.
    Bus(E),
}

impl Device {
    /// Writes `bytes` to register `reg` in one transfer (register
    /// address, then the bytes).
    pub fn write<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        bytes: &[u8],
    ) -> Result<(), BusError<B::Error>> {
        if !self.writable.contains(&reg) {
            return Err(BusError::NotWritable(reg));
        }
        bus.with(|b| {
            b.transaction(
                self.addr,
                &mut [Operation::Write(&[reg]), Operation::Write(bytes)],
            )
        })
        .ok_or(BusError::Busy)?
        .map_err(BusError::Bus)
    }

    /// Reads `buf.len()` bytes from register `reg`.
    pub fn read<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        buf: &mut [u8],
    ) -> Result<(), BusError<B::Error>> {
        bus.with(|b| b.write_read(self.addr, &[reg], buf))
            .ok_or(BusError::Busy)?
            .map_err(BusError::Bus)
    }
}

/// The `BoardBringUp` port: the board's power, pins and buses at app
/// start, before DAGON serves its link. The board package provides it; a
/// user's own step is an extension later in the chain on `app.bring_up`.
pub trait BoardBringUp {
    fn bring_up(&mut self);
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use embedded_hal::i2c::{ErrorKind, ErrorType};

    use super::*;

    /// Records each transfer's address and written bytes; reads return
    /// the address.
    #[derive(Default)]
    struct Log(Vec<(u8, Vec<u8>)>);

    impl ErrorType for Log {
        type Error = ErrorKind;
    }

    impl I2c for Log {
        fn transaction(&mut self, addr: u8, ops: &mut [Operation<'_>]) -> Result<(), Self::Error> {
            let mut sent = Vec::new();
            for op in ops {
                match op {
                    Operation::Write(b) => sent.extend_from_slice(b),
                    Operation::Read(b) => b.fill(addr),
                }
            }
            self.0.push((addr, sent));
            Ok(())
        }
    }

    const IOE: Device = Device {
        addr: 0x4F,
        writable: &[0x05],
    };

    #[test]
    fn the_bus_port_writes_only_writable_registers() {
        let bus = Shared::<Log>::new();
        assert_eq!(IOE.write(&bus, 0x05, &[1]), Err(BusError::Busy));
        bus.put(Log::default());
        IOE.write(&bus, 0x05, &[1, 2]).unwrap();
        assert_eq!(
            IOE.write(&bus, 0x03, &[1]),
            Err(BusError::NotWritable(0x03))
        );
        let mut b = [0u8; 2];
        IOE.read(&bus, 0x03, &mut b).unwrap();
        assert_eq!(b, [0x4F; 2]);
        let log = bus.with(|l| core::mem::take(&mut l.0)).unwrap();
        assert_eq!(
            log,
            [(0x4F, std::vec![0x05, 1, 2]), (0x4F, std::vec![0x03])]
        );
    }

    #[test]
    fn the_bring_up_trait_is_the_catalogs_port() {
        let p = crate::Port::find("BoardBringUp").unwrap();
        assert!(p.in_layer(crate::Layer::App) && p.in_layer(crate::Layer::Relay));
    }

    #[test]
    fn a_nested_use_is_busy_not_a_panic() {
        let bus = Shared::<Log>::new();
        bus.put(Log::default());
        let inner = bus.with(|_| IOE.write(&bus, 0x05, &[0])).unwrap();
        assert_eq!(inner, Err(BusError::Busy));
    }
}
