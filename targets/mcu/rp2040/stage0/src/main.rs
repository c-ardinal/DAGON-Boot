//! Coordinator stage0 (never updated): picks the boot core slot from the boot core
//! log and the trial cell (`dagon_mcu_rp2040::bl::choose`, the shared
//! `dagon_core::bootcore::decide`): the pending core on trial while its
//! starts last, else the confirmed one; never a core no record names.
//! Every core it may start passes the shared
//! check: its header's signature under the key built into stage0's flash
//! (the MCU definition's `[stage0] key`), the log record, and its body's
//! SHA-256. It leaves the trial cell for the next reset, starts the
//! watchdog before a trial core and jumps. It never writes flash. Holds
//! boot2 for the whole flash.
//!
//! No core to start (one boot core slot, its new core cut or failed): it
//! starts the app the app log confirms after the same checks and more
//! (`dagon_core::bootcore::start`), under the confirmed watchdog period,
//! and leaves the trial cell alone; that app writes the core again.
//!
//! It checks on the clock the definitions choose for the node (`clock`:
//! the MCU's `[[stage0.clock]]` setup for its board's `[clock]`) and puts
//! the clocks back as the chip starts them before it starts a core.
//!
//! Neither to start: it tells the reason's code to the board's diagnostic
//! output (`BoardDiag`, generated from the board's `[boot]` LED and halt
//! pattern; none without one) and waits for recovery (BOOTSEL and the
//! install UF2).
//!
//! With the `verify-time` feature it is built to measure: it times each
//! signature check, shows the longest in tenths of a second on the
//! diagnostic output (`dagon_mcu_common::verify_time`, which a debug probe
//! reads in cycles) and starts no core.
#![no_std]
#![no_main]
// The build to measure starts no core: the code that does is unused there.
#![cfg_attr(feature = "verify-time", allow(dead_code, unused_imports))]

mod clock;

use cortex_m_rt::entry;
use dagon_core::bootcore::Boot;
use dagon_ed25519::Ed25519Small;
use dagon_format::stage0;
use dagon_mcu_rp2040::arm::{jump, led, trial, xip_flash};
use dagon_mcu_rp2040::{bl, PUBKEY};
use panic_halt as _;

#[link_section = ".boot2"]
#[used]
static BOOT2: [u8; dagon_mcu_rp2040::STAGE0_HEAD_LEN as usize] =
    rp2040_boot2::BOOT_LOADER_GENERIC_03H;

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

/// Stage0's diagnostic output on the board's `[boot] led` (the generated
/// `BoardDiag` calls it), timed on the clock stage0 runs on.
#[allow(dead_code)]
fn report(gpio: u8, active_high: bool, period_ms: u32, code: u8) -> ! {
    led::report(gpio, active_high, period_ms, code, clock::CYCLES_PER_MS)
}

#[entry]
fn main() -> ! {
    // Keeps the mark in the image (nothing else reads it here).
    core::hint::black_box(&VERSION_MARK);
    clock::start();
    let mut flash = xip_flash();
    let mut out = diag::BoardDiag::default();
    #[cfg(feature = "verify-time")]
    measure(&mut flash, &mut out);
    #[cfg(not(feature = "verify-time"))]
    start_core(&mut flash, &mut out);
}

/// Starts the core (or the app) stage0's choice gives, on the clocks as
/// the chip starts them; with neither, waits for recovery.
#[cfg(not(feature = "verify-time"))]
fn start_core(flash: &mut dagon_mcu_rp2040::mapped::Mapped, out: &mut diag::BoardDiag) -> ! {
    match bl::start(flash, trial::read(), &VERIFIER, out) {
        Ok(Boot::Core(start)) => {
            clock::hand_back();
            trial::write(start.cell);
            if start.trial {
                trial::arm_watchdog(dagon_mcu_rp2040::TRIAL_WDT_MS);
            }
            jump(start.body)
        }
        // The confirmed app, as the boot core starts it: on the confirmed
        // period; the trial cell stays as it is.
        Ok(Boot::App { body, .. }) => {
            clock::hand_back();
            trial::arm_watchdog(dagon_mcu_rp2040::CONFIRMED_WDT_MS);
            jump(body)
        }
        // Reported: wait for recovery.
        Err(_) => loop {
            cortex_m::asm::wfi();
        },
    }
}

/// Runs stage0's choice with every signature check timed, shows the
/// longest and stays.
#[cfg(feature = "verify-time")]
fn measure(flash: &mut dagon_mcu_rp2040::mapped::Mapped, out: &mut diag::BoardDiag) -> ! {
    use dagon_core::bootcore::{Diag, NoDiag};
    use dagon_mcu_common::verify_time::{self, systick, Timed};
    systick::start();
    let timed = Timed {
        verifier: &VERIFIER,
        cycles: systick::cycles,
    };
    let _ = bl::choose(flash, trial::read(), &timed, &mut NoDiag);
    let longest = verify_time::DAGON_VERIFY_CYCLES.load(core::sync::atomic::Ordering::Relaxed);
    out.report(verify_time::tenths_of_a_second(longest, clock::OUT_HZ));
    loop {
        cortex_m::asm::wfi();
    }
}
