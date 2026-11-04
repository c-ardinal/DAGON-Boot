//! Hub stage0 (never updated): picks the boot core slot from the boot core
//! log and the trial cell (`dagon_mcu_rp2040::bl::choose`): the pending core
//! on trial while its starts last, else the confirmed one (slot A while the
//! log is empty). It checks the core's header against the log record and
//! its body's SHA-256, leaves the trial cell for the next reset, starts the
//! watchdog before a trial core and jumps. It never writes flash and has no
//! signature check: a core is recorded only after the agent verified its
//! signature, and stage0 starts only the bytes that record names. Holds
//! boot2 for the whole flash.
//!
//! Nothing to start: the board's boot LED (`[boot] led`) blinks five times
//! faster than the board's halt pattern, so a stage0 halt is told from the
//! boot core's; with the pattern off it stays dark. The `diag` build lights
//! it at entry; a panic blinks the code of the last step
//! (1 entry, 2 reading the log, 3 checking the boot core). A failed check
//! blinks five groups, then a long pause: 4 (5 if the log named no slot);
//! which test failed (`image::Fail`, 7 if SHA-256 itself is wrong); then
//! what the boot core log, slot A's header and stage0's own vector
//! table read as (1 erased, 2 zero, 3 as expected, 4 other, 5 unreadable).
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use dagon_mcu_rp2040::arm::{jump, led, step, trial, xip_flash};
use dagon_mcu_rp2040::layout::{BL_LOG, BL_SLOT, RAM, RAM_LEN, STAGE0};
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
    let chosen = bl::choose(&mut flash, trial::read());
    step(3);
    match chosen {
        Ok(start) => {
            trial::write(start.cell);
            if start.trial {
                trial::arm_watchdog(dagon_mcu_rp2040::TRIAL_WDT_MS);
            }
            if cfg!(feature = "diag") {
                led::set(false);
            }
            jump(start.body)
        }
        // Nothing to start: recover with BOOTSEL and the install UF2.
        Err(no) => {
            led_init();
            if cfg!(feature = "diag") {
                let k = if image::sha_ok() { no.why as u8 } else { 7 };
                led::codes(&[
                    if no.named { 4 } else { 5 },
                    k,
                    probe(&mut flash, BL_LOG[0], b"DGSL"),
                    probe(&mut flash, BL_SLOT[0], b"DGIM"),
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
