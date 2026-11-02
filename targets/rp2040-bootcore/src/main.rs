//! Hub boot core: dagon-core's kind S decision over the hub layout. A
//! pending app slot is tried (signature + SHA-256, BOOT_ATTEMPT before the
//! jump) until the attempts run out, then rolled back; otherwise the
//! confirmed slot starts after a SHA-256 check. dagon-core also requires
//! each header's `load_addr` to be its slot body (the hub's `Flash` speaks
//! XIP addresses). Every start arms the watchdog for 8 s; the app keeps
//! feeding it. With nothing startable the watchdog stops and the backlight
//! blinks at 1 Hz (reinstall with BOOTSEL + UF2). The `diag` build blinks
//! twice at entry.
#![no_std]
#![no_main]

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_format::header::{mcu_id, target_tag};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
#[cfg(not(feature = "diag"))]
use panic_halt as _;
use rp2040_common::arm::{jump, led};
use rp2040_common::layout::{APP_LOG_LAYOUT, APP_SLOTS, HUB_TAG, MAX_ATTEMPTS, WDT_MS};
use rp2040_common::rp::RpFlash;

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

const HALT_MS: u32 = 500;
const BLINK_MS: u32 = 300;

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: mcu_id::RP2040,
    target_tag: target_tag(HUB_TAG),
    max_attempts: MAX_ATTEMPTS,
};

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());
    if cfg!(feature = "diag") {
        led::init();
        led::blinks(2, BLINK_MS, led::PLL_MS);
        led::set(false);
    }
    let mut flash = RpFlash::new(p.FLASH);
    let mut wdt = Watchdog::new(p.WATCHDOG);
    wdt.start(Duration::from_millis(WDT_MS as u64));
    let decision = match Log::load(&mut flash, &APP_LOG_LAYOUT) {
        Ok(mut log) => boot::decide(&mut flash, &mut log, &APP_SLOTS, &CONFIG),
        Err(_) => Decision::NoImage,
    };
    if let Decision::Boot { entry, .. } = decision {
        wdt.feed(Duration::from_millis(WDT_MS as u64));
        jump(entry);
    }
    wdt.stop();
    led::init();
    led::halt(HALT_MS, led::PLL_MS)
}
