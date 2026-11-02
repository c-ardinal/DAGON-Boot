//! Hub boot core: chooses the hub app slot from the app state log.
//!
//! - A slot pending trial: after 3 boot attempts it rolls back; otherwise it
//!   checks signature and SHA-256, appends BOOT_ATTEMPT and starts it.
//!   A failed check is recorded as a rollback with its reason.
//! - Otherwise the confirmed slot, after a SHA-256 check.
//! - Nothing startable: the backlight blinks (reinstall with BOOTSEL + UF2).
//!
//! Every start arms the watchdog for 8 s; the app keeps feeding it.
#![no_std]
#![no_main]

use dagon_format::header::image_kind;
use dagon_format::statelog::{kind, reason, slot, target};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
use panic_halt as _;
use rp2040_common::arm::jump;
use rp2040_common::image;
use rp2040_common::layout::{APP_LOG, APP_SLOT, APP_SLOT_SIZE, MAX_ATTEMPTS, WDT_MS};
use rp2040_common::log::{self, Mark};
use rp2040_common::rp::RpFlash;

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

/// Half a second at the default 125 MHz system clock.
const BLINK_CYCLES: u32 = 62_500_000;

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());
    let mut flash = RpFlash::new(p.FLASH);
    if let Some(body) = choose(&mut flash) {
        let mut wdt = Watchdog::new(p.WATCHDOG);
        wdt.start(Duration::from_millis(WDT_MS as u64));
        jump(body);
    }
    let mut led = Output::new(p.PIN_25, Level::High);
    loop {
        cortex_m::asm::delay(BLINK_CYCLES);
        led.toggle();
    }
}

fn choose(flash: &mut RpFlash) -> Option<u32> {
    let (fold, _) = log::read(flash, APP_LOG, target::APP).ok()?;
    if let Some(p) = fold.pending.filter(|m| m.slot <= slot::B) {
        let tried = if fold.attempts >= MAX_ATTEMPTS {
            Err(reason::TRIAL_EXHAUSTED)
        } else {
            start(flash, &p, true)
        };
        match tried {
            // An attempt that cannot be counted is not tried (it could loop).
            Ok(body) => {
                if log::append(flash, APP_LOG, kind::BOOT_ATTEMPT, target::APP, p, 0).is_ok() {
                    return Some(body);
                }
            }
            Err(why) => {
                let _ = log::append(flash, APP_LOG, kind::ROLLBACK, target::APP, p, why);
            }
        }
    }
    let c = fold.confirmed.filter(|m| m.slot <= slot::B)?;
    start(flash, &c, false).ok()
}

/// Checks the slot `m` names; `signed` adds the signature check (trial).
fn start(flash: &mut RpFlash, m: &Mark, signed: bool) -> Result<u32, u8> {
    let base = APP_SLOT[m.slot as usize];
    if signed {
        image::verify(flash, base, &PUBKEY)?;
    }
    image::check(flash, base, APP_SLOT_SIZE, m.slot, Some(m), image_kind::APP)
}
