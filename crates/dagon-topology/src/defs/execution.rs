//! `[execution]` and `[conformance]` of an MCU definition: what a flash
//! operation stops on this MCU, how long one erase may take, how long
//! DAGON's own code may mask interrupts, how long one request that erases
//! nothing may keep the providing task, and the flash a conformance build
//! may destroy. Hardware facts the MCU package's build turns into
//! constants (the design's execution model).

use serde::Deserialize;

use super::{BoardDef, McuDef, Region};

/// The longest `masked_max_us` a definition may declare.
pub const MASKED_MAX_US_LIMIT: u32 = 1_000;

/// What the CPU can still do while the flash erases or programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlashOp {
    /// Single-bank flash the code runs from: instruction fetch stalls
    /// until the operation ends; nothing runs, interrupts stay enabled.
    FetchStall,
    /// The operation runs from RAM with execute-in-place off; the driver
    /// masks interrupts (and pauses the other core) for each operation.
    XipOffMasked,
    /// The flash cache is off during the operation; the driver masks
    /// interrupts for each operation (handlers outside IRAM cannot run).
    CacheOffMasked,
}

impl FlashOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FetchStall => "fetch-stall",
            Self::XipOffMasked => "xip-off-masked",
            Self::CacheOffMasked => "cache-off-masked",
        }
    }
}

/// A hardware fact and how far it is established: a plain value is
/// checked against its source (the definition's comment cites it);
/// `{ unverified = V }` is a value not yet checked against its source;
/// `"unmeasured"` has no value yet (to be measured on the hardware). Most
/// facts are numbers; some are a word of a closed set (`[watchdog] flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Fact<T = u32> {
    Known(T),
    Unverified { unverified: T },
    Unmeasured(Unmeasured),
}

/// The `"unmeasured"` of a [`Fact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Unmeasured {
    #[serde(rename = "unmeasured")]
    Unmeasured,
}

impl<T> Fact<T> {
    /// The fact a definition leaves out: no value yet.
    pub fn unmeasured() -> Self {
        Fact::Unmeasured(Unmeasured::Unmeasured)
    }
}

impl<T: Copy> Fact<T> {
    /// The value, checked or not; `None` when unmeasured.
    pub fn value(self) -> Option<T> {
        match self {
            Fact::Known(v) | Fact::Unverified { unverified: v } => Some(v),
            Fact::Unmeasured(_) => None,
        }
    }

    /// The value is checked against its source.
    pub fn is_known(self) -> bool {
        matches!(self, Fact::Known(_))
    }
}

/// A fact firmware is built with that is not checked against its source:
/// where the definition states it and the value used (`None` while
/// unmeasured).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFact {
    pub key: String,
    pub value: Option<u32>,
}

impl std::fmt::Display for OpenFact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value {
            Some(v) => write!(f, "{} = {v} is not checked against its source", self.key),
            None => write!(f, "{} is unmeasured", self.key),
        }
    }
}

impl McuDef {
    /// The `[execution]` facts firmware for this MCU on `board` is built
    /// with that are not checked against their source: the masking budget,
    /// the longest request that erases nothing and every effective erase
    /// time (the board's `[flash]` when it has
    /// one). They are what the MCU package's `MASKED_MAX_US_KNOWN` and
    /// `ERASE_UNIT_MAX_MS_KNOWN` say; empty without `[execution]`.
    pub fn open_facts_on(&self, board: Option<&BoardDef>) -> Vec<OpenFact> {
        let mut out = Vec::new();
        let mut note = |key: String, f: Fact| {
            if !f.is_known() {
                out.push(OpenFact {
                    key,
                    value: f.value(),
                });
            }
        };
        if let Some(e) = &self.execution {
            note("execution.masked_max_us".into(), e.masked_max_us);
            note("execution.request_max_ms".into(), e.request_max_ms);
        }
        let flash = board.and_then(|b| Some((b.id.as_str(), b.flash.as_ref()?)));
        let (key, regions) = match flash {
            Some((id, f)) => (
                format!("board {id} flash.erase_regions"),
                &f.erase_regions[..],
            ),
            None => (
                "execution.erase_regions".into(),
                self.execution
                    .as_ref()
                    .map_or(&[][..], |e| e.erase_regions.as_slice()),
            ),
        };
        for (i, r) in regions.iter().enumerate() {
            note(format!("{key}[{i}].unit_max_ms"), r.unit_max_ms);
        }
        out
    }
}

/// `[execution]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    /// Longest DAGON's own code masks interrupts (a critical section of
    /// its own, never a flash operation), in microseconds.
    pub masked_max_us: Fact,
    /// Longest one request that erases nothing keeps the task that
    /// provides DAGON, in milliseconds: the app's thread-mode tasks and
    /// lower interrupts wait that long (execution model, rule 8). The
    /// longest are XFER_END and SET_PENDING, each a digest over a full
    /// slot. Left out, it is unmeasured.
    #[serde(default = "Fact::unmeasured")]
    pub request_max_ms: Fact,
    pub flash_op: FlashOp,
    /// Bytes one flash write covers: every write starts and ends on a
    /// multiple of it (a power of two dividing every erase unit).
    pub write_unit: u32,
    /// The value every byte of erased flash reads as.
    pub erased_value: u8,
    /// The flash DAGON erases in, by region: each region's erase unit and
    /// that unit's worst-case erase time. Every erase DAGON plans takes
    /// whole units of the region holding the address
    /// (`dagon_port::timeout::plan_erase`).
    pub erase_regions: Vec<EraseRegionDef>,
}

