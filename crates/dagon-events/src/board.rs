//! Board resources several parties need: a shared bus, the devices on it
//! and the bus port that reaches them, and the `BoardBringUp` port.
//!
//! A board package exports a bus its bring-up and recovery need and the
//! app needs for its own devices as a [`Shared`] static (`SYS_I2C`), and
//! each device of its definition's `[[device]]` table as a [`Device`]
//! generated from that table. Everyone takes the bus through
//! [`Shared::with`]: a lease, not a critical section, so a driver that
//! waits for its interrupts works inside it. A device's registers are
//! reached through [`Device::write`] and [`Device::read`], each transfer
//! bounded by the device's own maximum (its datasheet's, declared in the
//! definition); `write` refuses a register the definition does not list
//! as writable.
//!
//! A device the definition gives a `probe` may not be fitted (a lot, a
//! unit or a revision carries another part, or none): the package has it
//! as a [`Probed`], and its [`Device`] comes only from [`Probed::find`],
//! once the part answered its probe on the bus. The candidates of one
//! place (`place`) are told apart by [`fitted`].

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_hal::i2c::{Error as _, ErrorKind, I2c, Operation};

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
/// constants a board package generates from its definition, through the
/// `unsafe` [`Device::from_definition`].
///
/// Why `unsafe` is the seal: code built into the image outside the
/// extension interface (DAGON's crates, the board package, the
/// application's start-up) is the trusted base of one address space;
/// extension code is the boundary (design 2.2 and 4), and `dagon gen`
/// requires `#![forbid(unsafe_code)]` of extension crates, so no extension
/// can call it.
///
/// ```compile_fail,E0451
/// let forged = dagon_events::Device { addr: 0x6E, writable: &[0x0C], max: 1 };
/// ```
///
/// ```compile_fail,E0133
/// let forged = dagon_events::Device::from_definition(0x6E, &[0x0C], 1);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    addr: u8,
    writable: &'static [u8],
    max: usize,
}

/// Why the bus port refused or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError<E> {
    /// The bus is not put in yet, or another holder has its lease.
    Busy,
    /// The definition does not list the register as writable.
    NotWritable(u8),
    /// More bytes than the device takes in one transfer.
    TooLong(usize),
    /// The driver's error.
    Bus(E),
}

impl Device {
    /// The device of a validated board definition's `[[device]]`.
    ///
    /// # Safety
    ///
    /// Only the definition code generator calls this, with the address,
    /// writable registers and maximum transfer (data bytes) of a
    /// `[[device]]` the definition loader accepted: these are what the
    /// port enforces, so any other caller could widen them.
    #[doc(hidden)]
    #[allow(unsafe_code)]
    pub const unsafe fn from_definition(addr: u8, writable: &'static [u8], max: usize) -> Self {
        assert!(addr >= 0x08 && addr <= 0x77, "a 7-bit I2C device address");
        assert!(max > 0, "a device takes at least one byte a transfer");
        Device {
            addr,
            writable,
            max,
        }
    }

    /// 7-bit address.
    pub const fn addr(&self) -> u8 {
        self.addr
    }

    /// The registers anyone outside DAGON may write.
    pub const fn writable(&self) -> &'static [u8] {
        self.writable
    }

    /// Most data bytes the device takes in one transfer.
    pub const fn max_transfer(&self) -> usize {
        self.max
    }

    /// Writes `bytes` (at most [`Device::max_transfer`]) to register `reg`
    /// in one transfer (register address, then the bytes).
    pub fn write<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        bytes: &[u8],
    ) -> Result<(), BusError<B::Error>> {
        if !self.writable.contains(&reg) {
            return Err(BusError::NotWritable(reg));
        }
        if bytes.len() > self.max {
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

    /// Reads `buf.len()` bytes (at most [`Device::max_transfer`]) from
    /// register `reg`.
    pub fn read<B: I2c>(
        &self,
        bus: &Shared<B>,
        reg: u8,
        buf: &mut [u8],
    ) -> Result<(), BusError<B::Error>> {
        if buf.len() > self.max {
            return Err(BusError::TooLong(buf.len()));
        }
        bus.with(|b| b.write_read(self.addr, &[reg], buf))
            .ok_or(BusError::Busy)?
            .map_err(BusError::Bus)
    }
}

/// A device that may not be fitted, as a `[[device]]` with a `probe`
/// declares it: its [`Device`] is given only once the part answered the
/// probe, so no code holds a device that is not there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probed {
    device: Device,
    reg: u8,
    mask: u8,
    value: Option<u8>,
}

impl Probed {
    /// `device`, fitted when it answers a read of register `reg` and,
    /// with a `value`, when the byte read is `value` under `mask`.
    pub const fn new(device: Device, reg: u8, mask: u8, value: Option<u8>) -> Self {
        if let Some(v) = value {
            assert!(mask != 0 && v & !mask == 0, "a value its mask can show");
        }
        Probed {
            device,
            reg,
            mask,
            value,
        }
    }

    /// 7-bit address the probe reads at.
    pub const fn addr(&self) -> u8 {
        self.device.addr
    }

    /// The device when it is fitted: `None` when nothing acknowledges
    /// its address or register, or the register reads another value
    /// (another part at the address). Any other failure of the bus is an
    /// error: it does not tell whether the device is there.
    pub fn find<B: I2c>(&self, bus: &Shared<B>) -> Result<Option<Device>, BusError<B::Error>> {
        let mut byte = [0u8];
        let read = bus
            .with(|b| b.write_read(self.device.addr, &[self.reg], &mut byte))
            .ok_or(BusError::Busy)?;
        match read {
            Ok(()) => Ok(match self.value {
                Some(v) if byte[0] & self.mask != v => None,
                _ => Some(self.device),
            }),
            Err(e) if matches!(e.kind(), ErrorKind::NoAcknowledge(_)) => Ok(None),
            Err(e) => Err(BusError::Bus(e)),
        }
    }
}

