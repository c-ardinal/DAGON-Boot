//! The boot core trial cell (`dagon_core::bootcore::trial_cell`) in the
//! RTC backup registers BKP0R and BKP1R, and the independent watchdog
//! stage0 starts before a core on trial. The backup registers keep their
//! value through every system reset (the watchdog's included) and lose it
//! with the backup domain's supply (RM0368, RTC backup registers; on a
//! board whose VBAT is tied to VDD, with power). Writing them needs the
//! PWR clock and DBP set (RM0368, backup domain access). Both behaviours
//! are to be confirmed on the board.

const RCC_APB1ENR: *mut u32 = 0x4002_3840 as *mut u32;
const RCC_APB1ENR_PWREN: u32 = 1 << 28;
const PWR_CR: *mut u32 = 0x4000_7000 as *mut u32;
const PWR_CR_DBP: u32 = 1 << 8;
const BKP: [*mut u32; 2] = [0x4000_2850 as *mut u32, 0x4000_2854 as *mut u32];

pub fn read() -> [u32; 2] {
    // SAFETY: fixed STM32F401 RTC backup registers; reading has no side
    // effect.
    BKP.map(|r| unsafe { r.read_volatile() })
}

pub fn write(cell: [u32; 2]) {
    // SAFETY: fixed RCC, PWR and RTC backup registers: the PWR clock on
    // and the backup domain open for writing, then the two cell words;
    // nothing else in DAGON uses BKP0R-BKP1R.
    unsafe {
        RCC_APB1ENR.write_volatile(RCC_APB1ENR.read_volatile() | RCC_APB1ENR_PWREN);
        PWR_CR.write_volatile(PWR_CR.read_volatile() | PWR_CR_DBP);
        for (r, v) in BKP.into_iter().zip(cell) {
            r.write_volatile(v);
        }
    }
}

const IWDG_KR: *mut u32 = 0x4000_3000 as *mut u32;
const IWDG_PR: *mut u32 = 0x4000_3004 as *mut u32;
const IWDG_RLR: *mut u32 = 0x4000_3008 as *mut u32;
const IWDG_SR: *const u32 = 0x4000_300C as *const u32;
const KEY_UNLOCK: u32 = 0x5555;
const KEY_RELOAD: u32 = 0xAAAA;
const KEY_START: u32 = 0xCCCC;
/// LSI / 32: a 1 ms tick at the LSI's nominal 32 kHz (RM0368, IWDG).
const PRESCALE_32: u32 = 3;
const RELOAD_MAX: u32 = 0xFFF;

/// Starts the independent watchdog with a period of `ms` (at most 4095)
/// before stage0 jumps to a core on trial, so a core that hangs before it
/// takes the watchdog over is reset and its start counted. The period
/// follows the LSI, which varies between parts.
pub fn arm_watchdog(ms: u32) {
    // SAFETY: fixed IWDG registers; the started core takes the watchdog
    // over (embassy-stm32 `IndependentWatchdog`).
    unsafe {
        IWDG_KR.write_volatile(KEY_START);
        IWDG_KR.write_volatile(KEY_UNLOCK);
        IWDG_PR.write_volatile(PRESCALE_32);
        IWDG_RLR.write_volatile(ms.min(RELOAD_MAX));
        while IWDG_SR.read_volatile() != 0 {}
        IWDG_KR.write_volatile(KEY_RELOAD);
    }
}