/// One `[execution] erase_regions` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EraseRegionDef {
    pub start: u32,
    pub len: u32,
    /// Bytes one erase operation covers here.
    pub unit: u32,
    /// Worst-case time of one such erase (the flash's datasheet maximum),
    /// in milliseconds; required (measured or not): the watchdog check and
    /// every erase wait need it.
    pub unit_max_ms: Fact,
}

/// `[conformance]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conformance {
    /// Flash a conformance build erases and writes: no slot, header, log or
    /// boot stage lies in it.
    pub scratch: Region,
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// A board's `[flash]`: the flash part it carries (its datasheet cited in
/// a comment) and how that part erases, in the MCU's shape. Its regions
/// replace the MCU's `[execution] erase_regions` whole for nodes on the
/// board.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardFlash {
    pub part: String,
    pub erase_regions: Vec<EraseRegionDef>,
}

/// `[execution]`: the masking budget within its limit and the MCU's erase
/// regions ([`erase_regions`]). An unverified or unmeasured value passes
/// the check it can take; tools report it as not established.
pub(super) fn execution(m: &McuDef) -> Result<(), String> {
    let Some(e) = &m.execution else { return Ok(()) };
    let masked = e.masked_max_us.value();
    fail(
        masked.is_some_and(|v| v == 0 || v > MASKED_MAX_US_LIMIT),
        || format!("execution.masked_max_us must be 1..={MASKED_MAX_US_LIMIT}"),
    )?;
    fail(e.request_max_ms.value() == Some(0), || {
        "execution.request_max_ms must be above 0".into()
    })?;
    erase_regions(&e.erase_regions, m, "execution.erase_regions")?;
    let unit = e.write_unit;
    fail(!unit.is_power_of_two(), || {
        format!("execution.write_unit {unit} must be a power of two")
    })?;
    for (i, r) in e.erase_regions.iter().enumerate() {
        fail(r.unit % unit != 0, || {
            format!(
                "execution.write_unit {unit} does not divide execution.erase_regions[{i}].unit {}",
                r.unit
            )
        })?;
    }
    Ok(())
}

/// Erase regions (`key`, an MCU's or a board's for MCU `m`): not empty,
/// whole units, within 32 bits, not overlapping; and two of each region's
/// erases shorter than every watchdog period of `m` (trial and confirmed):
/// the watchdog is fed between flash operations, so when an erase starts
/// up to one operation has run since the last feed.
pub(super) fn erase_regions(
    regions: &[EraseRegionDef],
    m: &McuDef,
    key: &str,
) -> Result<(), String> {
    fail(regions.is_empty(), || {
        format!("{key} needs at least one region")
    })?;
    for (i, r) in regions.iter().enumerate() {
        let at = format!("{key}[{i}]");
        fail(r.unit == 0 || r.len == 0 || r.len % r.unit != 0, || {
            format!("{at}: len must be a whole number of units, both above 0")
        })?;
        fail(u64::from(r.start) + u64::from(r.len) > 1 << 32, || {
            format!("{at} runs past 2^32")
        })?;
        let Some(ms) = r.unit_max_ms.value() else {
            return Err(format!("{at}.unit_max_ms needs a value (measured or not)"));
        };
        fail(ms == 0, || format!("{at}.unit_max_ms must be above 0"))?;
        for o in &regions[..i] {
            fail(overlaps((r.start, r.len), (o.start, o.len)), || {
                format!("{at} overlaps another erase region")
            })?;
        }
        let Some(w) = m.watchdog else { continue };
        for (name, period) in [("trial_ms", w.trial_ms), ("confirmed_ms", w.confirmed_ms)] {
            fail(u64::from(ms) * ERASE_MARGIN > u64::from(period), || {
                format!(
                        "watchdog.{name} {period} must be at least {ERASE_MARGIN} x                      {at}.unit_max_ms {ms}: the watchdog is fed between                      flash operations, and one may already have run when an erase starts"
                    )
            })?;
        }
    }
    Ok(())
}

/// Erases a watchdog period must cover (see [`execution`]).
const ERASE_MARGIN: u64 = 2;

fn overlaps(a: (u32, u32), b: (u32, u32)) -> bool {
    let (a0, a1) = (u64::from(a.0), u64::from(a.0) + u64::from(a.1));
    let (b0, b1) = (u64::from(b.0), u64::from(b.0) + u64::from(b.1));
    a0 < b1 && b0 < a1
}

/// `[conformance]`: the scratch lies in the default flash map and apart
/// from every slot, header, log face and stage0.
pub(super) fn conformance(m: &McuDef) -> Result<(), String> {
    let Some(c) = m.conformance else {
        return Ok(());
    };
    let s = (c.scratch.at, c.scratch.size);
    fail(s.1 == 0, || {
        "conformance.scratch.size must be above 0".into()
    })?;
    let map = m.default_flash_map();
    let base = if map.xip { map.base } else { 0 };
    fail(
        u64::from(s.0) < u64::from(base)
            || u64::from(s.0) + u64::from(s.1) > u64::from(base) + u64::from(map.bytes),
        || "conformance.scratch lies outside the default flash map".into(),
    )?;
    let mut taken: Vec<(u32, u32)> = Vec::new();
    for slot in &m.slots {
        taken.push((slot.start, slot.size));
        taken.push((slot.header_addr, super::requirements::HEADER));
    }
    for log in [m.layout.app_log, m.layout.bootcore_log]
        .into_iter()
        .flatten()
    {
        taken.extend(log.faces.iter().map(|f| (*f, log.face_size)));
    }
    taken.extend(m.layout.stage0.map(|r| (r.at, r.size)));
    fail(taken.iter().any(|t| overlaps(s, *t)), || {
        "conformance.scratch overlaps a slot, a header, a log face or stage0".into()
    })
}
