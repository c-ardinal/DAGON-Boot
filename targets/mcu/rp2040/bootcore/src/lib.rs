//! Hub boot core: dagon-core's kind S decision over the hub layout. A
//! pending app slot is tried (signature + SHA-256, BOOT_ATTEMPT before the
//! jump) until the attempts run out, then rolled back; otherwise the
//! confirmed slot starts after a SHA-256 check. dagon-core also requires
//! each header's `load_addr` to be its slot body (the hub's `Flash` speaks
//! XIP addresses). Every start arms the watchdog for 8 s before the log is
//! read; the app keeps feeding it. With nothing startable the watchdog
//! stops and the board's boot LED shows its halt pattern (`[boot] led`,
//! `halt`; reinstall with BOOTSEL + UF2). In the `diag` build a panic
//! blinks the code of the last step (6 before HAL init, 7 after it, 8
//! deciding) and nothing startable blinks code 9.
//!
//! Boot core A/B (`dagon_mcu_rp2040::bl::settle_core`): before it starts
//! the app, the core settles the boot core trial. Started as the pending
//! core, it confirms itself (CONFIRMED in the boot core log) only after its
//! own check: an app is startable and that app's header verifies under
//! this core's key. Otherwise it resets through the watchdog, so stage0
//! counts the start and falls back once the starts run out. Started while
//! another core is pending, it records the rollback. Built for slot A, or
//! B with `slot-b`.
//!
//! A node's boot core is the boot crate `dagon gen` writes into
//! `boot/<id>/` ([`boot!`]): it runs the extensions its
//! `boot_wiring.rs` lists (each declared trusted in dagon.toml) on every
//! boot event, each call timed against its `max_run_us`, and takes the
//! board facts and the node's tag from that folder (`DAGON_BOOT_CRATE`,
//! `DAGON_TARGET_TAG`); the build fails without them.
#![no_std]

use dagon_boot::BootRun;
use dagon_core::boot::{BootConfig, Decision};
use dagon_core::bootcore::CLEARED;
use dagon_events::{compose_boot, BootExtension, Extension};
use dagon_format::header::{self, mcu_id, HEADER_SIZE};
use dagon_mcu_rp2040::arm::{jump, led, step, trial};
use dagon_mcu_rp2040::bl::{self, Settled};
use dagon_mcu_rp2040::flash::RpFlash;
use dagon_mcu_rp2040::layout::{APP_LOG_LAYOUT, APP_SLOTS, WDT_MS};
use dagon_mcu_rp2040::tag::checked_tag;
use dagon_mcu_rp2040::{LINKED_SLOT, PUBKEY, TARGET_TAG, TRIAL_ATTEMPTS};
use dagon_port::Flash;
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
#[cfg(not(feature = "diag"))]
use panic_halt as _;

#[doc(hidden)]
pub use cortex_m_rt::entry as __entry;

/// The boot core's `main` for the node whose generated `boot_wiring.rs`
/// is the module `$wiring`: its extension chain and their time limits.
#[macro_export]
macro_rules! boot {
    ($wiring:ident) => {
        #[$crate::__entry]
        fn main() -> ! {
            $crate::run($wiring::BOOT_CHAIN, $wiring::BOOT_EXTENSIONS)
        }
    };
}

/// Half period of the halt blink (the board's `[boot] halt`).
const HALT_MS: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(ms / 2),
    None => None,
};

/// The board's `[boot]` values (build.rs, from the node's boot folder).
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

