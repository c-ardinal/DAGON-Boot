//! Port contract: what a board provides to DAGON.
//!
//! Node side: [`Flash`], [`Link`], [`Reset`], boot selection ([`BootSelectS`]
//! for kind S, [`BootSelectV`] for kind V), [`Clock`], [`Watchdog`].
//! Hub side: [`StreamPort`], [`I2cBus`], [`SubControl`].
//!
//! Link-type and hub-side traits are async (they wait on peripherals or on a
//! remote hub); flash, clock and watchdog are blocking. This crate only
//! declares traits; `unsafe` belongs in the board implementations (jump,
//! RAM-resident flash routines, registers).
//!
//! Compatibility rule: these traits are frozen. Any method added later comes
//! with a default implementation, so existing ports keep compiling.
#![no_std]
#![allow(async_fn_in_trait)]

/// How a board switches between A/B images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PortKind {
    /// Kind S: a DAGON boot core picks and jumps to the slot.
    SelfBoot = 0x01,
    /// Kind V: the vendor second-stage loader picks the slot.
    VendorStage2 = 0x02,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashRegion {
    pub start: u32,
    pub len: u32,
    pub erase_size: u32,
    pub write_size: u32,
    pub erased_value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashError {
    /// Address range outside every region.
    Range,
    /// Not aligned to the erase or write unit.
    Align,
    /// Erase, program or read-back failure.
    Hw,
}

/// Contract: erase only whole erase units; write only aligned to the write
/// unit into erased memory; `read` reflects completed erase/write.
pub trait Flash {
    fn geometry(&self) -> &[FlashRegion];
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError>;
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError>;
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Uart,
    I2cTarget,
    UsbCdc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkError {
    Timeout,
    /// Frame longer than the buffer or the link's max payload.
    Overflow,
    Io,
}

/// Node side of a DAGON link, carrying whole frames.
///
/// UART implementations cut frames out of the byte stream by SOF search and
/// CRC. I2C target implementations run the mailbox: after `recv_frame`
/// returns, the node may erase or write flash before calling `send_frame`, so
/// the implementation must never stretch the clock in that window (NACK the
/// own address, or answer BUSY from RAM).
pub trait Link {
    /// Waits up to `timeout_ms` for one complete frame; returns its length.
    async fn recv_frame(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, LinkError>;

    /// Sends one frame and returns only once it has left the node: the last
    /// bit is on the wire (UART: transmitter empty, not just queued; USB: the
    /// host has taken the data) or, on an I2C target, the master has read the
    /// whole response. Callers reset right after the reply to REBOOT, so
    /// returning earlier would lose that reply.
    async fn send_frame(&mut self, frame: &[u8]) -> Result<(), LinkError>;
    fn kind(&self) -> LinkKind;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetCause {
    PowerOn,
    Watchdog,
    Software,
    Pin,
    Unknown,
}

pub trait Reset {
    /// Normal system reset.
    fn reset(&mut self) -> !;

    fn reset_cause(&self) -> ResetCause {
        ResetCause::Unknown
    }
}

/// Kind S: start the image whose body (vector table) begins at `entry`.
pub trait BootSelectS {
    fn boot(&mut self, entry: u32) -> !;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootSelectError {
    /// Vendor selection data could not be written or read back.
    Write,
    /// The slot is not a valid boot candidate.
    Slot,
}

/// Kind V: drive the vendor loader's selection data (ESP: otadata).
/// Slots: 0 = A, 1 = B, 2 = factory.
pub trait BootSelectV {
    fn current_slot(&self) -> u8;
    fn set_next(&mut self, slot: u8) -> Result<(), BootSelectError>;
    fn select_factory(&mut self) -> Result<(), BootSelectError>;
}

/// Monotonic time.
pub trait Clock {
    fn now_us(&self) -> u64;
    fn delay_us(&mut self, us: u32);
}

/// Resets the system unless fed within the timeout after `start`.
pub trait Watchdog {
    fn start(&mut self, timeout_ms: u32);
    fn feed(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError {
    Nack,
    Timeout,
    Io,
}

/// Hub: point-to-point byte stream to a UART node (hardware or PIO UART).
pub trait StreamPort {
    /// Reads at least one byte, waiting up to `timeout_ms`; returns the count.
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError>;
    async fn write(&mut self, data: &[u8]) -> Result<(), BusError>;
}

/// Hub: master on the shared I2C bus. `addr` is the 7-bit address.
pub trait I2cBus {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError>;
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError>;
    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError>;
}

/// NRST action, as in the SUBCTL command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Nrst {
    /// Release (Hi-Z).
    Release = 0,
    /// Hold low.
    Low = 1,
    /// Low for about 10 ms, then release; timed by the hub.
    Pulse = 2,
}

/// Hub: control lines of a sub MCU. NRST is only pulled low or released
/// (Hi-Z), never driven high.
///
/// Implemented on the hub over GPIO, and on the PC side by sending one SUBCTL
/// frame per call and awaiting the reply (a hub timeout maps to
/// [`BusError::Timeout`]).
pub trait SubControl {
    /// Sets BOOT0 (`true` = high, ROM loader on the next reset) and applies
    /// `nrst`, in that order, so a [`Nrst::Pulse`] samples the new BOOT0.
    /// Returns after the action (including the whole pulse) is done.
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError>;

    /// Resets the sub MCU with a ~10 ms NRST pulse, BOOT0 as given.
    async fn pulse_reset(&mut self, boot0_high: bool) -> Result<(), BusError> {
        self.set_lines(Nrst::Pulse, boot0_high).await
    }
}
