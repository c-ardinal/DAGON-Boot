//! M5StopWatch (ESP32-S3R8, 16 MB flash) board layer for DAGON.
//!
//! What this board adds to `dagon-mcu-esp32s3`, every board fact read from
//! `dagon-board.toml` when the package builds ([`def`]):
//!
//! | What | Where |
//! |---|---|
//! | DAGON link | bus `link`: the rear MUX_IO_1/2 as UART0 ([`rear_uart`]), plus USB Serial/JTAG on USB-C |
//! | Internal bus | bus `sys`, shared as [`SYS_I2C`]: DAGON's bring-up and the app take it through `SYS_I2C.with(..)` |
//! | Rear pins UART/USB switch | [`IOE`] (M5IOE1) PYG1, driven low at bring-up ([`route_rear_uart`]) |
//! | Power manager | [`PMIC`] (M5PM1); ENTER_ROM method 2 only with the `pmic-download` feature (unconfirmed command) |
//! | Flash | [`FLASH_SIZE`]; DAGON's partition table from the MCU crate |
//!
//! An app starts DAGON with [`start`] (see the README for the few lines).
//! The display and the other peripherals are the app's, not DAGON's.
#![no_std]

/// The board's facts from `dagon-board.toml`.
#[macro_use]
pub mod def {
    include!(concat!(env!("OUT_DIR"), "/def.rs"));

    /// Pins and blocks by their definition names, and `take!`.
    #[macro_use]
    pub mod pins {
        include!(concat!(env!("OUT_DIR"), "/pins.rs"));
    }

    /// The `[[device]]`s as bus port capabilities.
    pub mod devices {
        include!(concat!(env!("OUT_DIR"), "/devices.rs"));
    }
}

use core::fmt;

use dagon_events::{BoardBringUp, BusError, Device, Shared};
use dagon_mcu_esp32s3::board::BoardDef;
use dagon_mcu_esp32s3::liveness::Liveness;
use dagon_mcu_esp32s3::node::{self, NodeConfig, NodePeripherals, WatchdogArmed};
use def::devices;
use def::pins::{Link, LinkRx, LinkTx, Sys, SysScl, SysSda};
use esp_hal::{
    i2c::master::{Config as I2cConfig, ConfigError, Error as I2cError, I2c},
    peripherals::{FLASH, FROM_CPU_INTR1, RTC_TIMER, USB_DEVICE},
    time::{Duration, Instant},
    uart::Uart,
    usb::usb_serial_jtag::UsbSerialJtag,
    Blocking,
};
use static_cell::StaticCell;

/// Flash size of the board (espflash `--flash-size 16mb`).
pub const FLASH_SIZE: u32 = def::FLASH_BYTES;
/// Image `target_tag` of the node: `DAGON_TARGET_TAG` at build time (the
/// node's tag in the topology; the build fails without it).
pub const TARGET_TAG: &str = match dagon_mcu_esp32s3::TARGET_TAG_OVERRIDE {
    Some(t) => t,
    None => panic!("DAGON_TARGET_TAG is unset: set it to the node's image target tag"),
};

/// The internal bus (`sys`), shared: [`start`] (or [`open_sys_i2c`]) puts
/// it in; DAGON's bring-up and the app use it through `SYS_I2C.with(..)`.
pub static SYS_I2C: Shared<I2c<'static, Blocking>> = Shared::new();

/// M5IOE1 I/O expander on [`SYS_I2C`] (`[[device]] ioe`; its registers
/// are `def::DEVICE_IOE_REG_*`).
pub const IOE: Device = devices::IOE;
/// PYG1 (PYB_MUX_CTR): low = rear MUX_IO_1/2 carry UART0, high = USB.
pub const PYG1_BIT: u8 = 1 << 0;

/// M5PM1 power manager on [`SYS_I2C`] (`[[device]] pmic`; its registers
/// are `def::DEVICE_PMIC_REG_*`).
pub const PMIC: Device = devices::PMIC;
/// The SYS_CMD value that restarts the chip in download mode.
// Unverified: the datasheet gives SYS_CMD = KEY 0xA + CMD 0b11
// (Download); KEY in the high nibble is an assumption, hence the
// `pmic-download` feature gate until M5Stack documents or the hardware
// confirm it.
#[cfg(feature = "pmic-download")]
const PMIC_DOWNLOAD: u8 = 0xA3;
/// Time the M5PM1 gets to reset the chip into download mode.
#[cfg(feature = "pmic-download")]
const PMIC_WAIT_MS: u32 = 1000;

/// Give up on a USB console write after this long (no host reading).
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(5);

/// Puts the internal bus into [`SYS_I2C`] (default configuration).
pub fn open_sys_i2c(
    i2c0: Sys<'static>,
    sda: SysSda<'static>,
    scl: SysScl<'static>,
) -> Result<(), ConfigError> {
    let bus = I2c::new(i2c0, I2cConfig::default())?
        .with_sda(sda)
        .with_scl(scl);
    // Only `start` and a test app put it in, before anyone leases it.
    let _ = SYS_I2C.put(bus);
    Ok(())
}

