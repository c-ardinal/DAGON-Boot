//! Hub stage0 (never updated): picks the boot core slot from the boot core
//! state log (slot A while the log is empty), checks its SHA-256 and starts
//! it. Holds boot2 for the whole flash. No signature check here: boot core
//! images are verified before they are ever selected.
//!
//! Nothing to start: the backlight blinks fast (100 ms) forever. The `diag`
//! build lights it at entry; a panic blinks the code of the last step
//! (1 entry, 2 reading the log, 3 checking the boot core); a failed check
//! blinks 4 (5 if the log named no slot), a short pause, then which test
//! failed (`image::Fail`, 7 if SHA-256 itself is wrong), then a long pause.
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use dagon_format::header::image_kind;
use dagon_format::statelog::slot;
#[cfg(not(feature = "diag"))]
use panic_halt as _;
use rp2040_common::arm::{jump, led, step, XipFlash};
use rp2040_common::layout::{BL_LOG, BL_SLOT, BL_SLOT_SIZE};
use rp2040_common::{bl, image};

#[link_section = ".boot2"]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_GENERIC_03H;

const HALT_MS: u32 = 100;

#[entry]
fn main() -> ! {
    step(1);
    if cfg!(feature = "diag") {
        led::init();
    }
    let mut flash = XipFlash;
    step(2);
    let named = bl::confirmed(&mut flash, BL_LOG).filter(|m| m.slot <= slot::B);
    let s = named.map_or(slot::A, |m| m.slot);
    let base = BL_SLOT[s as usize];
    step(3);
    match image::check(
        &mut flash,
        base,
        BL_SLOT_SIZE,
        s,
        named.as_ref(),
        image_kind::BOOTCORE,
    ) {
        Ok(body) => {
            if cfg!(feature = "diag") {
                led::set(false);
            }
            jump(body)
        }
        // Nothing to start: recover with BOOTSEL and the install UF2.
        Err(why) => {
            led::init();
            if cfg!(feature = "diag") {
                // 4, then which test failed (7: SHA-256 itself is wrong);
                // 5 instead of 4 when the boot core log names no slot.
                let k = if image::sha_ok() { why as u8 } else { 7 };
                led::code2(if named.is_some() { 4 } else { 5 }, k)
            }
            led::halt(HALT_MS, led::ROSC_MS)
        }
    }
}