/// One candidate for a place: its probe, and the place's variant for it.
pub type Candidate<T> = (Probed, fn(Device) -> T);

/// What is fitted at a place with several candidates: the first of
/// `candidates` (a generated `CANDIDATES` table, in the definition's
/// order) that answers its probe, as the place's type; `None` when none
/// does.
pub fn fitted<B: I2c, T>(
    candidates: &[Candidate<T>],
    bus: &Shared<B>,
) -> Result<Option<T>, BusError<B::Error>> {
    for (probe, part) in candidates {
        if let Some(device) = probe.find(bus)? {
            return Ok(Some(part(device)));
        }
    }
    Ok(None)
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
    const IOE: Device = unsafe { Device::from_definition(0x4F, &[0x05], 2) };

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
        // The device's own bound, both ways.
        assert_eq!(IOE.write(&bus, 0x05, &[0; 3]), Err(BusError::TooLong(3)));
        assert_eq!(IOE.read(&bus, 0x03, &mut [0; 3]), Err(BusError::TooLong(3)));
        let log = bus.with(|l| core::mem::take(&mut l.0)).unwrap();
        assert_eq!(
            log,
            [(0x4F, std::vec![0x05, 1, 2]), (0x4F, std::vec![0x03])]
        );
    }

    /// The parts fitted on a bus: (address, register, the byte it reads).
    /// Any other address or register is not acknowledged; `stuck` fails
    /// every transfer.
    struct Fitted {
        parts: &'static [(u8, u8, u8)],
        stuck: bool,
    }

    impl ErrorType for Fitted {
        type Error = ErrorKind;
    }

    impl I2c for Fitted {
        fn transaction(&mut self, addr: u8, ops: &mut [Operation<'_>]) -> Result<(), Self::Error> {
            use embedded_hal::i2c::NoAcknowledgeSource::{Address, Data};
            if self.stuck {
                return Err(ErrorKind::Bus);
            }
            if !self.parts.iter().any(|p| p.0 == addr) {
                return Err(ErrorKind::NoAcknowledge(Address));
            }
            let mut reg = None;
            for op in ops {
                match op {
                    Operation::Write(b) => reg = b.first().copied(),
                    Operation::Read(b) => {
                        let part = self.parts.iter().find(|p| p.0 == addr && Some(p.1) == reg);
                        b.fill(part.ok_or(ErrorKind::NoAcknowledge(Data))?.2);
                    }
                }
            }
            Ok(())
        }
    }

    fn bus_with(parts: &'static [(u8, u8, u8)], stuck: bool) -> Shared<Fitted> {
        let bus = Shared::new();
        assert!(bus.put(Fitted { parts, stuck }).is_ok());
        bus
    }

    // SAFETY: test devices standing in for generated ones.
    #[allow(unsafe_code)]
    const TOUCH_A: Device = unsafe { Device::from_definition(0x15, &[], 4) };
    #[allow(unsafe_code)]
    const TOUCH_B: Device = unsafe { Device::from_definition(0x15, &[0x01], 4) };
    #[allow(unsafe_code)]
    const SENSOR: Device = unsafe { Device::from_definition(0x40, &[], 2) };

    #[test]
    fn a_probed_device_is_given_only_when_its_part_answers() {
        // The chip id of part A under the mask's bits.
        let a = Probed::new(TOUCH_A, 0xA7, 0xF0, Some(0xB0));
        let any = Probed::new(SENSOR, 0x00, 0xFF, None);
        let fitted = bus_with(&[(0x15, 0xA7, 0xB5), (0x40, 0x00, 0x00)], false);
        assert_eq!(a.find(&fitted), Ok(Some(TOUCH_A)));
        assert_eq!(any.find(&fitted), Ok(Some(SENSOR)));
        // Nothing at the address, no such register, another part's id.
        assert_eq!(a.find(&bus_with(&[], false)), Ok(None));
        assert_eq!(a.find(&bus_with(&[(0x15, 0x00, 0xB5)], false)), Ok(None));
        assert_eq!(a.find(&bus_with(&[(0x15, 0xA7, 0x11)], false)), Ok(None));
        // A bus that fails, or is not put in, tells nothing of the device.
        assert_eq!(
            a.find(&bus_with(&[(0x15, 0xA7, 0xB5)], true)),
            Err(BusError::Bus(ErrorKind::Bus))
        );
        assert_eq!(a.find(&Shared::<Fitted>::new()), Err(BusError::Busy));
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Touch {
        A(Device),
        B(Device),
    }

    #[test]
    fn the_part_fitted_at_a_place_is_the_candidate_that_answers() {
        // Two parts for one place at one address, told apart by their id.
        let candidates: [Candidate<Touch>; 2] = [
            (Probed::new(TOUCH_A, 0xA7, 0xFF, Some(0xB5)), Touch::A),
            (Probed::new(TOUCH_B, 0xA7, 0xFF, Some(0x11)), Touch::B),
        ];
        let find = |parts| fitted(&candidates, &bus_with(parts, false));
        assert_eq!(find(&[(0x15, 0xA7, 0xB5)]), Ok(Some(Touch::A(TOUCH_A))));
        assert_eq!(find(&[(0x15, 0xA7, 0x11)]), Ok(Some(Touch::B(TOUCH_B))));
        assert_eq!(find(&[]), Ok(None));
        assert_eq!(find(&[(0x15, 0xA7, 0x77)]), Ok(None));
        assert_eq!(
            fitted(&candidates, &bus_with(&[], true)),
            Err(BusError::Bus(ErrorKind::Bus))
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
