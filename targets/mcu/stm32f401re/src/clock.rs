//! The clock a boot loader or an app runs on: the setup the definitions
//! choose for the node (the MCU definition's `[[bootcore.clock]]` or
//! `[[app.clock]]` for the board's `[clock]`), from the constants
//! `dagon_topology::defs::codegen::clock_rs` writes (a boot loader's build,
//! an app's `wiring.rs`, a board package's `def::app_clock`), handed to
//! embassy-stm32. Nothing here assumes an HSE the board does not state.

/// What drives the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The HSI, the MCU's own 16 MHz oscillator.
    Internal,
    /// A crystal on the HSE pins.
    Crystal,
    /// A clock signal driven into OSC_IN (HSE bypass).
    External,
}

/// The main PLL's fields (RCC_PLLCFGR), as the definition names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pll {
    pub m: u8,
    pub n: u16,
    pub p: u8,
    pub q: u8,
    /// Checked against the core clock when the definition loads; the HAL
    /// sets FLASH_ACR.LATENCY from the core clock itself.
    pub flash_wait_states: u8,
}

/// A clock setup of this MCU's definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub source: Source,
    /// The input's frequency, Hz (none for the HSI without the PLL).
    pub in_hz: Option<u32>,
    /// The core clock, Hz.
    pub out_hz: u32,
    /// The main PLL; none: the core runs on the source itself.
    pub pll: Option<Pll>,
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn need(pll: &[(&str, u32)], name: &str) -> u32 {
    let mut i = 0;
    while i < pll.len() {
        if same(pll[i].0, name) {
            return pll[i].1;
        }
        i += 1;
    }
    panic!("an STM32F401 PLL setup names m, n, p, q and flash_wait_states")
}

const fn small(v: u32, max: u32) -> u32 {
    assert!(v <= max, "an STM32F401 PLL field is out of range");
    v
}

/// The PLL's fields.
const PLL_FIELDS: usize = 5;

impl Setup {
    /// The setup from its constants (`SOURCE`, `IN_HZ`, `OUT_HZ`, `PLL`).
    /// The definitions loader checked the values; this refuses, at compile
    /// time, constants that do not name such a setup.
    pub const fn new(source: &str, in_hz: Option<u32>, out_hz: u32, pll: &[(&str, u32)]) -> Self {
        let source = if same(source, "internal") {
            Source::Internal
        } else if same(source, "crystal") {
            Source::Crystal
        } else if same(source, "external") {
            Source::External
        } else {
            panic!("the STM32F401 takes its clock from the HSI, a crystal or an external clock")
        };
        assert!(
            pll.is_empty() || pll.len() == PLL_FIELDS,
            "an STM32F401 PLL setup names m, n, p, q and flash_wait_states"
        );
        assert!(
            in_hz.is_some() || (pll.is_empty() && matches!(source, Source::Internal)),
            "an STM32F401 setup from a crystal, an external clock or through the PLL states in_hz"
        );
        let pll = if pll.is_empty() {
            None
        } else {
            Some(Pll {
                m: small(need(pll, "m"), 63) as u8,
                n: small(need(pll, "n"), 432) as u16,
                p: small(need(pll, "p"), 8) as u8,
                q: small(need(pll, "q"), 15) as u8,
                flash_wait_states: small(need(pll, "flash_wait_states"), 15) as u8,
            })
        };
        Self {
            source,
            in_hz,
            out_hz,
            pll,
        }
    }

