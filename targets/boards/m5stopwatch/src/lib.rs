//! M5StopWatch (ESP32-S3R8, 16 MB flash) board layer for DAGON.
//!
//! What this board adds to `dagon-mcu-esp32s3`:
//!
//! | What | Where |
//! |---|---|
//! | DAGON link | rear bus MUX_IO_1/2 = UART0 TX G43 / RX G44 ([`rear_uart`]), plus USB Serial/JTAG on USB-C |
//! | Rear pins UART/USB switch | M5IOE1 IO expander PYG1 on the internal I2C (SDA G47, SCL G48), driven low at bring-up |
//! | Power manager | M5PM1 on the internal I2C; ENTER_ROM method 2 only with the `pmic-download` feature (unconfirmed values) |
//! | Flash | 16 MB ([`FLASH_SIZE`]); DAGON's partition table from the MCU crate |
//!
//! An app starts DAGON with [`start`] (see the README for the few lines).
//! The display and the other peripherals are the app's, not DAGON's.
#![no_std]

use core::fmt;

use dagon_mcu_esp32s3::board::BoardDef;
use dagon_mcu_esp32s3::liveness::Liveness;
use dagon_mcu_esp32s3::node::{self, NodeConfig, NodePeripherals, WatchdogArmed};
use esp_hal::{
    i2c::master::{Config as I2cConfig, Error as I2cError, I2c},
    peripherals::{
        FLASH, FROM_CPU_INTR1, GPIO43, GPIO44, GPIO47, GPIO48, I2C0, RTC_TIMER, UART0, USB_DEVICE,
    },
    time::{Duration, Instant},
    uart::Uart,
    usb::usb_serial_jtag::UsbSerialJtag,
    Blocking,
};
use static_cell::StaticCell;

/// Flash size of the board (espflash `--flash-size 16mb`).
pub const FLASH_SIZE: u32 = 16 * 1024 * 1024;
/// Image `target_tag` of this board's node (the topology's `watch` node);
/// `DAGON_TARGET_TAG` at build time overrides it.
pub const TARGET_TAG: &str = "watch-s3";

/// This board's definition id (`dagon-board.toml`).
pub const BOARD_ID: &str = "m5stopwatch";

/// M5IOE1 I/O expander on the internal SYS I2C bus (SDA G47, SCL G48).
const IOE_ADDR: u8 = 0x4F;
const IOE_REG_MODE_L: u8 = 0x03;
const IOE_REG_OUT_L: u8 = 0x05;
const IOE_REG_DRV_L: u8 = 0x13;
/// PYG1 (PYB_MUX_CTR): low = rear MUX_IO_1/2 carry UART0, high = USB.
const PYG1_BIT: u8 = 1 << 0;

/// M5PM1 power manager on the internal SYS I2C.
// Unverified: the datasheet gives SYS_CMD 0x0C = KEY 0xA + CMD 0b11
// (Download); KEY in the high nibble and the 7-bit address 0x6E are
// assumptions, hence the `pmic-download` feature gate until M5Stack
// documents or the hardware confirm them.
#[cfg(feature = "pmic-download")]
const M5PM1_ADDR: u8 = 0x6E;
#[cfg(feature = "pmic-download")]
const M5PM1_SYS_CMD: u8 = 0x0C;
#[cfg(feature = "pmic-download")]
const M5PM1_DOWNLOAD: u8 = 0xA3;
/// Time the M5PM1 gets to reset the chip into download mode.
#[cfg(feature = "pmic-download")]
const M5PM1_WAIT_MS: u32 = 1000;

/// Give up on a USB console write after this long (no host reading).
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(5);

fn ioe_update(i2c: &mut I2c<'_, Blocking>, reg: u8, set: u8, clear: u8) -> Result<(), I2cError> {
    let mut cur = [0u8];
    i2c.write_read(IOE_ADDR, &[reg], &mut cur)?;
    i2c.write(IOE_ADDR, &[reg, (cur[0] & !clear) | set])
}

/// The internal SYS I2C bus, opened for one job and released afterwards.
fn sys_i2c<'a>(
    i2c0: &'a mut I2C0<'static>,
    sda: &'a mut GPIO47<'static>,
    scl: &'a mut GPIO48<'static>,
) -> Result<I2c<'a, Blocking>, I2cError> {
    Ok(I2c::new(i2c0.reborrow(), I2cConfig::default())
        .map_err(|_| I2cError::Timeout)?
        .with_sda(sda.reborrow())
        .with_scl(scl.reborrow()))
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

/// The DAGON link UART: UART0 on the rear pins, `link::uart_config()`.
pub fn rear_uart<'d>(uart0: UART0<'d>, tx: GPIO43<'d>, rx: GPIO44<'d>) -> Uart<'d, Blocking> {
    // The link settings (115200 8N1) are always valid.
    Uart::new(uart0, dagon_mcu_esp32s3::link::uart_config())
        .unwrap_or_else(|_| esp_hal::system::software_reset())
        .with_tx(tx)
        .with_rx(rx)
}

