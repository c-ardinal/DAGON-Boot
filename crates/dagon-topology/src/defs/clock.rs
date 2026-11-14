//! Stage0's clock: a board's `[clock] source` (what drives the MCU's clock
//! input), the MCU's `[[stage0.clock]]` setups, their checks, and the setup
//! stage0 runs on for a node.
//!
//! A PLL's fields are the MCU's own, and they name the PLL: the rules that
//! check them are one row per PLL in [`PLL_RULES`], found by its fields, so
//! a project's copy of an MCU definition under another id keeps its rules
//! (an MCU whose PLL has no row may list only its internal
//! setup, as its stage0 configures no PLL). Each row cites its source.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{BoardDef, McuDef, Stage0Def};

/// What a clock comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClockKind {
    /// The MCU's own oscillator, running from reset.
    Internal,
    /// A crystal on the MCU's oscillator pins.
    Crystal,
    /// A clock signal driven into the MCU's clock input.
    External,
}

impl ClockKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Crystal => "crystal",
            Self::External => "external",
        }
    }
}

/// A board's `[clock] source`: `"internal"`, `{ crystal_hz = N }` or
/// `{ external_hz = N }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "toml::Value")]
pub enum ClockSource {
    Internal,
    Crystal(u32),
    External(u32),
}

const SOURCE_FORMS: &str =
    "clock.source is \"internal\", { crystal_hz = N } or { external_hz = N }";

impl TryFrom<toml::Value> for ClockSource {
    type Error = String;

    fn try_from(v: toml::Value) -> Result<Self, String> {
        if v.as_str() == Some("internal") {
            return Ok(Self::Internal);
        }
        let table = v.as_table().filter(|t| t.len() == 1);
        let (key, hz) = table
            .and_then(|t| t.iter().next())
            .ok_or_else(|| SOURCE_FORMS.to_string())?;
        let hz = hz
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("clock.source {key} is not a frequency in Hz (a u32)"))?;
        match key.as_str() {
            "crystal_hz" => Ok(Self::Crystal(hz)),
            "external_hz" => Ok(Self::External(hz)),
            _ => Err(SOURCE_FORMS.into()),
        }
    }
}

impl ClockSource {
    pub fn kind(self) -> ClockKind {
        match self {
            Self::Internal => ClockKind::Internal,
            Self::Crystal(_) => ClockKind::Crystal,
            Self::External(_) => ClockKind::External,
        }
    }

    /// The input frequency, Hz (none for the internal oscillator).
    pub fn in_hz(self) -> Option<u32> {
        match self {
            Self::Internal => None,
            Self::Crystal(hz) | Self::External(hz) => Some(hz),
        }
    }
}

/// A board's `[clock]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardClock {
    pub source: ClockSource,
}

/// One `[[stage0.clock]]` setup: from `source` (at `in_hz` for a crystal or
/// an external clock) stage0 runs at `out_hz`, through `pll` (the MCU's own
/// fields) when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSetup {
    pub source: ClockKind,
    pub in_hz: Option<u32>,
    pub out_hz: u32,
    #[serde(default)]
    pub pll: BTreeMap<String, u32>,
}

impl ClockSetup {
    /// The `pll` fields by name, as a stage0 build writes them.
    pub fn pll_fields(&self) -> Vec<(&str, u32)> {
        self.pll.iter().map(|(k, v)| (k.as_str(), *v)).collect()
    }

    /// The setup serves a node whose clock comes from `source`: the
    /// internal setup an internal source, another setup a source of its
    /// kind at its `in_hz`.
    pub fn matches(&self, source: ClockSource) -> bool {
        self.source == source.kind() && self.in_hz == source.in_hz()
    }
}

impl Stage0Def {
    /// The setup stage0 runs on for a node whose clock comes from
    /// `source`: the matching setup with the highest `out_hz`, else the
    /// internal one.
    pub fn clock_for(&self, source: ClockSource) -> &ClockSetup {
        self.clock
            .iter()
            .filter(|s| s.matches(source))
            .max_by_key(|s| s.out_hz)
            .or_else(|| self.clock.iter().find(|s| s.source == ClockKind::Internal))
            .expect("one internal setup, checked on load")
    }
}

impl BoardDef {
    /// The board's `[clock] source`; internal when it states none.
    pub fn clock_source(&self) -> ClockSource {
        self.clock.map_or(ClockSource::Internal, |c| c.source)
    }
}

impl McuDef {
    /// The setup this MCU's stage0 runs on for a node on `board` (`None`:
    /// no board, so the internal oscillator); `None` for an MCU without a
    /// DAGON stage0.
    pub fn stage0_clock(&self, board: Option<&BoardDef>) -> Option<&ClockSetup> {
        let source = board.map_or(ClockSource::Internal, BoardDef::clock_source);
        self.stage0.as_ref().map(|s| s.clock_for(source))
    }
}

