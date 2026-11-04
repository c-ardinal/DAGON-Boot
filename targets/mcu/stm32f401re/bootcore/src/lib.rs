//! STM32F401RE boot core: arms the watchdog with the trial period, reads
//! the state log, picks the APP slot (trial of the pending slot, rollback
//! after the attempts run out, otherwise the confirmed slot), sets the
//! watchdog to the confirmed period when the slot is not on trial, and
//! jumps. With no startable slot it shows the board's halt pattern on its
//! boot LED and stays here.
//!
//! Images are verified with `dagon_mcu_stm32f401re::PUBKEY` (`DAGON_PUBKEY`
//! at build time; the public test key only with `DAGON_ALLOW_TEST_KEY=1`).
//!
//! A node's boot core is the boot crate `dagon gen` writes into
//! `boot/<id>/` ([`boot!`]): it runs the extensions its
//! `boot_wiring.rs` lists (each declared trusted in dagon.toml) on every
//! boot event, each call timed against its `max_run_us`, and takes the
//! board's `[boot] led` and `halt` and the node's tag from that folder
//! (`DAGON_BOOT_CRATE`, `DAGON_TARGET_TAG`); the build fails without
//! them. It accepts only images tagged with its node's tag.
#![no_std]

use core::panic::PanicInfo;

use cortex_m::peripheral::{DWT, NVIC, SCB};
use dagon_boot::BootRun;
use dagon_core::boot::{BootConfig, Decision};
use dagon_events::{compose_boot, Clock, Extension};
use dagon_mcu_stm32f401re::flash::F401Flash;
use dagon_mcu_stm32f401re::layout as board;
use dagon_mcu_stm32f401re::tag::checked_tag;
use dagon_mcu_stm32f401re::{clocks, PUBKEY, SYSCLK_HZ, TARGET_TAG};
use dagon_port::BootSelectS;
#[allow(unused_imports)]
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;

#[doc(hidden)]
pub use cortex_m_rt::entry as __entry;

/// The boot core's `main` for the node whose generated `boot_wiring.rs`
/// is the module `$wiring`: its extension chain and their time limits.
#[macro_export]
macro_rules! boot {
    ($wiring:ident) => {
        #[$crate::__entry]
        fn main() -> ! {
            $crate::run($wiring::BOOT_CHAIN, $wiring::BOOT_BUDGETS_US)
        }
    };
}

/// The board's `[boot]` values (build.rs, from the node's boot folder).
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
#[allow(dead_code)]
const HALT_BLINK_CYCLES: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(SYSCLK_HZ / 1000 * (ms / 2)),
    None => None,
};

/// CPU cycles per microsecond.
const CYCLES_PER_US: u32 = SYSCLK_HZ / 1_000_000;

/// Microseconds from the cycle counter (DWT CYCCNT at `SYSCLK_HZ`), read
/// at least once per 32-bit wrap (51 s at 84 MHz). Each read converts the
/// cycles since the last one with a 32-bit division (one instruction here;
/// a 64-bit one would link a software divider into the boot core).
struct Cycles {
    last: u32,
    rem: u32,
    us: u64,
}

impl Clock for Cycles {
    fn now_us(&mut self) -> u64 {
        let now = DWT::cycle_count();
        let d = now.wrapping_sub(self.last).saturating_add(self.rem);
        self.last = now;
        self.us += u64::from(d / CYCLES_PER_US);
        self.rem = d % CYCLES_PER_US;
        self.us
    }
}

/// One boot with `chain` (the node's boot extensions, in order; their
/// time limits per call in `budgets_us`).
pub fn run<E: Extension>(chain: E, budgets_us: &'static [u32]) -> ! {
    let p = embassy_stm32::init(clocks());
    // Before the log is read and before any extension runs.
    IndependentWatchdog::new(p.IWDG, board::TRIAL_WDT_MS * 1000).unleash();
    // SAFETY: the boot core takes the core peripherals nowhere else; the
    // cycle counter is enabled once here and only read afterwards.
    let mut cp = unsafe { cortex_m::Peripherals::steal() };
    cp.DCB.enable_trace();
    cp.DWT.enable_cycle_counter();
    let clock = Cycles {
        last: DWT::cycle_count(),
        rem: 0,
        us: 0,
    };
    let mut flash = F401Flash::new(p.FLASH);
    let mut chain = compose_boot(chain, clock, budgets_us);
    let mut boot = BootRun::start(
        &mut chain,
        &mut flash,
        &board::LOG,
        &board::SLOTS,
        &CONFIG,
        board::TRIAL_WDT_MS,
    );
    if let Decision::Boot { entry, trial, .. } = boot.decision() {
        if !trial {
            // SAFETY: the IWDG was handed to the watchdog above and no
            // handle of it is left; this sets the confirmed period on the
            // running watchdog (new PR/RLR) and reloads it.
            let iwdg = unsafe { embassy_stm32::peripherals::IWDG::steal() };
            IndependentWatchdog::new(iwdg, board::CONFIRMED_WDT_MS * 1000).pet();
        }
        boot.before_jump(&mut chain);
        Jump.boot(entry)
    }
    boot.halt(&mut chain);
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
