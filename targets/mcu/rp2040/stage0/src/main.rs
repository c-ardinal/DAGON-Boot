//! Hub stage0 (never updated): picks the boot core slot from the boot core
//! state log (slot A while the log is empty), checks its SHA-256 and starts
//! it. Holds boot2 for the whole flash. No signature check here: boot core
//! images are verified before they are ever selected.
//!
//! Nothing to start: the board's boot LED (`[boot] led`) blinks five times
//! faster than the board's halt pattern, so a stage0 halt is told from the
//! boot core's; with the pattern off it stays dark. The `diag` build lights
//! it at entry; a panic blinks the code of the last step
//! (1 entry, 2 reading the log, 3 checking the boot core). A failed check
//! blinks five groups, then a long pause: 4 (5 if the log named no slot);
//! which test failed (`image::Fail`, 7 if SHA-256 itself is wrong); then
//! what the boot core log, the boot core header and stage0's own vector
//! table read as (1 erased, 2 zero, 3 as expected, 4 other, 5 unreadable).
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use dagon_format::header::image_kind;
use dagon_format::statelog::slot;
use dagon_mcu_rp2040::arm::{jump, led, step, xip_flash};
use dagon_mcu_rp2040::layout::{BL_LOG, BL_SLOT, BL_SLOT_SIZE, RAM, RAM_LEN, STAGE0};
use dagon_mcu_rp2040::{bl, image};
use dagon_port::Flash;
#[cfg(not(feature = "diag"))]
use panic_halt as _;

#[link_section = ".boot2"]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_GENERIC_03H;

/// Half period of stage0's halt blink: five times faster than the
/// board's.
const HALT_MS: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(ms / 2 / 5),
    None => None,
};
/// Initial stack pointer in stage0's vector table (top of RAM in memory.x).
const STACK_TOP: u32 = RAM + RAM_LEN;

/// The board's `[boot]` values (build.rs).
#[allow(dead_code)]
mod board_def {
    include!(concat!(env!("OUT_DIR"), "/board.rs"));
}

/// Takes the board's boot LED, if it has one (output, on).
fn led_init() {
    if let Some(gpio) = board_def::BOOT_LED_GPIO {
        led::init(gpio, board_def::BOOT_LED_ACTIVE_HIGH);
    }
}

/// What the first 4 bytes at `addr` read as: 1 erased (FF), 2 zero, 3 the
/// `expected` bytes, 4 something else, 5 unreadable.
fn probe(flash: &mut impl Flash, addr: u32, expected: &[u8; 4]) -> u8 {
    let mut b = [0u8; 4];
    if flash.read(addr, &mut b).is_err() {
        return 5;
    }
    match b {
        [0xFF, 0xFF, 0xFF, 0xFF] => 1,
        [0, 0, 0, 0] => 2,
        _ if &b == expected => 3,
        _ => 4,
    }
}

#[entry]
fn main() -> ! {
    step(1);
    if cfg!(feature = "diag") {
        led_init();
    }
    let mut flash = xip_flash();
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
            led_init();
            if cfg!(feature = "diag") {
                let k = if image::sha_ok() { why as u8 } else { 7 };
                led::codes(&[
                    if named.is_some() { 4 } else { 5 },
                    k,
                    probe(&mut flash, BL_LOG[0], b"DGSL"),
                    probe(&mut flash, base, b"DGIM"),
                    probe(&mut flash, STAGE0 + 0x100, &STACK_TOP.to_le_bytes()),
                ])
            }
            match HALT_MS {
                Some(ms) => led::halt(ms, led::ROSC_MS),
                None => {
                    led::set(false);
                    loop {
                        cortex_m::asm::wfi();
                    }
                }
            }
        }
    }
}
