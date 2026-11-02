//! NUCLEO-F401RE boot core: reads the state log, picks the APP slot (trial
//! of the pending slot, rollback after the attempts run out, otherwise the
//! confirmed slot), starts the watchdog and jumps. With no startable slot it
//! blinks LD2 fast and stays here.
//!
//! Images are verified with `dagon_mcu_stm32f401re::PUBKEY` (`DAGON_PUBKEY`
//! at build time; the public test key only with `DAGON_ALLOW_TEST_KEY=1`).
//! The halt LED is PA5 (LD2 on NUCLEO boards). It accepts only images
//! tagged with its node's tag: `DAGON_TARGET_TAG` at build time (the build
//! fails without it).
#![no_std]
#![no_main]

use core::panic::PanicInfo;

use cortex_m::peripheral::{NVIC, SCB};
use cortex_m_rt::entry;
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_mcu_stm32f401re::flash::F401Flash;
use dagon_mcu_stm32f401re::layout as board;
use dagon_mcu_stm32f401re::tag::checked_tag;
use dagon_mcu_stm32f401re::{clocks, PUBKEY, SYSCLK_HZ, TARGET_TAG};
use dagon_port::BootSelectS;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: board::MCU_ID,
    target_tag: checked_tag(TARGET_TAG),
    max_attempts: board::MAX_ATTEMPTS,
};

/// Half period of the "no image" blink (about 50 ms).
const HALT_BLINK_CYCLES: u32 = SYSCLK_HZ / 20;

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(clocks());
    let mut flash = F401Flash::new(p.FLASH);
    let decision = match Log::load(&mut flash, &board::LOG) {
        Ok(mut log) => decide(&mut flash, &mut log, &board::SLOTS, &CONFIG),
        Err(_) => Decision::NoImage,
    };
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
            let mut led = Output::new(p.PA5, Level::Low, Speed::Low);
            loop {
                led.toggle();
                cortex_m::asm::delay(HALT_BLINK_CYCLES);
            }
        }
    }
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
