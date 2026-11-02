//! Hub stage0 (never updated): picks the boot core slot from the boot core
//! state log (slot A while the log is empty), checks its SHA-256 and starts
//! it. Holds boot2 for the whole flash. No signature check here: boot core
//! images are verified before they are ever selected.
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use dagon_format::header::image_kind;
use dagon_format::statelog::slot;
use panic_halt as _;
use rp2040_common::arm::{jump, XipFlash};
use rp2040_common::layout::{BL_LOG, BL_SLOT, BL_SLOT_SIZE};
use rp2040_common::{bl, image};

#[link_section = ".boot2"]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_GENERIC_03H;

#[entry]
fn main() -> ! {
    let mut flash = XipFlash;
    let named = bl::confirmed(&mut flash, BL_LOG).filter(|m| m.slot <= slot::B);
    let s = named.map_or(slot::A, |m| m.slot);
    let base = BL_SLOT[s as usize];
    match image::check(
        &mut flash,
        base,
        BL_SLOT_SIZE,
        s,
        named.as_ref(),
        image_kind::BOOTCORE,
    ) {
        Ok(body) => jump(body),
        // Nothing to start: recover with BOOTSEL and the install UF2.
        Err(_) => loop {
            cortex_m::asm::wfi();
        },
    }
}
