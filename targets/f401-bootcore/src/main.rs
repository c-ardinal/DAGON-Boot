//! NUCLEO-F401RE boot core: reads the state log, picks the APP slot (trial
//! of the pending slot, rollback after the attempts run out, otherwise the
//! confirmed slot), starts the watchdog and jumps. With no startable slot it
//! blinks LD2 fast and stays here.
#![no_std]
#![no_main]

use core::panic::PanicInfo;

use cortex_m::peripheral::{NVIC, SCB};
use cortex_m_rt::entry;
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_port::BootSelectS;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;
use f401_board as board;

/// Verifying key, embedded at build time: 64 hex digits in `DAGON_PUBKEY`
/// (the public key printed by `dagon key gen`). Unset, it is the RFC 8032
/// section 7.1 TEST 1 public key, which only accepts TEST_KEY images.
const PUBKEY: [u8; 32] = hex32(match option_env!("DAGON_PUBKEY") {
    Some(k) => k,
    None => "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
});

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: board::MCU_ID,
    target_tag: board::TARGET_TAG,
    max_attempts: board::MAX_ATTEMPTS,
};

/// Half period of the "no image" blink at the 16 MHz reset clock (about 50 ms).
const HALT_BLINK_CYCLES: u32 = 800_000;

const fn hex32(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("DAGON_PUBKEY: not a hex digit"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() == 64, "DAGON_PUBKEY: need 64 hex digits");
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(Default::default());
    let mut flash = board::BoardFlash::new(p.FLASH);
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
