//! The clock stage0 runs on: the setup the definitions chose for the node
//! (`clock.rs` from build.rs), started before the boot core check and
//! handed back before stage0 starts a core. Registers: RP2040 datasheet
//! sections 2.14 (RESETS), 2.15 (CLOCKS), 2.16 (XOSC) and 2.18 (PLL).

/// The system PLL's settings (the MCU definition's `pll` fields).
#[derive(Clone, Copy)]
pub struct Pll {
    pub refdiv: u32,
    pub fbdiv: u32,
    pub postdiv1: u32,
    pub postdiv2: u32,
}

include!(concat!(env!("OUT_DIR"), "/clock.rs"));

// The PLL runs from the crystal oscillator; the definitions loader takes no
// other source for the RP2040.
const _: () = assert!(PLL.is_none() || matches!(SOURCE.as_bytes(), b"crystal"));

/// CPU cycles per millisecond on the chosen setup.
pub const CYCLES_PER_MS: u32 = OUT_HZ / 1000;

const RESETS_CLR: *mut u32 = (0x4000_C000 + 0x3000) as *mut u32;
const RESET_DONE: *const u32 = (0x4000_C000 + 0x8) as *const u32;
const PLL_SYS_RESET: u32 = 1 << 12;

const CLK_SYS_CTRL: *mut u32 = (0x4000_8000 + 0x3C) as *mut u32;
const CLK_SYS_SELECTED: *const u32 = (0x4000_8000 + 0x44) as *const u32;
/// CLK_SYS_CTRL.SRC: clk_ref (0) or the aux source (1); AUXSRC (bits 7:5)
/// 0 is pll_sys.
const SRC_REF: u32 = 0;
const SRC_AUX: u32 = 1;

const XOSC_CTRL: *mut u32 = 0x4002_4000 as *mut u32;
const XOSC_STATUS: *const u32 = (0x4002_4000 + 0x04) as *const u32;
const XOSC_STARTUP: *mut u32 = (0x4002_4000 + 0x0C) as *mut u32;
const XOSC_RANGE_1_15MHZ: u32 = 0xAA0;
const XOSC_ENABLE: u32 = 0xFAB << 12;
const XOSC_DISABLE: u32 = 0xD1E << 12;
const XOSC_STABLE: u32 = 1 << 31;
/// XOSC STARTUP counts 256 cycles a step; this waits 128 x 1 ms worth of
/// the crystal's cycles, as embassy-rp 0.10's `ClockConfig::crystal` does
/// (its `delay_multiplier` 128), long enough for the crystals that HAL
/// starts on the boards.
const XOSC_DELAY_MULTIPLIER: u32 = 128;

const PLL_CS: *mut u32 = 0x4002_8000 as *mut u32;
const PLL_PWR: *mut u32 = (0x4002_8000 + 0x04) as *mut u32;
const PLL_FBDIV: *mut u32 = (0x4002_8000 + 0x08) as *mut u32;
const PLL_PRIM: *mut u32 = (0x4002_8000 + 0x0C) as *mut u32;
const PLL_LOCK: u32 = 1 << 31;
/// PWR: PD (bit 0), DSMPD (2), POSTDIVPD (3), VCOPD (5); all set is off.
const PWR_OFF: u32 = 0x2D;
const PWR_POSTDIVPD: u32 = 1 << 3;
const PWR_DSMPD: u32 = 1 << 2;

fn wait(reg: *const u32, mask: u32, want: u32) {
    // SAFETY: a fixed RP2040 status register; reading has no side effect.
    while unsafe { reg.read_volatile() } & mask != want {}
}

/// Runs the core on the chosen setup: a crystal's PLL output, or (for the
/// internal setup) the ring oscillator as the chip starts.
pub fn start() {
    let (Some(pll), Some(in_hz)) = (PLL, IN_HZ) else {
        return;
    };
    // SAFETY: fixed RP2040 registers of XOSC, PLL_SYS, RESETS and CLOCKS,
    // in the datasheet's order (XOSC stable, PLL locked, then clk_sys
    // switched glitch-free); stage0 is the only code running and uses no
    // peripheral these clock.
    unsafe {
        XOSC_CTRL.write_volatile(XOSC_RANGE_1_15MHZ);
        XOSC_STARTUP.write_volatile(((in_hz / 1000) * XOSC_DELAY_MULTIPLIER + 128) / 256);
        XOSC_CTRL.write_volatile(XOSC_RANGE_1_15MHZ | XOSC_ENABLE);
        wait(XOSC_STATUS, XOSC_STABLE, XOSC_STABLE);

        RESETS_CLR.write_volatile(PLL_SYS_RESET);
        wait(RESET_DONE, PLL_SYS_RESET, PLL_SYS_RESET);
        PLL_PWR.write_volatile(PWR_OFF);
        PLL_CS.write_volatile(pll.refdiv);
        PLL_FBDIV.write_volatile(pll.fbdiv);
        PLL_PWR.write_volatile(PWR_POSTDIVPD | PWR_DSMPD);
        wait(PLL_CS, PLL_LOCK, PLL_LOCK);
        PLL_PRIM.write_volatile(pll.postdiv1 << 16 | pll.postdiv2 << 12);
        PLL_PWR.write_volatile(PWR_DSMPD);

        // AUXSRC is pll_sys (0) while clk_sys still runs from clk_ref.
        CLK_SYS_CTRL.write_volatile(SRC_REF);
        CLK_SYS_CTRL.write_volatile(SRC_AUX);
        wait(CLK_SYS_SELECTED, 0b11, 1 << SRC_AUX);
    }
}

/// Puts the clocks back the way the chip starts (clk_sys on clk_ref, the
/// ring oscillator; the PLL and the crystal oscillator off), so the core
/// stage0 starts sets its clocks up from the same state on every board.
pub fn hand_back() {
    if PLL.is_none() {
        return;
    }
    // SAFETY: as in `start`, the reverse order: clk_sys leaves the PLL
    // before the PLL and XOSC stop.
    unsafe {
        CLK_SYS_CTRL.write_volatile(SRC_REF);
        wait(CLK_SYS_SELECTED, 0b11, 1 << SRC_REF);
        PLL_PWR.write_volatile(PWR_OFF);
        XOSC_CTRL.write_volatile(XOSC_RANGE_1_15MHZ | XOSC_DISABLE);
    }
}
