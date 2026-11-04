//! Hub boot core: dagon-core's kind S decision over the hub layout. A
//! pending app slot is tried (signature + SHA-256, BOOT_ATTEMPT before the
//! jump) until the attempts run out, then rolled back; otherwise the
//! confirmed slot starts after a SHA-256 check. dagon-core also requires
//! each header's `load_addr` to be its slot body (the hub's `Flash` speaks
//! XIP addresses). Every start arms the watchdog for 8 s; the app keeps
//! feeding it. With nothing startable the watchdog stops and the board's
//! boot LED shows its halt pattern (`[boot] led`, `halt`; reinstall with
//! BOOTSEL + UF2). In the `diag` build a
//! panic blinks the code of the last step (6 before HAL init, 7 after it,
//! 8 deciding) and nothing startable blinks code 9.
//!
//! It accepts only images tagged with its node's tag: `DAGON_TARGET_TAG`
//! at build time (the build fails without it).
#![no_std]
#![no_main]

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_format::header::mcu_id;
use dagon_mcu_rp2040::arm::{jump, led, step};
use dagon_mcu_rp2040::flash::RpFlash;
use dagon_mcu_rp2040::layout::{APP_LOG_LAYOUT, APP_SLOTS, MAX_ATTEMPTS, WDT_MS};
use dagon_mcu_rp2040::tag::checked_tag;
use dagon_mcu_rp2040::{PUBKEY, TARGET_TAG};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
#[cfg(not(feature = "diag"))]
use panic_halt as _;

/// Half period of the halt blink (the board's `[boot] halt`).
const HALT_MS: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(ms / 2),
    None => None,
};

/// The board's `[boot]` values (build.rs).
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

const CONFIG: BootConfig = BootConfig {
    pubkey: PUBKEY,
    mcu_id: mcu_id::RP2040,
    target_tag: checked_tag(TARGET_TAG),
    max_attempts: MAX_ATTEMPTS,
};

#[cortex_m_rt::entry]
fn main() -> ! {
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
    let decision = match Log::load(&mut flash, &APP_LOG_LAYOUT) {
        Ok(mut log) => boot::decide(&mut flash, &mut log, &APP_SLOTS, &CONFIG),
        Err(_) => Decision::NoImage,
    };
    if let Decision::Boot { entry, .. } = decision {
        wdt.feed(Duration::from_millis(WDT_MS as u64));
        hand_back_clocks();
        jump(entry);
    }
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
