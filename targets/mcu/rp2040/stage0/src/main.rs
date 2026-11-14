//! Hub stage0 (never updated): picks the boot core slot from the boot core
//! log and the trial cell (`dagon_mcu_rp2040::bl::choose`, the shared
//! `dagon_core::bootcore::decide`): the pending core on trial while its
//! starts last, else the confirmed one (slot A while the log is empty),
//! else any other slot's core. Every core it may start passes the shared
//! check: its header's signature under the key built into stage0's flash
//! (the MCU definition's `[stage0] key`), the log record, and its body's
//! SHA-256. It leaves the trial cell for the next reset, starts the
//! watchdog before a trial core and jumps. It never writes flash. Holds
//! boot2 for the whole flash.
//!
//! Nothing to start: it tells the reason's code to the board's diagnostic
//! output (`BoardDiag`, generated from the board's `[boot]` LED and halt
//! pattern; none without one) and waits for recovery (BOOTSEL and the
//! install UF2).
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use dagon_ed25519::Ed25519Small;
use dagon_format::stage0;
use dagon_mcu_rp2040::arm::{jump, trial, xip_flash};
use dagon_mcu_rp2040::{bl, PUBKEY};
use panic_halt as _;

#[link_section = ".boot2"]
#[used]
static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_GENERIC_03H;

/// The version mark the agent's VERSION reads (`dagon_format::stage0`):
/// this package's Cargo.toml version.
static VERSION_MARK: stage0::Mark = stage0::Mark::of(stage0::version(env!("CARGO_PKG_VERSION")));

// Stage0 reads its key from its own flash only.
const _: () = assert!(matches!(
    dagon_mcu_rp2040::STAGE0_KEY_PLACE.as_bytes(),
    b"flash"
));

/// Stage0's signature verifier, under the key built into its flash.
const VERIFIER: Ed25519Small = Ed25519Small::new(PUBKEY);

/// The board's diagnostic output (build.rs).
mod diag {
    include!(concat!(env!("OUT_DIR"), "/diag.rs"));
}

#[entry]
fn main() -> ! {
    // Keeps the mark in the image (nothing else reads it here).
    core::hint::black_box(&VERSION_MARK);
    let mut flash = xip_flash();
    let mut out = diag::BoardDiag::default();
    match bl::choose(&mut flash, trial::read(), &VERIFIER, &mut out) {
        Ok(start) => {
            trial::write(start.cell);
            if start.trial {
                trial::arm_watchdog(dagon_mcu_rp2040::TRIAL_WDT_MS);
            }
            jump(start.body)
        }
        // Reported: wait for recovery.
        Err(_) => loop {
            cortex_m::asm::wfi();
        },
    }
}
