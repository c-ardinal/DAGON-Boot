//! NUCLEO-F401RE boot core: reads the state log, picks the APP slot (trial
//! of the pending slot, rollback after the attempts run out, otherwise the
//! confirmed slot), starts the watchdog and jumps. With no startable slot it
//! shows the board's halt pattern on its boot LED and stays here.
//!
//! Images are verified with `dagon_mcu_stm32f401re::PUBKEY` (`DAGON_PUBKEY`
//! at build time; the public test key only with `DAGON_ALLOW_TEST_KEY=1`).
//! The halt LED and pattern are the board definition's `[boot] led` and
//! `halt` (`DAGON_BOARD` names the board, else the MCU's default board). It
//! accepts only images
//! tagged with its node's tag: `DAGON_TARGET_TAG` at build time (the build
//! fails without it).
//!
//! Boot core A/B (`dagon_core::bootcore::settle_log` over the shared state
//! log): started by stage0 as the pending core, it confirms itself only
//! after its own check (an app is startable and that app's header verifies
//! under this core's key); otherwise it resets, so stage0 counts the start
//! and falls back once the starts run out. Started while another core is
//! pending, it records the rollback. Built for boot core slot A, or B with
//! `slot-b`.
#![no_std]
#![no_main]

use core::panic::PanicInfo;

use cortex_m::peripheral::{NVIC, SCB};
use cortex_m_rt::entry;
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::bootcore::{settle_log, MustReset, Settled, CLEARED};
use dagon_format::header::{verify_header, HEADER_SIZE};
use dagon_mcu_stm32f401re::{trial, LINKED_SLOT, TRIAL_ATTEMPTS};
use dagon_port::Flash;

use dagon_core::log::Log;
use dagon_mcu_stm32f401re::flash::F401Flash;
use dagon_mcu_stm32f401re::layout as board;
use dagon_mcu_stm32f401re::tag::checked_tag;
use dagon_mcu_stm32f401re::{clocks, PUBKEY, SYSCLK_HZ, TARGET_TAG};
use dagon_port::BootSelectS;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;

/// The board's `[boot]` values (build.rs).
#[allow(dead_code)]
mod board_def {
    include!(concat!(env!("OUT_DIR"), "/board.rs"));
}
#[allow(dead_code, unused_macros)]
#[macro_use]
mod pins {
    include!(concat!(env!("OUT_DIR"), "/pins.rs"));
}

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: board::MCU_ID,
    target_tag: checked_tag(TARGET_TAG),
    max_attempts: dagon_mcu_stm32f401re::TRIAL_ATTEMPTS,
};

/// Half period of the halt blink in CPU cycles; `None`: the LED stays off.
const HALT_BLINK_CYCLES: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(SYSCLK_HZ / 1000 * (ms / 2)),
    None => None,
};

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(clocks());
    let mut flash = F401Flash::new(p.FLASH);
    let decision = match Log::load(&mut flash, &board::LOG) {
        Ok(mut log) => decide(&mut flash, &mut log, &board::SLOTS, &CONFIG),
        Err(_) => Decision::NoImage,
    };
    let healthy = match decision {
        Decision::Boot { slot, .. } => app_verifies(&mut flash, slot),
        Decision::NoImage => false,
    };
    let settled = settle_log(
        &mut flash,
        &board::LOG,
        LINKED_SLOT,
        (trial::read(), TRIAL_ATTEMPTS),
        healthy,
    );
    match settled {
        Ok(Settled::Nothing) => {}
        Ok(Settled::Confirmed | Settled::RolledBack) => trial::write(CLEARED),
        // On trial and not fit to run the app: stage0 counts this start
        // (a system reset keeps the trial cell).
        Err(MustReset) => SCB::sys_reset(),
    }
    match decision {
        Decision::Boot { entry, trial, .. } => {
            let ms = if trial {
                board::TRIAL_WDT_MS
            } else {
                board::CONFIRMED_WDT_MS
            };
            IndependentWatchdog::new(p.IWDG, ms * 1000).unleash();
            Jump.boot(entry)
        }
        Decision::NoImage => {
            #[cfg(boot_led)]
            {
                let off = if board_def::BOOT_LED_ACTIVE_HIGH {
                    Level::Low
                } else {
                    Level::High
                };
                let mut led = Output::new(take!(p.boot_led), off, Speed::Low);
                if let Some(cycles) = HALT_BLINK_CYCLES {
                    loop {
                        led.toggle();
                        cortex_m::asm::delay(cycles);
                    }
                }
            }
            loop {
                cortex_m::asm::wfi();
            }
        }
    }
}

/// The header of the app in APP slot `slot` verifies under this core's
/// key: the part of a new core's own check that proves its verifier and
/// key accept the project's images.
fn app_verifies(flash: &mut F401Flash, slot: u8) -> bool {
    let mut raw = [0u8; HEADER_SIZE];
    board::SLOTS
        .slots
        .get(usize::from(slot))
        .is_some_and(|&base| flash.read(base, &mut raw).is_ok())
        && verify_header(&raw, &PUBKEY).is_ok()
}

struct Jump;

impl BootSelectS for Jump {
    fn boot(&mut self, entry: u32) -> ! {
        // SAFETY: the boot core enables no interrupts, so clearing NVIC
        // enables and pending bits leaves nothing to fire mid-handover.
        // `entry` is the body of a slot whose SHA-256 (and, on trial, header
        // signature) was just verified, so it starts with a vector table
        // (initial MSP, reset vector) at a 512-byte aligned address, as VTOR
        // requires on this part.
        unsafe {
            let nvic = &*NVIC::PTR;
            for i in 0..8 {
                nvic.icer[i].write(u32::MAX);
                nvic.icpr[i].write(u32::MAX);
            }
            (*SCB::PTR).vtor.write(entry);
            cortex_m::asm::bootload(entry as *const u32)
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::nop();
    }
}
