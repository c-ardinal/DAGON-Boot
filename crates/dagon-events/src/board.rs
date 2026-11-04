//! Board resources several parties need: a shared bus, the devices on it
//! and the bus port that reaches them, and the `BoardBringUp` port.
//!
//! A board package exports a bus its bring-up and recovery need and the
//! app needs for its own devices as a [`Shared`] static (`SYS_I2C`), and
//! each device of its definition's `[[device]]` table as a [`Device`]
//! generated from that table. Everyone takes the bus through
//! [`Shared::with`]: a lease, not a critical section, so a driver that
//! waits for its interrupts works inside it. A device's registers are
//! reached through [`Device::write`] and [`Device::read`], at most
//! [`BUS_MAX`] bytes a transfer; `write` refuses a register the definition
//! does not list as writable.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_hal::i2c::{I2c, Operation};

/// Most bytes one bus port transfer moves, either way.
pub const BUS_MAX: usize = 32;

/// A board resource shared, not owned: put in once by the board package,
/// then used by anyone through [`Shared::with`].
pub struct Shared<T>(Mutex<CriticalSectionRawMutex, Option<T>>);

impl<T> Shared<T> {
    pub const fn new() -> Self {
        Shared(Mutex::new(None))
    }

    /// Puts the resource in and returns the one it replaces; gives
    /// `resource` back while someone holds the lease.
    pub fn put(&self, resource: T) -> Result<Option<T>, T> {
        match self.0.try_lock() {
            Ok(mut slot) => Ok(slot.replace(resource)),
            Err(_) => Err(resource),
        }
    }

    /// Runs `f` with the resource under a lease: interrupts stay enabled,
    /// so `f` may wait for them. `None` while nothing is put in, or while
    /// another holder (an outer `with`, another task) has the lease.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut slot = self.0.try_lock().ok()?;
        slot.as_mut().map(f)
    }
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A device on a board bus, as the board definition's `[[device]]`
/// declares it. Its fields are private: a `Device` comes only from the
/// constants a board package generates from its definition.
///
/// ```compile_fail,E0451
/// let forged = dagon_events::Device { addr: 0x6E, writable: &[0x0C] };
/// ```
///
/// ```compile_fail,E0133
/// let forged = dagon_events::Device::from_definition(0x6E, &[0x0C]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    addr: u8,
    writable: &'static [u8],
}

/// Why the bus port refused or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError<E> {
    /// The bus is not put in yet, or another holder has its lease.
    Busy,
    /// The definition does not list the register as writable.
    NotWritable(u8),
    /// More than [`BUS_MAX`] bytes.
    TooLong(usize),
    /// The driver's error.
    Bus(E),
}

impl Device {
    /// The device of a validated board definition's `[[device]]`.
    ///
    /// # Safety
    ///
    /// Only the definition code generator calls this, with the address and
    /// writable registers of a `[[device]]` the definition loader
    /// accepted: the writable list is what the port enforces, so any other
    /// caller could widen it.
    #[doc(hidden)]
    #[allow(unsafe_code)]
    pub const unsafe fn from_definition(addr: u8, writable: &'static [u8]) -> Self {
        assert!(addr >= 0x08 && addr <= 0x77, "a 7-bit I2C device address");
        Device { addr, writable }
    }

    /// 7-bit address.
    pub const fn addr(&self) -> u8 {
        self.addr
    }

    /// The registers anyone outside DAGON may write.
    pub const fn writable(&self) -> &'static [u8] {
        self.writable
    }

    /// Writes `bytes` (at most [`BUS_MAX`]) to register `reg` in one
    /// transfer (register address, then the bytes).
    pub fn write<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        bytes: &[u8],
    ) -> Result<(), BusError<B::Error>> {
        if !self.writable.contains(&reg) {
            return Err(BusError::NotWritable(reg));
        }
        if bytes.len() > BUS_MAX {
            return Err(BusError::TooLong(bytes.len()));
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

    /// Reads `buf.len()` bytes (at most [`BUS_MAX`]) from register `reg`.
    pub fn read<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        buf: &mut [u8],
    ) -> Result<(), BusError<B::Error>> {
        if buf.len() > BUS_MAX {
            return Err(BusError::TooLong(buf.len()));
        }
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

    // SAFETY: a test device standing in for a generated one.
    #[allow(unsafe_code)]
    const IOE: Device = unsafe { Device::from_definition(0x4F, &[0x05]) };

    #[test]
    fn the_bus_port_writes_only_writable_registers() {
        let bus = Shared::<Log>::new();
        assert_eq!(IOE.write(&bus, 0x05, &[1]), Err(BusError::Busy));
        assert!(matches!(bus.put(Log::default()), Ok(None)));
        IOE.write(&bus, 0x05, &[1, 2]).unwrap();
        assert_eq!(
            IOE.write(&bus, 0x03, &[1]),
            Err(BusError::NotWritable(0x03))
        );
        let mut b = [0u8; 2];
        IOE.read(&bus, 0x03, &mut b).unwrap();
        assert_eq!(b, [0x4F; 2]);
        assert_eq!(
            IOE.write(&bus, 0x05, &[0; BUS_MAX + 1]),
            Err(BusError::TooLong(BUS_MAX + 1))
        );
        assert_eq!(
            IOE.read(&bus, 0x03, &mut [0; BUS_MAX + 1]),
            Err(BusError::TooLong(BUS_MAX + 1))
        );
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
        assert!(bus.put(Log::default()).is_ok());
        let inner = bus.with(|_| IOE.write(&bus, 0x05, &[0])).unwrap();
        assert_eq!(inner, Err(BusError::Busy));
        // Nor is the resource swapped while it is leased.
        assert_eq!(bus.with(|_| bus.put(Log::default()).is_err()), Some(true));
    }
}
