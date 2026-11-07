//! The clock set-up every DAGON image on this MCU uses.

/// Core clock set by [`clocks`].
pub const SYSCLK_HZ: u32 = 84_000_000;

/// 84 MHz from the HSI through the PLL (16 MHz / 16 x 336 / 4; 48 MHz on Q),
/// APB1 42 MHz. HSI keeps it independent of whether the board feeds HSE.
/// Safe to apply again after a jump: the HAL moves the core to HSI before
/// it reprograms the PLL.
pub fn clocks() -> embassy_stm32::Config {
    use embassy_stm32::rcc::{
        AHBPrescaler, APBPrescaler, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource, Sysclk,
    };
    let mut c = embassy_stm32::Config::default();
    c.rcc.hsi = true;
    c.rcc.pll_src = PllSource::HSI;
    c.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV16,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV4),
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    c.rcc.sys = Sysclk::PLL1_P;
    c.rcc.ahb_pre = AHBPrescaler::DIV1;
    c.rcc.apb1_pre = APBPrescaler::DIV2;
    c.rcc.apb2_pre = APBPrescaler::DIV1;
    c
}