    /// The embassy-stm32 configuration for this setup: the source (the HSI,
    /// or the HSE as a crystal oscillator or bypassed for an external
    /// clock), through the main PLL's P output when the setup has a PLL;
    /// AHB and APB2 at the core clock, APB1 at half of it above its 42 MHz
    /// maximum (RM0368 section 6.2).
    #[cfg(feature = "hal")]
    pub fn hal_config(&self) -> embassy_stm32::Config {
        use embassy_stm32::rcc::{
            AHBPrescaler, APBPrescaler, Hse, HseMode, Pll as HalPll, PllMul, PllPDiv, PllPreDiv,
            PllQDiv, PllSource, Sysclk,
        };
        let mut c = embassy_stm32::Config::default();
        let hse = |mode| Hse {
            freq: embassy_stm32::time::Hertz(self.in_hz.unwrap_or(0)),
            mode,
        };
        let (pll_src, direct) = match self.source {
            Source::Internal => (PllSource::HSI, Sysclk::HSI),
            Source::Crystal => {
                c.rcc.hse = Some(hse(HseMode::Oscillator));
                (PllSource::HSE, Sysclk::HSE)
            }
            Source::External => {
                c.rcc.hse = Some(hse(HseMode::Bypass));
                (PllSource::HSE, Sysclk::HSE)
            }
        };
        c.rcc.hsi = true;
        c.rcc.pll_src = pll_src;
        c.rcc.sys = direct;
        if let Some(p) = self.pll {
            c.rcc.pll = Some(HalPll {
                prediv: PllPreDiv::from_bits(p.m),
                mul: PllMul::from_bits(p.n),
                // RCC_PLLCFGR.PLLP: 0 divides by 2, 1 by 4, 2 by 6, 3 by 8.
                divp: Some(PllPDiv::from_bits(p.p / 2 - 1)),
                divq: Some(PllQDiv::from_bits(p.q)),
                divr: None,
            });
            c.rcc.sys = Sysclk::PLL1_P;
        }
        c.rcc.ahb_pre = AHBPrescaler::DIV1;
        c.rcc.apb1_pre = if self.out_hz > APB1_MAX_HZ {
            APBPrescaler::DIV2
        } else {
            APBPrescaler::DIV1
        };
        c.rcc.apb2_pre = APBPrescaler::DIV1;
        c
    }
}

/// APB1's highest clock (RM0368 section 6.2).
#[cfg(feature = "hal")]
const APB1_MAX_HZ: u32 = 42_000_000;

/// The [`Setup`] of a module of clock constants (`SOURCE`, `IN_HZ`,
/// `OUT_HZ`, `PLL`), such as an app's `wiring::clock`:
/// `const CLOCK: Setup = dagon_mcu_stm32f401re::clock_setup!(wiring::clock);`.
#[macro_export]
macro_rules! clock_setup {
    ($($m:ident)::+) => {
        $crate::clock::Setup::new($($m::)+SOURCE, $($m::)+IN_HZ, $($m::)+OUT_HZ, $($m::)+PLL)
    };
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    const HSI_PLL: &[(&str, u32)] = &[
        ("flash_wait_states", 2),
        ("m", 16),
        ("n", 336),
        ("p", 4),
        ("q", 7),
    ];

    #[test]
    fn the_hsi_through_the_pll_is_read_by_name() {
        const S: Setup = Setup::new("internal", Some(16_000_000), 84_000_000, HSI_PLL);
        assert_eq!(S.source, Source::Internal);
        assert_eq!(
            S.pll,
            Some(Pll {
                m: 16,
                n: 336,
                p: 4,
                q: 7,
                flash_wait_states: 2
            })
        );
        let hsi = Setup::new("internal", None, 16_000_000, &[]);
        assert_eq!((hsi.pll, hsi.in_hz), (None, None));
    }

    #[test]
    fn constants_that_name_no_setup_are_refused() {
        for (source, in_hz, pll) in [
            ("pll", Some(16_000_000), HSI_PLL),
            ("crystal", None, HSI_PLL),
            ("external", Some(8_000_000), &HSI_PLL[..4]),
        ] {
            let r = std::panic::catch_unwind(|| Setup::new(source, in_hz, 1, pll));
            assert!(r.is_err(), "{source} {in_hz:?} {pll:?}");
        }
    }
}
