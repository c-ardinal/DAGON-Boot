//! Test app that never confirms: it starts the trial watchdog, reports, and
//! stops feeding. The watchdog resets it and the 2nd stage, finding the slot
//! still PendingVerify, marks it Aborted and boots the previous slot.
#![no_std]
#![no_main]

use core::{cell::RefCell, fmt::Write};

use dagon_board_m5stopwatch::{self as board, Console};
use dagon_mcu_esp32s3::{boot::EspBootSelect, sys::RtcWatchdog, TRIAL_WDT_MS};
use dagon_port::Watchdog;
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::{
    rtc_cntl::Rtc,
    time::{Duration, Instant},
    usb::usb_serial_jtag::UsbSerialJtag,
};

esp_bootloader_esp_idf::esp_app_desc!();

const APP: &str = "testapp-noconfirm";

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    let mut wdt = RtcWatchdog::new(Rtc::new(p.RTC_TIMER).rwdt);
    wdt.start(TRIAL_WDT_MS);

    let routed = board::open_sys_i2c(
        board::take!(p.sys),
        board::take!(p.sys_sda),
        board::take!(p.sys_scl),
    )
    .is_ok()
        && board::route_rear_uart().is_ok();
    let uart = board::rear_uart(
        board::take!(p.link),
        board::take!(p.link_tx),
        board::take!(p.link_rx),
    );
    let mut con = Console::new(uart, UsbSerialJtag::new(p.USB_DEVICE));
    if !routed {
        let _ = con.write_str("M5IOE1 not reachable, rear pins left at default\r\n");
    }

    let storage = RefCell::new(FlashStorage::new(p.FLASH));
    match EspBootSelect::new(&storage) {
        Ok(boot) => board::report(&mut con, APP, &boot),
        Err(e) => {
            let _ = write!(con, "dagon {APP}: init failed {e:?}\r\n");
        }
    }
    let _ = write!(
        con,
        "dagon {APP}: not confirming, watchdog reset in {TRIAL_WDT_MS} ms\r\n"
    );

    let start = Instant::now();
    let mut next = Duration::from_secs(1);
    loop {
        if start.elapsed() >= next {
            let _ = write!(con, "dagon {APP}: {} ms unconfirmed\r\n", next.as_millis());
            next += Duration::from_secs(1);
        }
    }
}
