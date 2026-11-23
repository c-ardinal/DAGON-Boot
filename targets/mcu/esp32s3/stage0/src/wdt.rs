//! The watchdogs: held while stage0 checks (the ROM leaves the RTC and
//! main system watchdogs running for its own load), and the RTC watchdog
//! armed for the image stage0 starts. Its stage 0 resets the main system
//! and keeps the RTC domain, where the retained cell lives, so a trial
//! image that is not confirmed in time comes back to stage0 with its start
//! counted. Registers as esp-hal 1.2's `Rwdt` and `Swd` (ESP32-S3 PAC
//! 0.36).

use crate::rom::{reg_read, reg_write};

const RTC_CNTL: u32 = 0x6000_8000;
const WDTCONFIG0: u32 = RTC_CNTL + 0x98;
const WDTCONFIG1: u32 = RTC_CNTL + 0x9C;
const WDTFEED: u32 = RTC_CNTL + 0xAC;
const WDTWPROTECT: u32 = RTC_CNTL + 0xB0;
const SWD_CONF: u32 = RTC_CNTL + 0xB4;
const SWD_WPROTECT: u32 = RTC_CNTL + 0xB8;
const TIMG0: u32 = 0x6001_F000;
const TIMG0_WDTCONFIG0: u32 = TIMG0 + 0x48;
const TIMG0_WDTWPROTECT: u32 = TIMG0 + 0x64;
/// EFUSE_RD_REPEAT_DATA1: WDT_DELAY_SEL in bits 16 and 17.
const EFUSE_RD_REPEAT_DATA1: u32 = 0x6000_7034;

const WDT_KEY: u32 = 0x50D8_3AA1;
const SWD_KEY: u32 = 0x8F1D_312A;
const SWD_AUTO_FEED_EN: u32 = 1 << 31;
const WDT_EN: u32 = 1 << 31;
const WDT_FEED: u32 = 1 << 31;
/// Stage 0's action: reset the main system, the RTC domain kept
/// (esp-hal `RwdtStageAction::ResetCore`).
const RESET_CORE: u32 = 3;
const STG0_SHIFT: u32 = 28;
const CPU_RESET_LENGTH_SHIFT: u32 = 16;
const SYS_RESET_LENGTH_SHIFT: u32 = 13;
/// The reset pulse lengths esp-hal sets.
const RESET_LENGTH: u32 = 7;

/// Stops the main system watchdog and the RTC watchdog, and lets the super
/// watchdog feed itself, while stage0 checks.
pub fn hold() {
    reg_write(TIMG0_WDTWPROTECT, WDT_KEY);
    reg_write(TIMG0_WDTCONFIG0, 0);
    reg_write(TIMG0_WDTWPROTECT, 0);
    reg_write(WDTWPROTECT, WDT_KEY);
    reg_write(WDTCONFIG0, 0);
    reg_write(WDTWPROTECT, 0);
    reg_write(SWD_WPROTECT, SWD_KEY);
    reg_write(SWD_CONF, reg_read(SWD_CONF) | SWD_AUTO_FEED_EN);
    reg_write(SWD_WPROTECT, 0);
}

/// Arms the RTC watchdog to reset the main system after `period_ms`
/// without a feed, on an RTC slow clock of `slow_hz`.
pub fn arm(period_ms: u32, slow_hz: u32) {
    let cal = ((1_000_000u64 << dagon_mcu_esp32s3::rwdt::CAL_FRACT) / u64::from(slow_hz)) as u32;
    let multiplier = ((reg_read(EFUSE_RD_REPEAT_DATA1) >> 16) & 3) as u8;
    let hold = dagon_mcu_esp32s3::rwdt::stage0_hold(period_ms, cal, multiplier);
    reg_write(WDTWPROTECT, WDT_KEY);
    reg_write(WDTCONFIG0, 0);
    reg_write(WDTCONFIG1, hold);
    reg_write(
        WDTCONFIG0,
        WDT_EN
            | RESET_CORE << STG0_SHIFT
            | RESET_LENGTH << CPU_RESET_LENGTH_SHIFT
            | RESET_LENGTH << SYS_RESET_LENGTH_SHIFT,
    );
    reg_write(WDTFEED, WDT_FEED);
    reg_write(WDTWPROTECT, 0);
}