/// Puts the clocks back the way the chip starts: clk_sys and clk_ref on the
/// ring oscillator, the PLL-fed clocks off, both PLLs powered down. The app
/// then sets its clocks up from the same state as after a cold reset. The
/// watchdog keeps counting meanwhile (slower on the ring oscillator).
fn hand_back_clocks() {
    use embassy_rp::pac;
    use pac::clocks::vals::{ClkRefCtrlSrc, ClkSysCtrlSrc};
    let c = pac::CLOCKS;
    c.clk_sys_ctrl()
        .modify(|w| w.set_src(ClkSysCtrlSrc::CLK_REF));
    while c.clk_sys_selected().read() != 1 {}
    c.clk_ref_ctrl()
        .modify(|w| w.set_src(ClkRefCtrlSrc::ROSC_CLKSRC_PH));
    while c.clk_ref_selected().read() != 1 {}
    c.clk_peri_ctrl().modify(|w| w.set_enable(false));
    c.clk_usb_ctrl().modify(|w| w.set_enable(false));
    c.clk_adc_ctrl().modify(|w| w.set_enable(false));
    c.clk_rtc_ctrl().modify(|w| w.set_enable(false));
    for pll in [pac::PLL_SYS, pac::PLL_USB] {
        pll.pwr().write(|w| {
            w.set_pd(true);
            w.set_vcopd(true);
            w.set_postdivpd(true);
            w.set_dsmpd(true);
        });
    }
}

/// The header of the app in APP slot `slot` verifies under this core's
/// key: the part of a new core's own check that proves its verifier and
/// key accept the project's images.
fn app_verifies(flash: &mut RpFlash, slot: u8) -> bool {
    let mut raw = [0u8; HEADER_SIZE];
    APP_SLOTS
        .slots
        .get(usize::from(slot))
        .is_some_and(|&base| flash.read(base, &mut raw).is_ok())
        && header::verify_header(&raw, &PUBKEY).is_ok()
}

/// Microseconds since reset: the TIMER, which counts the watchdog's 1 MHz
/// tick that HAL init starts (RP2040 datasheet section 4.6). The high word is read
/// around the low one so a carry between the two reads is not lost.
fn now_us() -> u64 {
    let t = embassy_rp::pac::TIMER;
    loop {
        let hi = t.timerawh().read();
        let lo = t.timerawl().read();
        if t.timerawh().read() == hi {
            return (u64::from(hi) << 32) | u64::from(lo);
        }
    }
}

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: mcu_id::RP2040,
    target_tag: checked_tag(TARGET_TAG),
    max_attempts: TRIAL_ATTEMPTS,
};

/// One boot with `chain` (the node's boot extensions, in order; their
/// time limits and observed events in `limits`).
pub fn run<E: Extension>(chain: E, limits: &'static [BootExtension]) -> ! {
    // The diag panic handler blinks the selected LED.
    if let Some(gpio) = board_def::BOOT_LED_GPIO {
        led::select(gpio, board_def::BOOT_LED_ACTIVE_HIGH);
    }
    step(6);
    let p = embassy_rp::init(Default::default());
    step(7);
    let mut flash = RpFlash::new(p.FLASH);
    let mut wdt = Watchdog::new(p.WATCHDOG);
    wdt.start(Duration::from_millis(WDT_MS as u64));
    step(8);
    let mut chain = compose_boot(chain, now_us, limits);
    let mut boot = BootRun::start(
        &mut chain,
        &mut flash,
        &APP_LOG_LAYOUT,
        &APP_SLOTS,
        &CONFIG,
        WDT_MS,
    );
    let decision = boot.decision();
    let healthy = match decision {
        Decision::Boot { slot, .. } => app_verifies(&mut flash, slot),
        Decision::NoImage => false,
    };
    match bl::settle_core(
        &mut flash,
        LINKED_SLOT,
        (trial::read(), trial::holds()),
        healthy,
    ) {
        Ok(Settled::Nothing) => {}
        Ok(Settled::Confirmed | Settled::RolledBack) => trial::write(CLEARED),
        // On trial and not fit to run the app: stage0 counts this start.
        Err(bl::MustReset) => {
            wdt.trigger_reset();
            loop {
                cortex_m::asm::nop();
            }
        }
    }
    if let Decision::Boot { entry, .. } = decision {
        boot.before_jump(&mut chain);

        wdt.feed(Duration::from_millis(WDT_MS as u64));
        hand_back_clocks();
        jump(entry);
    }
    boot.halt(&mut chain);
    wdt.stop();
    led_init();
    if cfg!(feature = "diag") {
        led::code(9)
    }
    match HALT_MS {
        Some(ms) => led::halt(ms, led::PLL_MS),
        None => {
            led::set(false);
            loop {
                cortex_m::asm::wfi();
            }
        }
    }
}
