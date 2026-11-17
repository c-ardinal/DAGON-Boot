//! The clock a boot loader or an app runs on: the setup the definitions
//! choose for the node (the MCU definition's `[[bootcore.clock]]` or
//! `[[app.clock]]` for the board's `[clock]`), from the constants
//! `dagon_topology::defs::codegen::clock_rs` writes (a boot loader's build,
//! an app's `wiring.rs`, a board package's `def::app_clock`), handed to
//! embassy-rp. Nothing here assumes a crystal: without one in the setup
//! the core runs on the ring oscillator and there is no USB clock.

#[cfg(feature = "hal")]
use embassy_rp::clocks::{ClockConfig, PllConfig, RoscConfig, RoscRange, XoscConfig};

use dagon_mcu_common::link::Kind;

/// One PLL's dividers (RP2040 datasheet section 2.18.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pll {
    pub refdiv: u8,
    pub fbdiv: u16,
    pub postdiv1: u8,
    pub postdiv2: u8,
}

/// A crystal and the PLLs it drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crystal {
    /// The crystal's frequency, Hz.
    pub hz: u32,
    /// The system PLL, which clocks the core and the peripherals.
    pub sys: Pll,
    /// The USB PLL (48 MHz for the USB controller), when the setup has it.
    pub usb: Option<Pll>,
}

/// A clock setup of this MCU's definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    /// The core clock, Hz (for the ring oscillator its nominal frequency).
    pub out_hz: u32,
    /// The crystal and its PLLs; none: the ring oscillator.
    pub crystal: Option<Crystal>,
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

const fn field(pll: &[(&str, u32)], name: &str) -> Option<u32> {
    let mut i = 0;
    while i < pll.len() {
        if same(pll[i].0, name) {
            return Some(pll[i].1);
        }
        i += 1;
    }
    None
}

const fn need(pll: &[(&str, u32)], name: &str) -> u32 {
    match field(pll, name) {
        Some(v) => v,
        None => panic!("an RP2040 PLL setup names refdiv, fbdiv, postdiv1 and postdiv2"),
    }
}

const fn byte(v: u32) -> u8 {
    assert!(v <= u8::MAX as u32, "an RP2040 PLL divider is out of range");
    v as u8
}

/// The system PLL's fields, and the USB PLL's (`usb_` before each name).
const SYS_FIELDS: [&str; 4] = ["refdiv", "fbdiv", "postdiv1", "postdiv2"];
const USB_FIELDS: [&str; 4] = ["usb_refdiv", "usb_fbdiv", "usb_postdiv1", "usb_postdiv2"];

/// The PLL whose fields are `names` (refdiv, fbdiv, postdiv1, postdiv2).
const fn pll_named(pll: &[(&str, u32)], names: [&str; 4]) -> Pll {
    let fbdiv = need(pll, names[1]);
    assert!(
        fbdiv <= u16::MAX as u32,
        "an RP2040 PLL divider is out of range"
    );
    Pll {
        refdiv: byte(need(pll, names[0])),
        fbdiv: fbdiv as u16,
        postdiv1: byte(need(pll, names[2])),
        postdiv2: byte(need(pll, names[3])),
    }
}

impl Setup {
    /// The setup from its constants (`SOURCE`, `IN_HZ`, `OUT_HZ`, `PLL`):
    /// `"internal"` without a PLL, or `"crystal"` with the system PLL's
    /// fields and, for the USB clock, the USB PLL's (`usb_` fields). The
    /// definitions loader checked the values; this refuses, at compile
    /// time, constants that do not name such a setup.
    pub const fn new(source: &str, in_hz: Option<u32>, out_hz: u32, pll: &[(&str, u32)]) -> Self {
        if same(source, "internal") {
            assert!(
                in_hz.is_none() && pll.is_empty(),
                "the RP2040's internal setup runs no PLL (its PLLs run from the crystal)"
            );
            return Self {
                out_hz,
                crystal: None,
            };
        }
        assert!(
            same(source, "crystal"),
            "the RP2040 takes its clock from its ring oscillator or a crystal"
        );
        let Some(hz) = in_hz else {
            panic!("an RP2040 crystal setup states the crystal's frequency")
        };
        let usb = field(pll, "usb_refdiv").is_some();
        assert!(
            pll.len() == if usb { 8 } else { 4 },
            "an RP2040 crystal setup names the system PLL's fields and, for USB, the USB PLL's"
        );
        Self {
            out_hz,
            crystal: Some(Crystal {
                hz,
                sys: pll_named(pll, SYS_FIELDS),
                usb: if usb {
                    Some(pll_named(pll, USB_FIELDS))
                } else {
                    None
                },
            }),
        }
    }

    /// The setup gives the USB controller its clock.
    pub const fn has_usb(&self) -> bool {
        matches!(self.crystal, Some(Crystal { usb: Some(_), .. }))
    }

    /// The setup gives `kind` the clock it runs on: a USB uplink needs
    /// the USB clock ([`require_uplink_clock!`] stops a build without it).
    pub const fn runs_uplink(&self, kind: Kind) -> bool {
        !matches!(kind, Kind::UsbCdc) || self.has_usb()
    }

    /// The embassy-rp configuration for this setup. From a crystal: the
    /// crystal oscillator with the system PLL on the core and the
    /// peripherals, and with the USB PLL on USB, the ADC and the RTC
    /// (48 MHz, / 1024 for the RTC, as embassy-rp's `ClockConfig::crystal`
    /// sets them). On the ring oscillator: medium range, divider 16, drive
    /// strengths 0, which embassy-rp 0.10 documents as its nominal 6.5 MHz
    /// (`RoscConfig::hz`), with `out_hz` as that frequency; no USB, ADC or
    /// RTC clock. Diagnostic blinks are timed on `out_hz` from here on.
    #[cfg(feature = "hal")]
    pub fn hal_config(&self) -> embassy_rp::config::Config {
        let clocks = match self.crystal {
            Some(x) => crystal_clocks(x),
            None => rosc_clocks(self.out_hz),
        };
        #[cfg(target_os = "none")]
        crate::arm::led::set_cycles_per_ms(self.out_hz / 1000);
        embassy_rp::config::Config::new(clocks)
    }
}