/// The board's [`BoardDef`]: holds the internal I2C bus for the expander (and
/// the power manager). Owned by the DAGON link task once started.
pub struct M5StopWatch {
    i2c0: I2C0<'static>,
    sda: GPIO47<'static>,
    scl: GPIO48<'static>,
}

impl BoardDef for M5StopWatch {
    fn id(&self) -> Option<&'static str> {
        Some(BOARD_ID)
    }

    /// Rear pins to UART0.
    fn bring_up(&mut self) {
        let Ok(mut i2c) = sys_i2c(&mut self.i2c0, &mut self.sda, &mut self.scl) else {
            return;
        };
        // Without the expander the rear pins stay at their default (UART).
        let _ = ioe_update(&mut i2c, IOE_REG_OUT_L, 0, PYG1_BIT)
            .and_then(|_| ioe_update(&mut i2c, IOE_REG_DRV_L, 0, PYG1_BIT))
            .and_then(|_| ioe_update(&mut i2c, IOE_REG_MODE_L, PYG1_BIT, 0));
    }

    fn pmic_download_supported(&self) -> bool {
        cfg!(feature = "pmic-download")
    }

    #[cfg(feature = "pmic-download")]
    fn pmic_download(&mut self) {
        let Ok(mut i2c) = sys_i2c(&mut self.i2c0, &mut self.sda, &mut self.scl) else {
            return;
        };
        if i2c
            .write(M5PM1_ADDR, &[M5PM1_SYS_CMD, M5PM1_DOWNLOAD])
            .is_ok()
        {
            esp_hal::delay::Delay::new().delay_millis(M5PM1_WAIT_MS);
        }
    }
}

/// The peripherals DAGON takes on this board; fill it with [`dagon_pins!`].
pub struct DagonPins {
    pub rtc_timer: RTC_TIMER<'static>,
    pub uart0: UART0<'static>,
    pub gpio43: GPIO43<'static>,
    pub gpio44: GPIO44<'static>,
    pub usb_device: USB_DEVICE<'static>,
    pub flash: FLASH<'static>,
    pub i2c0: I2C0<'static>,
    pub gpio47: GPIO47<'static>,
    pub gpio48: GPIO48<'static>,
    pub link_irq: FROM_CPU_INTR1<'static>,
}

/// Moves the peripherals DAGON needs out of `esp_hal::init`'s result:
/// `let dagon = dagon_pins!(p);`
#[macro_export]
macro_rules! dagon_pins {
    ($p:ident) => {
        $crate::DagonPins {
            rtc_timer: $p.RTC_TIMER,
            uart0: $p.UART0,
            gpio43: $p.GPIO43,
            gpio44: $p.GPIO44,
            usb_device: $p.USB_DEVICE,
            flash: $p.FLASH,
            i2c0: $p.I2C0,
            gpio47: $p.GPIO47,
            gpio48: $p.GPIO48,
            link_irq: $p.FROM_CPU_INTR1,
        }
    };
}

/// Starts DAGON on this board: the link task on the rear UART and USB-C,
/// at high priority. Call once, after `node::arm_watchdog` and
/// `esp_rtos::start`.
pub fn start(armed: WatchdogArmed, pins: DagonPins, cfg: NodeConfig, liveness: &'static Liveness) {
    static BOARD: StaticCell<M5StopWatch> = StaticCell::new();
    let board = BOARD.init(M5StopWatch {
        i2c0: pins.i2c0,
        sda: pins.gpio47,
        scl: pins.gpio48,
    });
    let p = NodePeripherals {
        rtc_timer: pins.rtc_timer,
        uart: rear_uart(pins.uart0, pins.gpio43, pins.gpio44),
        usb_device: pins.usb_device,
        flash: pins.flash,
        target_tag: TARGET_TAG,
        board,
    };
    node::start(armed, p, pins.link_irq, cfg, liveness);
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
    dagon_mcu_esp32s3::layout::app_partition(slot).map_or("?", |p| p.name)
}

/// One status line: running slot, otadata selection and state, reset cause,
/// plus a note when the 2nd stage has rolled back.
pub fn report(
    con: &mut Console<'_>,
    app: &str,
    boot: &dagon_mcu_esp32s3::boot::EspBootSelect<'_, '_>,
) {
    use dagon_port::{BootSelectV, Reset};
    use fmt::Write;

    let running = boot.current_slot();
    let cause = dagon_mcu_esp32s3::sys::EspReset.reset_cause();
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
    let view =
        dagon_mcu_esp32s3::otadata::selected_view(running, sel.slot, sel.state.map(|s| s as u32));
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