fn ioe_update(reg: u8, set: u8, clear: u8) -> Result<(), BusError<I2cError>> {
    let mut cur = [0u8];
    IOE.read(&SYS_I2C, reg, &mut cur)?;
    IOE.write(&SYS_I2C, reg, &[(cur[0] & !clear) | set])
}

/// Drives PYG1 low (output latch first, then push-pull output) so the rear
/// pins carry UART0, through [`SYS_I2C`].
pub fn route_rear_uart() -> Result<(), BusError<I2cError>> {
    ioe_update(def::DEVICE_IOE_REG_OUT_L, 0, PYG1_BIT)?;
    ioe_update(def::DEVICE_IOE_REG_DRV_L, 0, PYG1_BIT)?;
    ioe_update(def::DEVICE_IOE_REG_MODE_L, PYG1_BIT, 0)
}

/// The link's bit rate (`[default_link] baud`).
pub const LINK_BAUD: u32 = match def::DEFAULT_LINK_BAUD {
    Some(b) => b,
    None => panic!("dagon-board.toml: default_link.baud"),
};

/// The DAGON link UART (bus `link`) at [`LINK_BAUD`] 8N1.
pub fn rear_uart<'d>(uart0: Link<'d>, tx: LinkTx<'d>, rx: LinkRx<'d>) -> Uart<'d, Blocking> {
    // A definition-checked bit rate, 8N1: always valid.
    Uart::new(uart0, dagon_mcu_esp32s3::link::uart_config(LINK_BAUD))
        .unwrap_or_else(|_| esp_hal::system::software_reset())
        .with_tx(tx)
        .with_rx(rx)
}

/// The board's [`BoardDef`] and [`BoardBringUp`]: both reach the internal
/// bus through [`SYS_I2C`].
pub struct M5StopWatch;

impl BoardBringUp for M5StopWatch {
    /// Rear pins to UART0. Without the expander they stay at their default
    /// (UART).
    fn bring_up(&mut self) {
        let _ = route_rear_uart();
    }
}

impl BoardDef for M5StopWatch {
    fn bring_up(&mut self) {
        BoardBringUp::bring_up(self);
    }

    fn pmic_download_supported(&self) -> bool {
        cfg!(feature = "pmic-download")
    }

    #[cfg(feature = "pmic-download")]
    fn pmic_download(&mut self) {
        if PMIC
            .write(&SYS_I2C, def::DEVICE_PMIC_REG_SYS_CMD, &[PMIC_DOWNLOAD])
            .is_ok()
        {
            esp_hal::delay::Delay::new().delay_millis(PMIC_WAIT_MS);
        }
    }
}

/// The peripherals DAGON takes on this board; fill it with [`dagon_pins!`].
pub struct DagonPins {
    pub rtc_timer: RTC_TIMER<'static>,
    pub link: Link<'static>,
    pub link_tx: LinkTx<'static>,
    pub link_rx: LinkRx<'static>,
    pub usb_device: USB_DEVICE<'static>,
    pub flash: FLASH<'static>,
    pub sys: Sys<'static>,
    pub sys_sda: SysSda<'static>,
    pub sys_scl: SysScl<'static>,
    pub link_irq: FROM_CPU_INTR1<'static>,
}

/// Moves the peripherals DAGON needs out of `esp_hal::init`'s result:
/// `let dagon = dagon_pins!(p);`
#[macro_export]
macro_rules! dagon_pins {
    ($p:ident) => {
        $crate::DagonPins {
            rtc_timer: $p.RTC_TIMER,
            link: $crate::take!($p.link),
            link_tx: $crate::take!($p.link_tx),
            link_rx: $crate::take!($p.link_rx),
            usb_device: $p.USB_DEVICE,
            flash: $p.FLASH,
            sys: $crate::take!($p.sys),
            sys_sda: $crate::take!($p.sys_sda),
            sys_scl: $crate::take!($p.sys_scl),
            link_irq: $p.FROM_CPU_INTR1,
        }
    };
}

/// Starts DAGON on this board: puts the internal bus into [`SYS_I2C`], then
/// the link task on the rear UART and USB-C, at high priority. Call once,
/// after `node::arm_watchdog` and `esp_rtos::start`.
pub fn start(armed: WatchdogArmed, pins: DagonPins, cfg: NodeConfig, liveness: &'static Liveness) {
    static BOARD: StaticCell<M5StopWatch> = StaticCell::new();
    // Without the bus the bring-up finds nothing to drive: the rear pins
    // keep their default (UART).
    let _ = open_sys_i2c(pins.sys, pins.sys_sda, pins.sys_scl);
    let p = NodePeripherals {
        rtc_timer: pins.rtc_timer,
        uart: rear_uart(pins.link, pins.link_tx, pins.link_rx),
        usb_device: pins.usb_device,
        flash: pins.flash,
        target_tag: TARGET_TAG,
        board: BOARD.init(M5StopWatch),
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
