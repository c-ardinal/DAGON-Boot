//! The boot core trial cell (`dagon_core::bootcore::trial_cell`) in the
//! RTC backup registers BKP0R and BKP1R, and the independent watchdog
//! stage0 starts before a core on trial ([`crate::iwdg`] sets it). The backup registers keep their
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

/// Stores the cell, word 0 then word 1 (one store each).
pub fn write(cell: [u32; 2]) {
    cell.into_iter()
        .enumerate()
        .for_each(|(i, w)| write_word(i, w));
}

fn write_word(i: usize, w: u32) {
    // SAFETY: fixed RCC, PWR and RTC backup registers: the PWR clock on
    // and the backup domain open for writing, then the cell word; nothing
    // else in DAGON uses BKP0R-BKP1R.
    unsafe {
        RCC_APB1ENR.write_volatile(RCC_APB1ENR.read_volatile() | RCC_APB1ENR_PWREN);
        PWR_CR.write_volatile(PWR_CR.read_volatile() | PWR_CR_DBP);
        BKP[i].write_volatile(w);
    }
}

/// The cell holds what is written to it (`dagon_core::bootcore::cell_holds`).
pub fn holds() -> bool {
    dagon_core::bootcore::cell_holds(read, write_word)
}

/// Starts the independent watchdog with a period of `ms` before stage0
/// jumps to a core on trial, so a core that hangs before it takes the
/// watchdog over is reset and its start counted.
pub fn arm_watchdog(ms: u32) {
    crate::iwdg::start(ms);
}
