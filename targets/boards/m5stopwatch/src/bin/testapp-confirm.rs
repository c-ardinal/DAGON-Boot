//! Test app that confirms itself right after boot (otadata Valid), then
//! feeds the trial watchdog and takes one-key commands on either console:
//! `0` / `1` boot ota_0 / ota_1 on trial, `f` boot factory, `r` reset,
//! `s` status.
#![no_std]
#![no_main]

use core::{cell::RefCell, fmt::Write};

use dagon_board_m5stopwatch::{self as board, Console};
use dagon_mcu_esp32s3::{
    boot::EspBootSelect,
    layout::{SLOT_A, SLOT_B, SLOT_FACTORY},
    sys::{EspReset, RtcWatchdog},
    TRIAL_WDT_MS,
};
use dagon_port::{BootSelectV, Reset, Watchdog};
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::{
    rtc_cntl::Rtc,
    time::{Duration, Instant},
    usb::usb_serial_jtag::UsbSerialJtag,
};

esp_bootloader_esp_idf::esp_app_desc!();

const APP: &str = "testapp-confirm";
const STATUS_PERIOD: Duration = Duration::from_secs(5);

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    let mut wdt = RtcWatchdog::new(Rtc::new(p.RTC_TIMER).rwdt);
    wdt.start(TRIAL_WDT_MS);

    let routed = board::route_rear_uart(p.I2C0, p.GPIO47, p.GPIO48).is_ok();
    let uart = board::rear_uart(p.UART0, p.GPIO43, p.GPIO44);
    let mut con = Console::new(uart, UsbSerialJtag::new(p.USB_DEVICE));
    if !routed {
        let _ = con.write_str("M5IOE1 not reachable, rear pins left at default\r\n");
    }

    let storage = RefCell::new(FlashStorage::new(p.FLASH));
    let mut boot = match EspBootSelect::new(&storage) {
        Ok(b) => b,
        Err(e) => {
            let _ = write!(
                con,
                "dagon {APP}: init failed {e:?}, waiting for watchdog\r\n"
            );
            loop {
                core::hint::spin_loop();
            }
        }
    };
    board::report(&mut con, APP, &boot);
    let _ = write!(con, "dagon {APP}: confirm -> {:?}\r\n", boot.confirm());
    board::report(&mut con, APP, &boot);
    let _ = con.write_str("keys: 0/1 = try ota_0/ota_1, f = factory, r = reset, s = status\r\n");

    let mut last = Instant::now();
    loop {
        wdt.feed();
        if last.elapsed() >= STATUS_PERIOD {
            last = Instant::now();
            board::report(&mut con, APP, &boot);
        }
        let slot = match con.read_byte() {
            Some(b'0') => SLOT_A,
            Some(b'1') => SLOT_B,
            Some(b'f') => SLOT_FACTORY,
            Some(b'r') => EspReset.reset(),
            Some(b's') => {
                board::report(&mut con, APP, &boot);
                continue;
            }
            _ => continue,
        };
        let r = boot.set_next(slot);
        let _ = write!(
            con,
            "dagon {APP}: set_next({}) -> {r:?}\r\n",
            board::slot_name(slot)
        );
        board::report(&mut con, APP, &boot);
        if r.is_ok() {
            EspReset.reset();
        }
    }
}
