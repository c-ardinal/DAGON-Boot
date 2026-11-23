//! ESP32-S3 stage0 (never updated): the image the ROM loads from flash 0x0
//! in place of the ESP-IDF 2nd stage, so the image that boots has its
//! signature checked with the same small verifier and the same decision
//! as on the other MCUs (`dagon_core::bootcore::start_handed`).
//!
//! It holds the watchdogs, sets the ROM's flash driver up and decides from
//! the boot core log, the app log and the retained cell:
//!
//! - a whole START_APP request the boot core left: the app decision once
//!   (its BOOT_ATTEMPT and ROLLBACK records written here); the app it
//!   chooses starts under the trial or the confirmed watchdog period; when
//!   it chooses none, the cell takes the result and the confirmed boot
//!   core starts, which stays in recovery mode;
//! - otherwise the boot core: the pending one on trial while its starts
//!   last (the count stored in the cell, the trial watchdog armed), else
//!   the confirmed one; with none that passes, the app the log confirms,
//!   under the confirmed period, so it can write the boot core again.
//!
//! Every image passes the shared checks first: its detached header's
//! signature under the key built into stage0 (`DAGON_PUBKEY`), the log
//! record, and its body's SHA-256 on the SHA peripheral
//! (`flash::RomFlash::sha256`). Then the ESP image is planned before
//! anything is copied or mapped (`load`): a segment outside the ranges an
//! app may use, or overlapping stage0, is refused. With nothing to start
//! it waits for recovery through the ROM's download mode. It writes no
//! diagnostic output (the board gives none to stage0).
//!
//! The layout comes from `qemu_layout` until the ESP32-S3 definition gives
//! stage0's: without the `qemu-layout` feature it does not
//! build.
#![no_std]
#![no_main]

#[cfg(not(feature = "qemu-layout"))]
compile_error!(
    "the ESP32-S3 definition gives no stage0 layout yet: \
     build with the `qemu-layout` feature for the emulator test"
);

mod flash;
mod load;
mod qemu_layout;
mod rom;
mod wdt;

use dagon_core::boot::Place;
use dagon_core::bootcore::{refuse_app, start_handed, Apps, Handed, NoDiag, Refusal, Stage0};
use dagon_ed25519::Ed25519Small;
use dagon_mcu_esp32s3::app_image::Refused;
use dagon_mcu_esp32s3::{CONFIRMED_WDT_MS, MCU_WIRE_ID, PUBKEY, TRIAL_ATTEMPTS, TRIAL_WDT_MS};
use flash::RomFlash;
use qemu_layout as layout;

/// Stage0's signature verifier, under the key built into its image.
const VERIFIER: Ed25519Small = Ed25519Small::new(PUBKEY);

/// The node's tag, as the agent takes it: `DAGON_TARGET_TAG` at build
/// time; none (no app starts) without it.
const TAG: [u8; 16] = match dagon_mcu_esp32s3::TARGET_TAG_OVERRIDE {
    Some(t) => dagon_format::header::target_tag(t),
    None => dagon_core::bootcore::NO_TAG,
};

static STAGE0: Stage0<'static> = Stage0 {
    log: &layout::CORE_LOG,
    slots: &layout::CORE_PLACES,
    max: TRIAL_ATTEMPTS,
    mcu_id: MCU_WIRE_ID,
};

static APPS: Apps<'static> = Apps {
    log: &layout::APP_LOG,
    slots: &layout::APP_PLACES,
    target_tag: TAG,
    entry: at_body,
};

/// An ESP image's entry point is a RAM or mapped address, not a flash
/// one: the shared check sees the body itself, and `load::plan_at`
/// refuses an entry point outside the image's loaded code.
fn at_body(_: [u8; 8], body: u32) -> u32 {
    body
}

extern "C" {
    static mut _bss_start: u32;
    static mut _bss_end: u32;
}

fn read_cell() -> [u32; 2] {
    // SAFETY: retained RTC memory words, read as words.
    layout::CELL.map(|a| unsafe { core::ptr::read_volatile(a as *const u32) })
}

fn write_cell(i: usize, w: u32) {
    // SAFETY: as above; one word per store, as the hand-off expects.
    unsafe { core::ptr::write_volatile(layout::CELL[i] as *mut u32, w) }
}

/// The ROM's call of the image's entry point (a windowed C call on the
/// ROM's PRO CPU stack).
#[no_mangle]
#[link_section = ".text.stage0_start"]
pub extern "C" fn stage0_start() -> ! {
    // SAFETY: nothing has used .bss yet; zeroed a word at a time.
    unsafe {
        let mut p = &raw mut _bss_start;
        while p < &raw mut _bss_end {
            core::ptr::write_volatile(p, 0);
            p = p.add(1);
        }
    }
    wdt::hold();
    flash::attach();
    let mut f = RomFlash;
    let started = start_handed(
        &mut f,
        (&STAGE0, &APPS),
        (read_cell, write_cell),
        &VERIFIER,
        &mut NoDiag,
    );
    match started {
        Ok(Handed::Core(s)) => {
            s.cell
                .into_iter()
                .enumerate()
                .for_each(|(i, w)| write_cell(i, w));
            let period = s.trial.then_some(TRIAL_WDT_MS);
            let _ = run(&mut f, layout::CORE_PLACES[usize::from(s.slot)], period);
            // Signed, yet no ESP image stage0 may load. On trial the start
            // is counted: a reset ends the trial at the confirmed core.
            if s.trial {
                // SAFETY: the ROM's reset of the main system.
                unsafe { rom::software_reset() }
            }
            recovery()
        }
        Ok(Handed::App { slot, trial, .. }) => {
            let period = if trial {
                TRIAL_WDT_MS
            } else {
                CONFIRMED_WDT_MS
            };
            let at = layout::APP_PLACES[usize::from(slot)];
            let _ = run(&mut f, at, Some(period));
            // Signed, yet no ESP image stage0 may load: refused as the
            // decision refuses, so the boot core stays in recovery mode.
            match refuse_app(
                &mut f,
                &STAGE0,
                Refusal::Rejected,
                write_cell,
                &VERIFIER,
                &mut NoDiag,
            ) {
                Ok(s) => {
                    let _ = run(&mut f, layout::CORE_PLACES[usize::from(s.slot)], None);
                    recovery()
                }
                Err(_) => recovery(),
            }
        }
        Err(_) => recovery(),
    }
}

/// Starts the ESP image at `at` under the watchdog period `period` (none:
/// as the confirmed boot core runs); returns only when it is no image
/// stage0 may load, having changed nothing.
fn run(f: &mut RomFlash, at: Place, period: Option<u32>) -> Refused {
    let p = match load::plan_at(f, at) {
        Ok(p) => p,
        Err(why) => return why,
    };
    if let Some(ms) = period {
        wdt::arm(ms, layout::SLOW_HZ);
    }
    match load::start(f, &p) {
        Ok(never) => match never {},
        Err(why) => why,
    }
}

/// Nothing to start: wait for the ROM's download mode (a reset with the
/// boot pin held).
fn recovery() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    recovery()
}
