//! Hub boot core: dagon-core's kind S decision over the hub layout. A
//! pending app slot is tried (signature + SHA-256, BOOT_ATTEMPT before the
//! jump) until the attempts run out, then rolled back; otherwise the
//! confirmed slot starts after a SHA-256 check. Every start arms the
//! watchdog for 8 s; the app keeps feeding it. With nothing startable the
//! backlight blinks (reinstall with BOOTSEL + UF2).
#![no_std]
#![no_main]

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_format::header::{mcu_id, target_tag};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
use panic_halt as _;
use rp2040_common::arm::jump;
use rp2040_common::image;
use rp2040_common::layout::{APP_LOG_LAYOUT, APP_SLOT, APP_SLOTS, HUB_TAG, MAX_ATTEMPTS, WDT_MS};
use rp2040_common::rp::RpFlash;

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

/// Half a second at the default 125 MHz system clock.
const BLINK_CYCLES: u32 = 62_500_000;

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: mcu_id::RP2040,
    target_tag: target_tag(HUB_TAG),
    max_attempts: MAX_ATTEMPTS,
};

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());
    let mut flash = RpFlash::new(p.FLASH);
    let mut wdt = Watchdog::new(p.WATCHDOG);
    wdt.start(Duration::from_millis(WDT_MS as u64));
    let decision = match Log::load(&mut flash, &APP_LOG_LAYOUT) {
        Ok(mut log) => boot::decide(&mut flash, &mut log, &APP_SLOTS, &CONFIG),
        Err(_) => Decision::NoImage,
    };
    match decision {
        Decision::Boot { slot, entry, trial } => {
            if image::linked_here(&mut flash, APP_SLOT[slot as usize]) {
                wdt.feed(Duration::from_millis(WDT_MS as u64));
                jump(entry);
            }
            // Linked for the other slot, it would crash. On trial the
            // watchdog resets so the attempt counts and rolls back; a
            // confirmed image like that cannot recover, so stop and blink.
            if !trial {
                wdt.stop();
            }
        }
        Decision::NoImage => wdt.stop(),
    }
    let mut led = Output::new(p.PIN_25, Level::High);
    loop {
        cortex_m::asm::delay(BLINK_CYCLES);
        led.toggle();
    }
}
