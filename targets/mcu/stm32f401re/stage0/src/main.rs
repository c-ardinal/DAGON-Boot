//! STM32F401RE stage0 (never updated; all of sector 0, at the reset
//! vector): picks the boot core slot from the boot core records of the
//! state log and the trial cell (`dagon_core::bootcore::choose`): the
//! pending core on trial while its starts last, else the confirmed one
//! (slot A while the log names none). It checks the core's header against
//! the record and its body's SHA-256 (`check_inline`), leaves the trial
//! cell for the next reset, starts the independent watchdog before a core
//! on trial and jumps. It never writes flash and has no signature check: a
//! core is recorded only after the agent verified its signature, and stage0
//! starts only the bytes that record names. With nothing to start it stays
//! here (recover through the ROM loader).
#![no_std]
#![no_main]

use cortex_m::peripheral::{NVIC, SCB};
use cortex_m_rt::entry;
use dagon_core::bootcore;
use dagon_mcu_common::mapped::Mapped;
use dagon_mcu_stm32f401re::layout::{
    CORE_SLOTS, LOG, MCU_ID, MIN_FLASH_SIZE, STAGE0, TRIAL_WDT_MS,
};
use dagon_mcu_stm32f401re::{trial, TRIAL_ATTEMPTS};
use panic_halt as _;

#[entry]
fn main() -> ! {
    // SAFETY: the flash is memory-mapped read-only from its base for
    // MIN_FLASH_SIZE bytes, and nothing writes it while stage0 runs.
    let mut flash = unsafe { Mapped::new(STAGE0 as *const u8, STAGE0, MIN_FLASH_SIZE) };
    let st = bootcore::read_state(&mut flash, &LOG).unwrap_or_default();
    let chosen = bootcore::choose(&st, trial::read(), TRIAL_ATTEMPTS, |r| {
        let s = r.map_or(0, |r| r.slot);
        let slot = *CORE_SLOTS
            .get(usize::from(s))
            .ok_or(bootcore::CoreFail::Kind)?;
        bootcore::check_inline(&mut flash, slot, s, r, MCU_ID)
    });
    match chosen {
        Ok(start) => {
            trial::write(start.cell);
            if start.trial {
                trial::arm_watchdog(TRIAL_WDT_MS);
            }
            jump(start.body)
        }
        Err(_) => loop {
            cortex_m::asm::wfi();
        },
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
