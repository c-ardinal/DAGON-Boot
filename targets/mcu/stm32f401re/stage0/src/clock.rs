//! The clock stage0 runs on: the setup the definitions chose for the node
//! (`clock.rs` from build.rs), started before the boot core check and
//! handed back before stage0 starts a core. Registers: RM0368, reset and
//! clock control (RCC_CR, RCC_PLLCFGR, RCC_CFGR) and embedded flash
//! (FLASH_ACR); bit positions as stm32-metapac 21.0.0 `rcc_f4` and
//! `flash_f4` give them.

/// The main PLL's settings (the MCU definition's `pll` fields).
#[derive(Clone, Copy)]
pub struct Pll {
    pub m: u32,
    pub n: u32,
    pub p: u32,
    pub q: u32,
    pub flash_wait_states: u32,
}

include!(concat!(env!("OUT_DIR"), "/clock.rs"));

// The PLL runs from the HSE, crystal or external, at its stated input.
const _: () = assert!(
    PLL.is_none() || (IN_HZ.is_some() && matches!(SOURCE.as_bytes(), b"crystal" | b"external"))
);

/// CPU cycles per millisecond on the chosen setup.
pub const CYCLES_PER_MS: u32 = OUT_HZ / 1000;

const RCC_CR: *mut u32 = 0x4002_3800 as *mut u32;
const RCC_PLLCFGR: *mut u32 = 0x4002_3804 as *mut u32;
const RCC_CFGR: *mut u32 = 0x4002_3808 as *mut u32;
const FLASH_ACR: *mut u32 = 0x4002_3C00 as *mut u32;

const HSEON: u32 = 1 << 16;
const HSERDY: u32 = 1 << 17;
const HSEBYP: u32 = 1 << 18;
const PLLON: u32 = 1 << 24;
const PLLRDY: u32 = 1 << 25;
/// RCC_PLLCFGR.PLLSRC: the HSE.
const PLLSRC_HSE: u32 = 1 << 22;
/// RCC_PLLCFGR at reset.
const PLLCFGR_RESET: u32 = 0x2400_3010;
/// RCC_CFGR.SW / SWS: HSI 0, PLL 2.
const SW_MASK: u32 = 0b11;
const SW_PLL: u32 = 0b10;
/// RCC_CFGR.PPRE1 (bits 12:10): APB1 at HCLK / 2. APB1 runs at most 42 MHz
/// on the STM32F401 (embassy-stm32 0.6.0 `rcc/f247.rs`, PCLK1).
const PPRE1_MASK: u32 = 0b111 << 10;
const PPRE1_DIV2: u32 = 0b100 << 10;
const APB1_MAX_HZ: u32 = 42_000_000;
const LATENCY_MASK: u32 = 0xF;

/// The external clock is driven into OSC_IN (HSE bypass); a crystal runs
/// the HSE oscillator.
const BYPASS: bool = matches!(SOURCE.as_bytes(), b"external");

fn modify(reg: *mut u32, clear: u32, set: u32) {
    // SAFETY: the fixed RCC and FLASH registers above; stage0 is the only
    // code running.
    unsafe { reg.write_volatile(reg.read_volatile() & !clear | set) }
}

fn wait(reg: *mut u32, mask: u32, want: u32) {
    // SAFETY: as in `modify`; reading has no side effect.
    while unsafe { reg.read_volatile() } & mask != want {}
}

/// Runs the core on the chosen setup: the HSE through the main PLL, or
/// (for the internal setup) the HSI as the chip starts.
pub fn start() {
    let Some(pll) = PLL else {
        return;
    };
    let bypass = if BYPASS { HSEBYP } else { 0 };
    modify(RCC_CR, 0, bypass);
    modify(RCC_CR, 0, HSEON);
    wait(RCC_CR, HSERDY, HSERDY);
    // Wait states before the clock rises.
    modify(FLASH_ACR, LATENCY_MASK, pll.flash_wait_states);
    wait(FLASH_ACR, LATENCY_MASK, pll.flash_wait_states);
    if OUT_HZ > APB1_MAX_HZ {
        modify(RCC_CFGR, PPRE1_MASK, PPRE1_DIV2);
    }
    let p = (pll.p / 2 - 1) << 16;
    // SAFETY: as in `modify`; the PLL is off from reset.
    unsafe { RCC_PLLCFGR.write_volatile(pll.m | pll.n << 6 | p | PLLSRC_HSE | pll.q << 24) };
    modify(RCC_CR, 0, PLLON);
    wait(RCC_CR, PLLRDY, PLLRDY);
    modify(RCC_CFGR, SW_MASK, SW_PLL);
    wait(RCC_CFGR, SW_MASK << 2, SW_PLL << 2);
}

/// Puts the clocks back the way the chip starts (the HSI, the PLL and HSE
/// off, APB1 undivided, no flash wait states), so the core stage0 starts
/// sets its clocks up from the same state on every board.
pub fn hand_back() {
    if PLL.is_none() {
        return;
    }
    modify(RCC_CFGR, SW_MASK, 0);
    wait(RCC_CFGR, SW_MASK << 2, 0);
    modify(RCC_CR, PLLON, 0);
    wait(RCC_CR, PLLRDY, 0);
    // SAFETY: as in `modify`; the PLL is off.
    unsafe { RCC_PLLCFGR.write_volatile(PLLCFGR_RESET) };
    modify(RCC_CR, HSEON, 0);
    wait(RCC_CR, HSERDY, 0);
    modify(RCC_CR, HSEBYP, 0);
    modify(RCC_CFGR, PPRE1_MASK, 0);
    modify(FLASH_ACR, LATENCY_MASK, 0);
}