/// Whether this node's boot core receives on some link, as far as the
/// running app can tell: the app runs the USB clock, so the board has the
/// crystal the boot core's USB clock comes from too (an RP2040 app is
/// reached over USB, which needs it). Without it the app tells of none and
/// refuses the hand-over.
#[cfg(feature = "hal")]
pub fn boot_core_receives() -> bool {
    embassy_rp::pac::CLOCKS.clk_usb_ctrl().read().enable()
}

#[cfg(feature = "hal")]
impl From<Pll> for PllConfig {
    fn from(p: Pll) -> Self {
        PllConfig {
            refdiv: p.refdiv,
            fbdiv: p.fbdiv,
            post_div1: p.postdiv1,
            post_div2: p.postdiv2,
        }
    }
}

#[cfg(feature = "hal")]
/// XOSC STARTUP delay multiplier: embassy-rp 0.10's `ClockConfig::crystal`
/// value, which stage0 uses too.
const XOSC_DELAY_MULTIPLIER: u32 = 128;

#[cfg(feature = "hal")]
fn crystal_clocks(x: Crystal) -> ClockConfig {
    let mut c = ClockConfig::crystal(x.hz);
    c.rosc = None;
    c.xosc = Some(XoscConfig {
        hz: x.hz,
        sys_pll: Some(x.sys.into()),
        usb_pll: x.usb.map(Into::into),
        delay_multiplier: XOSC_DELAY_MULTIPLIER,
    });
    if x.usb.is_none() {
        c.usb_clk = None;
        c.adc_clk = None;
        c.rtc_clk = None;
    }
    c
}

#[cfg(feature = "hal")]
fn rosc_clocks(out_hz: u32) -> ClockConfig {
    let mut c = ClockConfig::rosc();
    c.rosc = Some(RoscConfig {
        hz: out_hz,
        range: RoscRange::Medium,
        drive_strength: [0; 8],
        div: 16,
    });
    c.adc_clk = None;
    c.rtc_clk = None;
    c
}

/// Stops the build of node `$node` (a string literal) when its clock
/// `$clock` does not give its uplink `$kind` the clock it runs on: a USB
/// uplink needs the 48 MHz USB clock, which the RP2040 makes only from a
/// crystal the board states.
/// `dagon_mcu_rp2040::require_uplink_clock!(CLOCK, UPLINK, "hub");`
#[macro_export]
macro_rules! require_uplink_clock {
    ($clock:expr, $kind:expr, $node:literal) => {
        const _: () = assert!(
            $clock.runs_uplink($kind),
            concat!(
                "node ",
                $node,
                ": its USB uplink needs the 48 MHz USB clock, which the RP2040 makes only \
                 from a crystal; its board states none that the MCU's [[app.clock]] setups \
                 take (board definition [clock] source = {{ crystal_hz = N }})"
            )
        );
    };
}

/// The [`Setup`] of a module of clock constants (`SOURCE`, `IN_HZ`,
/// `OUT_HZ`, `PLL`), such as an app's `wiring::clock`:
/// `const CLOCK: Setup = dagon_mcu_rp2040::clock_setup!(wiring::clock);`.
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

    const PLL_12: &[(&str, u32)] = &[
        ("fbdiv", 125),
        ("postdiv1", 6),
        ("postdiv2", 2),
        ("refdiv", 1),
        ("usb_fbdiv", 120),
        ("usb_postdiv1", 6),
        ("usb_postdiv2", 5),
        ("usb_refdiv", 1),
    ];

    #[test]
    fn a_crystal_setup_names_both_plls() {
        const S: Setup = Setup::new("crystal", Some(12_000_000), 125_000_000, PLL_12);
        let x = S.crystal.unwrap();
        assert_eq!(
            x.sys,
            Pll {
                refdiv: 1,
                fbdiv: 125,
                postdiv1: 6,
                postdiv2: 2
            }
        );
        assert_eq!(
            x.usb,
            Some(Pll {
                refdiv: 1,
                fbdiv: 120,
                postdiv1: 6,
                postdiv2: 5
            })
        );
        assert!(S.has_usb());
        assert!(S.runs_uplink(Kind::UsbCdc));
        let sys_only = Setup::new("crystal", Some(12_000_000), 125_000_000, &PLL_12[..4]);
        assert!(!sys_only.has_usb());
    }

    #[test]
    fn the_internal_setup_has_no_usb_clock() {
        let s = Setup::new("internal", None, 6_500_000, &[]);
        assert_eq!(
            s,
            Setup {
                out_hz: 6_500_000,
                crystal: None
            }
        );
        assert!(!s.has_usb());
        assert!(s.runs_uplink(Kind::I2c));
        assert!(!s.runs_uplink(Kind::UsbCdc));
    }

    #[test]
    fn constants_that_name_no_setup_are_refused() {
        for (source, in_hz, pll) in [
            ("internal", Some(1), &[][..]),
            ("external", Some(12_000_000), &PLL_12[..4]),
            ("crystal", None, &PLL_12[..4]),
            ("crystal", Some(12_000_000), &PLL_12[..3]),
        ] {
            let r = std::panic::catch_unwind(|| Setup::new(source, in_hz, 1, pll));
            assert!(r.is_err(), "{source} {in_hz:?} {pll:?}");
        }
    }
}
