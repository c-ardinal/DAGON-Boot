//! STM32F401RE stage0 (never updated; all of sector 0, at the reset
//! vector): picks the boot core slot from the boot core records of the
//! state log and the trial cell (the shared
//! `dagon_core::bootcore::decide`): the pending core on trial while its
//! starts last, else the confirmed one (slot A while the log names none),
//! else any other slot's core. Every core it may start passes the shared
//! check: its header's signature under the key built into stage0's flash
//! (the MCU definition's `[stage0] key`), the record, and its body's
//! SHA-256. It leaves the trial cell for the next reset, starts the
//! independent watchdog before a core on trial and jumps. It never writes
//! flash. With nothing to start it tells the reason's code to the board's
//! diagnostic output (`BoardDiag`, generated from the board's `[boot]` LED
//! and halt pattern; none without one) and stays here (recover through the
//! ROM loader).
//!
//! It checks on the clock the definitions choose for the node (`clock`:
//! the MCU's `[[stage0.clock]]` setup for its board's `[clock]`) and puts
//! the clocks back as the chip starts them before it starts a core.
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
// Used by `BoardDiag` only when the node's board has a `[boot]` LED.
#[allow(dead_code)]
mod indicator;

use cortex_m::peripheral::{NVIC, SCB};
use cortex_m_rt::entry;
use dagon_core::bootcore::{self, Stage0};
use dagon_ed25519::Ed25519Small;
use dagon_format::stage0;
use dagon_mcu_common::mapped::Mapped;
use dagon_mcu_stm32f401re::layout::{CORE_SLOTS, LOG, MCU_ID, MIN_FLASH_SIZE, STAGE0};
use dagon_mcu_stm32f401re::{trial, PUBKEY, STAGE0_KEY_PLACE, TRIAL_ATTEMPTS, TRIAL_WDT_MS};
use panic_halt as _;

// Stage0 reads its key from its own flash only.
const _: () = assert!(matches!(STAGE0_KEY_PLACE.as_bytes(), b"flash"));

/// Stage0's signature verifier, under the key built into its flash.
const VERIFIER: Ed25519Small = Ed25519Small::new(PUBKEY);

/// What stage0 knows of this MCU.
const AT: Stage0<'static> = Stage0 {
    log: &LOG,
    slots: &CORE_SLOTS,
    max: TRIAL_ATTEMPTS,
    mcu_id: MCU_ID,
};

/// The version mark the agent's VERSION reads (`dagon_format::stage0`):
/// this package's Cargo.toml version.
static VERSION_MARK: stage0::Mark = stage0::Mark::of(stage0::version(env!("CARGO_PKG_VERSION")));

/// The board's diagnostic output (build.rs).
mod diag {
    include!(concat!(env!("OUT_DIR"), "/diag.rs"));
}

#[entry]
fn main() -> ! {
    // Keeps the mark in the image (nothing else reads it here).
    core::hint::black_box(&VERSION_MARK);
    clock::start();
    // SAFETY: the flash is memory-mapped read-only from its base for
    // MIN_FLASH_SIZE bytes, and nothing writes it while stage0 runs.
    let mut flash = unsafe { Mapped::new(STAGE0 as *const u8, STAGE0, MIN_FLASH_SIZE) };
    let mut out = diag::BoardDiag::default();
    #[cfg(feature = "verify-time")]
    measure(&mut flash, &mut out);
    #[cfg(not(feature = "verify-time"))]
    start_core(&mut flash, &mut out);
}

/// Starts the core stage0's choice gives, on the clocks as the chip starts
/// them; with none, stays.
#[cfg(not(feature = "verify-time"))]
fn start_core(flash: &mut Mapped, out: &mut diag::BoardDiag) -> ! {
    match bootcore::decide(flash, &AT, trial::read(), &VERIFIER, out) {
        Ok(start) => {
            clock::hand_back();
            trial::write(start.cell);
            if start.trial {
                trial::arm_watchdog(TRIAL_WDT_MS);
            }
            jump(start.body)
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
fn measure(flash: &mut Mapped, out: &mut diag::BoardDiag) -> ! {
    use dagon_core::bootcore::{Diag, NoDiag};
    use dagon_mcu_common::verify_time::{self, systick, Timed};
    systick::start();
    let timed = Timed {
        verifier: &VERIFIER,
        cycles: systick::cycles,
    };
    let _ = bootcore::decide(flash, &AT, trial::read(), &timed, &mut NoDiag);
    let longest = verify_time::DAGON_VERIFY_CYCLES.load(core::sync::atomic::Ordering::Relaxed);
    out.report(verify_time::tenths_of_a_second(longest, clock::OUT_HZ));
    loop {
        cortex_m::asm::wfi();
    }
}

/// Starts the image whose vector table is at `body`.
fn jump(body: u32) -> ! {
    // SAFETY: stage0 enables no interrupts, so clearing NVIC enables and
    // pending bits leaves nothing to fire mid-handover; `body` is a checked
    // core's body, linked for that 512-byte aligned address (VTOR).
    unsafe {
        let nvic = &*NVIC::PTR;
        for i in 0..8 {
            nvic.icer[i].write(u32::MAX);
            nvic.icpr[i].write(u32::MAX);
        }
        (*SCB::PTR).vtor.write(body);
        cortex_m::asm::bootload(body as *const u32)
    }
}
