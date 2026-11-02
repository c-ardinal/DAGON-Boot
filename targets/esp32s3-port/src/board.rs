//! M5StopWatch board glue: rear-pin routing, the rear UART, a text console for
//! the test apps, and reset-on-panic.

use core::fmt;

use esp_hal::{
    i2c::master::{Config as I2cConfig, Error as I2cError, I2c},
    peripherals::{GPIO43, GPIO44, GPIO47, GPIO48, I2C0, UART0},
    time::{Duration, Instant},
    uart::{Config as UartConfig, Uart},
    usb::usb_serial_jtag::UsbSerialJtag,
    Blocking,
};

/// M5IOE1 I/O expander on the internal SYS I2C bus (SDA G47, SCL G48).
const IOE_ADDR: u8 = 0x4F;
const IOE_REG_MODE_L: u8 = 0x03;
const IOE_REG_OUT_L: u8 = 0x05;
const IOE_REG_DRV_L: u8 = 0x13;
/// PYG1 (PYB_MUX_CTR): low = rear MUX_IO_1/2 carry UART0, high = USB.
const PYG1_BIT: u8 = 1 << 0;

pub const LINK_BAUD: u32 = 115_200;

/// Give up on a USB console write after this long (no host reading).
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(5);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // A hung image would never let the 2nd stage roll back; reset instead.
    esp_hal::system::software_reset()
}

fn ioe_update(i2c: &mut I2c<'_, Blocking>, reg: u8, set: u8, clear: u8) -> Result<(), I2cError> {
    let mut cur = [0u8];
    i2c.write_read(IOE_ADDR, &[reg], &mut cur)?;
    i2c.write(IOE_ADDR, &[reg, (cur[0] & !clear) | set])
}

/// Drives PYG1 low (output latch first, then push-pull output) so the rear
/// pins carry UART0. The I2C bus is released afterwards.
pub fn route_rear_uart(i2c0: I2C0<'_>, sda: GPIO47<'_>, scl: GPIO48<'_>) -> Result<(), I2cError> {
    let mut i2c = I2c::new(i2c0, I2cConfig::default())
        .map_err(|_| I2cError::Timeout)?
        .with_sda(sda)
        .with_scl(scl);
    ioe_update(&mut i2c, IOE_REG_OUT_L, 0, PYG1_BIT)?;
    ioe_update(&mut i2c, IOE_REG_DRV_L, 0, PYG1_BIT)?;
    ioe_update(&mut i2c, IOE_REG_MODE_L, PYG1_BIT, 0)
}

/// Rear UART0 at 115200 8N1 on G43 (TX) / G44 (RX).
pub fn rear_uart<'d>(uart0: UART0<'d>, tx: GPIO43<'d>, rx: GPIO44<'d>) -> Uart<'d, Blocking> {
    // Default config is 115200 8N1 and always valid.
    Uart::new(uart0, UartConfig::default().with_baudrate(LINK_BAUD))
        .unwrap_or_else(|_| esp_hal::system::software_reset())
        .with_tx(tx)
        .with_rx(rx)
}

/// Text console on both the rear UART and USB Serial/JTAG (USB-C). The USB
/// side drops output when no host reads it, so it never stalls the caller.
pub struct Console<'d> {
    uart: Uart<'d, Blocking>,
    usb: UsbSerialJtag<'d, Blocking>,
}

impl<'d> Console<'d> {
    pub fn new(uart: Uart<'d, Blocking>, usb: UsbSerialJtag<'d, Blocking>) -> Self {
        Self { uart, usb }
    }

    /// One received byte from either side, if any.
    pub fn read_byte(&mut self) -> Option<u8> {
        if let Ok(b) = self.usb.read_byte() {
            return Some(b);
        }
        let mut b = [0u8];
        match self.uart.read_buffered(&mut b) {
            Ok(1) => Some(b[0]),
            _ => None,
        }
    }

    fn usb_write(&mut self, s: &[u8]) {
        let start = Instant::now();
        for &b in s {
            while self.usb.write_byte_nb(b).is_err() {
                if start.elapsed() >= USB_WRITE_TIMEOUT {
                    return;
                }
            }
        }
        while self.usb.flush_tx_nb().is_err() {
            if start.elapsed() >= USB_WRITE_TIMEOUT {
                return;
            }
        }
    }
}

impl fmt::Write for Console<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut sent = 0;
        while sent < s.len() {
            sent += self
                .uart
                .write(&s.as_bytes()[sent..])
                .map_err(|_| fmt::Error)?;
        }
        self.usb_write(s.as_bytes());
        Ok(())
    }
}

pub fn slot_name(slot: u8) -> &'static str {
    crate::layout::app_partition(slot).map_or("?", |p| p.name)
}

/// One status line: running slot, otadata selection and state, reset cause,
/// plus a note when the 2nd stage has rolled back.
pub fn report(con: &mut Console<'_>, app: &str, boot: &crate::boot::EspBootSelect<'_, '_>) {
    use dagon_port::{BootSelectV, Reset};
    use fmt::Write;

    let running = boot.current_slot();
    let cause = crate::sys::EspReset.reset_cause();
    let sel = match boot.selection() {
        Ok(sel) => sel,
        Err(e) => {
            let _ = write!(
                con,
                "dagon {app}: running={} otadata error {e:?}\r\n",
                slot_name(running)
            );
            return;
        }
    };
    let _ = write!(
        con,
        "dagon {app}: running={} selected={} state={:?} reset={cause:?}",
        slot_name(running),
        slot_name(sel.slot),
        sel.state,
    );
    let view = crate::otadata::selected_view(running, sel.slot, sel.state.map(|s| s as u32));
    if let Some(v) = view {
        let _ = write!(con, " status[{}]={}", slot_name(v.slot), v.state as u8);
        if let Some(r) = v.reason {
            let _ = write!(con, " reason={r}");
        }
        if v.state == dagon_proto::payload::SlotState::Reverted {
            let _ = write!(con, " ROLLED BACK from {}", slot_name(v.slot));
        }
    }
    let _ = con.write_str("\r\n");
}