/// How a stage0 takes a clock through one kind of PLL.
pub struct PllRules {
    /// The PLL, for messages.
    pub pll: &'static str,
    /// The sources besides the internal one its stage0 takes.
    pub sources: &'static [ClockKind],
    /// The fields of a setup's `pll`, all required: they name this PLL.
    pub fields: &'static [&'static str],
    /// Checks a setup with those fields (its source among `sources`).
    pub check: fn(&ClockSetup) -> Result<(), String>,
}

/// Every PLL a DAGON stage0 runs from, one row each.
pub const PLL_RULES: &[PllRules] = &[
    PllRules {
        pll: "the RP2040 system PLL",
        sources: &[ClockKind::Crystal],
        fields: &["refdiv", "fbdiv", "postdiv1", "postdiv2"],
        check: rp2040_pll,
    },
    PllRules {
        pll: "the STM32F401 main PLL",
        sources: &[ClockKind::Crystal, ClockKind::External],
        fields: &["m", "n", "p", "q", "flash_wait_states"],
        check: stm32f401_pll,
    },
];

fn within(name: &str, v: u32, lo: u32, hi: u32) -> Result<(), String> {
    if (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(format!("pll.{name} {v} is outside {lo}..={hi}"))
    }
}

fn hz_within(what: &str, hz: u64, lo: u64, hi: u64) -> Result<(), String> {
    ratio_within(what, hz, 1, lo, hi)
}

/// `num / den` Hz lies in `lo..=hi`, compared exactly (cross-multiplied, no
/// division), so a fraction of a hertz past a bound is refused.
fn ratio_within(what: &str, num: u64, den: u64, lo: u64, hi: u64) -> Result<(), String> {
    let (n, d) = (u128::from(num), u128::from(den));
    let hz = if num.is_multiple_of(den) {
        (num / den).to_string()
    } else {
        format!("{num}/{den}")
    };
    if n < u128::from(lo) * d {
        Err(format!("{what} {hz} Hz is below {lo} Hz"))
    } else if n > u128::from(hi) * d {
        Err(format!("{what} {hz} Hz is above {hi} Hz"))
    } else {
        Ok(())
    }
}

/// `in_hz * num / den` is exactly `out_hz`.
fn gives(s: &ClockSetup, num: u64, den: u64) -> Result<(), String> {
    let input = u64::from(s.in_hz.unwrap_or(0));
    if input * num == u64::from(s.out_hz) * den {
        return Ok(());
    }
    let gives = if (input * num).is_multiple_of(den) {
        (input * num / den).to_string()
    } else {
        format!("{}/{den}", input * num)
    };
    Err(format!(
        "this pll gives {gives} Hz, not out_hz {}",
        s.out_hz
    ))
}

/// The RP2040 system PLL (RP2040 datasheet section 2.18.2): REFDIV 1 to 63
/// (CS.REFDIV, as embassy-rp 0.10 checks it), FREF / REFDIV at least 5 MHz,
/// FBDIV 16 to 320, POSTDIV1 and POSTDIV2 1 to 7, the VCO 750 to 1600 MHz,
/// the system clock at most 133 MHz; the crystal 1 to 15 MHz (section
/// 2.16.1).
fn rp2040_pll(s: &ClockSetup) -> Result<(), String> {
    let f = |k: &str| s.pll[k];
    let (refdiv, fbdiv, pd1, pd2) = (f("refdiv"), f("fbdiv"), f("postdiv1"), f("postdiv2"));
    let input = u64::from(s.in_hz.unwrap_or(0));
    hz_within("crystal", input, 1_000_000, 15_000_000)?;
    within("refdiv", refdiv, 1, 63)?;
    within("fbdiv", fbdiv, 16, 320)?;
    within("postdiv1", pd1, 1, 7)?;
    within("postdiv2", pd2, 1, 7)?;
    ratio_within(
        "reference (in_hz / refdiv)",
        input,
        u64::from(refdiv),
        5_000_000,
        u64::MAX,
    )?;
    let vco = input * u64::from(fbdiv);
    ratio_within("VCO", vco, u64::from(refdiv), 750_000_000, 1_600_000_000)?;
    gives(s, u64::from(fbdiv), u64::from(refdiv * pd1 * pd2))?;
    hz_within("system clock", u64::from(s.out_hz), 1, 133_000_000)
}

