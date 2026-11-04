//! `[execution]` and `[conformance]` of an MCU definition: what a flash
//! operation stops on this MCU, how long one erase may take, how long
//! DAGON's own code may mask interrupts, and the flash a conformance build
//! may destroy. Hardware facts the MCU package's build turns into
//! constants (the design's execution model).

use serde::Deserialize;

use super::{McuDef, Region};

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

/// `[execution]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    /// Longest DAGON's own code masks interrupts (a critical section of
    /// its own, never a flash operation), in microseconds.
    pub masked_max_us: u32,
    pub flash_op: FlashOp,
    /// Largest unit DAGON erases in one flash operation (bytes).
    pub erase_unit: u32,
    /// Worst-case time of one such erase (the flash's datasheet maximum),
    /// in milliseconds.
    pub erase_unit_max_ms: u32,
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

/// `[execution]`: budgets above 0, the erase unit a power of two, and one
/// erase shorter than every watchdog period that is fed around it.
pub(super) fn execution(m: &McuDef) -> Result<(), String> {
    let Some(e) = m.execution else { return Ok(()) };
    fail(
        e.masked_max_us == 0 || e.masked_max_us > MASKED_MAX_US_LIMIT,
        || format!("execution.masked_max_us must be 1..={MASKED_MAX_US_LIMIT}"),
    )?;
    fail(!e.erase_unit.is_power_of_two(), || {
        "execution.erase_unit must be a power of two".into()
    })?;
    fail(e.erase_unit_max_ms == 0, || {
        "execution.erase_unit_max_ms must be above 0".into()
    })?;
    let Some(w) = m.watchdog else { return Ok(()) };
    fail(e.erase_unit_max_ms >= w.confirmed_ms, || {
        format!(
            "execution.erase_unit_max_ms {} must be below watchdog.confirmed_ms {}: one \
             erase runs between two feeds",
            e.erase_unit_max_ms, w.confirmed_ms
        )
    })
}

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
