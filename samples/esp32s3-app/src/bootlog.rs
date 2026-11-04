//! Start-up log of the panel power sequence, on the USB console (USB
//! Serial/JTAG) before DAGON takes it, plus a 32-byte snapshot in the
//! M5PM1's RTC RAM.
//!
//! The console is only read when a PC has the port open, and a start from
//! the power button enumerates USB anew, so its first lines are usually
//! lost. The snapshot is the evidence of such a start: each start prints the
//! previous start's snapshot first (the M5PM1 keeps it across chip resets
//! and power-button off/on), then writes its own. A missing or unchanged
//! snapshot after a power-button start means the app did not get that far.
//!
//! Output never blocks: a byte the USB FIFO does not take within
//! [`BYTE_WAIT_US`] ends the console for this start (no PC reading).

use core::fmt::{self, Write};

use esp_hal::{
    delay::Delay, peripherals::USB_DEVICE, usb::usb_serial_jtag::UsbSerialJtag, Blocking,
};

/// Longest wait for room in the USB FIFO before the console gives up.
const BYTE_WAIT_US: u32 = 2_000;
const POLL_US: u32 = 10;

/// M5PM1 RTC RAM (0xA0..0xBF): kept while the M5PM1 has power.
pub const RTC_RAM: u8 = 0xA0;
pub const SNAPSHOT_LEN: usize = 32;
/// First byte of a snapshot written by this app.
pub const MAGIC: u8 = 0xD5;

/// Snapshot layout (byte offsets).
pub mod at {
    pub const MAGIC: usize = 0;
    /// Starts counted while the M5PM1 kept the RAM.
    pub const STARTS: usize = 1;
    /// ESP reset reason (`SocResetReason` as u8, 0xFF unknown).
    pub const RESET: usize = 2;
    /// bit0 PM1 settings ok, bit1 panel powered, bits 7:4 L3B_EN checks.
    pub const RESULT: usize = 3;
    /// M5PM1 registers [`super::PM1_REGS`] before the sequence.
    pub const PM1_BEFORE: usize = 4;
    /// M5PM1 registers 0x06, 0x07, 0x09 after the sequence.
    pub const PM1_AFTER: usize = 22;
    /// M5IOE1 0x03, 0x05, 0x07 (low bytes), 0x13, 0x23 after the sequence.
    pub const IOE_AFTER: usize = 25;
}

/// M5PM1 registers recorded before the sequence: PWR_SRC, WAKE_SRC,
/// PWR_CFG, HOLD_CFG, I2C_CFG, WDT_CNT, GPIO MODE/OUT/IN/DRV, PUPD0, FUNC0,
/// IRQ_STATUS2/3, BTN_STATUS, BTN_CFG_1/2, IRQ_STATUS1.
pub const PM1_REGS: [u8; 18] = [
    0x04, 0x05, 0x06, 0x07, 0x09, 0x0A, 0x10, 0x11, 0x12, 0x13, 0x14, 0x16, 0x41, 0x42, 0x48, 0x49,
    0x4A, 0x40,
];

pub struct Console<'a> {
    usb: Option<UsbSerialJtag<'a, Blocking>>,
    delay: Delay,
}

impl<'a> Console<'a> {
    pub fn new(usb: USB_DEVICE<'a>) -> Self {
        Console {
            usb: Some(UsbSerialJtag::new(usb)),
            delay: Delay::new(),
        }
    }

    fn put(&mut self, b: u8) {
        let Some(usb) = self.usb.as_mut() else {
            return;
        };
        let mut waited = 0;
        while usb.write_byte_nb(b).is_err() {
            if waited >= BYTE_WAIT_US {
                self.usb = None;
                return;
            }
            self.delay.delay_micros(POLL_US);
            waited += POLL_US;
        }
    }

    /// One line, sent at once (the USB FIFO is flushed).
    pub fn line(&mut self, args: fmt::Arguments) {
        let _ = self.write_fmt(args);
        self.put(b'\r');
        self.put(b'\n');
        if let Some(usb) = self.usb.as_mut() {
            let _ = usb.flush_tx_nb();
        }
    }
}

impl Write for Console<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        s.bytes().for_each(|b| self.put(b));
        Ok(())
    }
}

/// `bytes` as hex pairs (`--` for a read that failed).
pub struct Hex<'a>(pub &'a [Option<u8>]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            match b {
                Some(b) => write!(f, "{b:02X}")?,
                None => f.write_str("--")?,
            }
        }
        Ok(())
    }
}