/// The STM32F401 main PLL from the HSE: PLLM 2 to 63, PLLN 50 to 432, PLLP 2,
/// 4, 6 or 8, PLLQ 2 to 15 (the RCC_PLLCFGR fields as stm32-metapac 21.0.0
/// `rcc_f4` lists them); the PLL input 1 to 2.1 MHz, the VCO 100 to 432 MHz,
/// the system clock at most 84 MHz, a crystal 4 to 26 MHz and an external
/// clock 1 to 50 MHz (embassy-stm32 0.6.0 `rcc/f247.rs`, the STM32F401
/// limits); flash wait states at least one per started 30 MHz above the
/// first (as that file sets them) and at most 15 (FLASH_ACR.LATENCY).
fn stm32f401_pll(s: &ClockSetup) -> Result<(), String> {
    let f = |k: &str| s.pll[k];
    let (m, n, p, q, ws) = (f("m"), f("n"), f("p"), f("q"), f("flash_wait_states"));
    let input = u64::from(s.in_hz.unwrap_or(0));
    match s.source {
        ClockKind::Crystal => hz_within("crystal", input, 4_000_000, 26_000_000)?,
        _ => hz_within("external", input, 1_000_000, 50_000_000)?,
    }
    within("m", m, 2, 63)?;
    within("n", n, 50, 432)?;
    if ![2, 4, 6, 8].contains(&p) {
        return Err(format!("pll.p {p} is not one of 2, 4, 6, 8"));
    }
    within("q", q, 2, 15)?;
    ratio_within("PLL input", input, u64::from(m), 1_000_000, 2_100_000)?;
    ratio_within(
        "VCO",
        input * u64::from(n),
        u64::from(m),
        100_000_000,
        432_000_000,
    )?;
    gives(s, u64::from(n), u64::from(m * p))?;
    hz_within("system clock", u64::from(s.out_hz), 1, 84_000_000)?;
    let need = (s.out_hz - 1) / 30_000_000;
    within("flash_wait_states", ws, 0, 15)?;
    if ws < need {
        return Err(format!(
            "out_hz {} needs {need} flash wait states, not {ws}",
            s.out_hz
        ));
    }
    Ok(())
}

/// `[[stage0.clock]]` of an MCU with a stage0: exactly one internal setup
/// (only `out_hz`); every other setup with its `in_hz` and a `pll` its MCU's
/// rules accept; frequencies above 0; no two setups stage0 could not choose
/// between (same source and input, same output).
pub fn check_mcu(m: &McuDef) -> Result<(), String> {
    let Some(s) = &m.stage0 else {
        return Ok(());
    };
    let internal = s
        .clock
        .iter()
        .filter(|c| c.source == ClockKind::Internal)
        .count();
    if internal != 1 {
        return Err(format!(
            "stage0.clock needs exactly one setup with source \"internal\" (it has {internal})"
        ));
    }
    for (i, c) in s.clock.iter().enumerate() {
        check_setup(c)?;
        if let Some(d) = s.clock[..i]
            .iter()
            .find(|d| d.source == c.source && d.in_hz == c.in_hz && d.out_hz == c.out_hz)
        {
            return Err(format!(
                "two {} setups for {} Hz give {} Hz: stage0 cannot choose between them",
                d.source.name(),
                d.in_hz.unwrap_or(0),
                d.out_hz
            ));
        }
    }
    Ok(())
}

fn check_setup(c: &ClockSetup) -> Result<(), String> {
    if c.out_hz == 0 {
        return Err("stage0.clock out_hz 0 is not a frequency".into());
    }
    if c.source == ClockKind::Internal {
        if c.in_hz.is_some() || !c.pll.is_empty() {
            return Err("stage0.clock: the internal setup takes only out_hz".into());
        }
        return Ok(());
    }
    let kind = c.source.name();
    match c.in_hz {
        None => return Err(format!("stage0.clock: a {kind} setup needs in_hz")),
        Some(0) => return Err("stage0.clock in_hz 0 is not a frequency".into()),
        Some(_) => {}
    }
    let has: Vec<&str> = c.pll.keys().map(String::as_str).collect();
    let rules = PLL_RULES
        .iter()
        .find(|r| {
            let mut want = r.fields.to_vec();
            want.sort_unstable();
            want == has
        })
        .ok_or_else(|| {
            let known: Vec<String> = PLL_RULES
                .iter()
                .map(|r| format!("{} ({})", r.pll, r.fields.join(", ")))
                .collect();
            format!(
                "stage0.clock: this dagon has no rules for a pll with {}; it knows {}",
                if has.is_empty() {
                    "no fields".to_string()
                } else {
                    has.join(", ")
                },
                known.join("; ")
            )
        })?;
    if !rules.sources.contains(&c.source) {
        let names: Vec<&str> = rules.sources.iter().map(|k| k.name()).collect();
        return Err(format!(
            "stage0.clock: a stage0 on {} takes no {kind} clock (it takes: {})",
            rules.pll,
            names.join(", ")
        ));
    }
    (rules.check)(c).map_err(|e| format!("stage0.clock ({kind} {} Hz): {e}", c.in_hz.unwrap_or(0)))
}

/// A board's `[clock] source` frequency is above 0. A source no setup of
/// the MCU matches runs stage0 on the internal setup; an MCU without a
/// stage0 does not use it.
pub fn check_board(b: &BoardDef, _: &McuDef) -> Result<(), String> {
    match b.clock.map(|c| c.source) {
        Some(ClockSource::Crystal(0)) => Err("clock.source crystal_hz 0 is not a frequency".into()),
        Some(ClockSource::External(0)) => {
            Err("clock.source external_hz 0 is not a frequency".into())
        }
        _ => Ok(()),
    }
}
