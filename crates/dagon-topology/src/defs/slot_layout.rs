//! `[[slot_layout]]` of an MCU definition: every XFER target and slot the
//! node receives into, with its body region and header placement, and the
//! [`EraseLayout`] the node and the host plan its erases from
//! (`dagon_port::timeout`).

use dagon_port::timeout::{
    erase_ranges, plan_erase, EraseLayout, EraseRegion, HeaderPlace, NodeTimings, PlanError,
    SlotLayout,
};
use dagon_proto::payload::target;
use serde::Deserialize;

use super::execution::EraseRegionDef;
use super::requirements::HEADER;
use super::{Binding, BoardDef, ImageKind, McuDef, Placement};

/// The wire slot of the recovery agent (STATUS slot 2).
const FACTORY: u8 = 2;

/// An erase layout that owns its lists (the host's form).
pub type OwnedLayout = EraseLayout<Vec<EraseRegion>, Vec<SlotLayout>>;

/// An XFER target by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetName {
    App,
    Bootcore,
    Agent,
    Topology,
}

impl TargetName {
    /// The XFER `target` byte.
    pub fn wire(self) -> u8 {
        match self {
            Self::App => target::APP,
            Self::Bootcore => target::BOOTCORE,
            Self::Agent => target::AGENT,
            Self::Topology => target::TOPOLOGY,
        }
    }

    /// Header bytes a transfer to this target carries (a topology none).
    fn header_len(self) -> u32 {
        match self {
            Self::Topology => 0,
            _ => HEADER,
        }
    }
}

/// A slot by name: `a`, `b` or `factory` (wire 0, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotName {
    A,
    B,
    Factory,
}

impl SlotName {
    pub fn wire(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::Factory => FACTORY,
        }
    }
}

/// `"inline"` or `{ detached = <address> }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum HeaderDef {
    Inline(InlineWord),
    Detached { detached: u32 },
}

/// The `"inline"` of a [`HeaderDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum InlineWord {
    #[serde(rename = "inline")]
    Inline,
}

/// One `[[slot_layout]]`: target `target`, slot `slot` takes a body of at
/// most `capacity` bytes from `start` (after its header when `inline`).
/// `partition` names the partition the body region is, checked against the
/// partition table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotLayoutDef {
    pub target: TargetName,
    pub slot: SlotName,
    pub start: u32,
    pub capacity: u32,
    pub header: HeaderDef,
    pub partition: Option<String>,
}

impl SlotLayoutDef {
    fn place(&self) -> HeaderPlace {
        match self.header {
            HeaderDef::Inline(_) => HeaderPlace::Inline,
            HeaderDef::Detached { detached } => HeaderPlace::Detached { at: detached },
        }
    }

    fn wire(&self) -> SlotLayout {
        SlotLayout {
            target: self.target.wire(),
            slot: self.slot.wire(),
            start: self.start,
            capacity: self.capacity,
            header: self.place(),
        }
    }

    fn name(&self) -> String {
        format!("slot_layout {:?}/{:?}", self.target, self.slot).to_lowercase()
    }
}

impl McuDef {
    /// The erase regions of a node of this MCU on `board`: the board's
    /// `[flash] erase_regions` when it states them, replacing the MCU's
    /// whole, else the MCU's `[execution] erase_regions`; `None` with
    /// neither.
    pub fn erase_regions_on<'a>(
        &'a self,
        board: Option<&'a BoardDef>,
    ) -> Option<&'a [EraseRegionDef]> {
        match board.and_then(|b| b.flash.as_ref()) {
            Some(f) => Some(&f.erase_regions),
            None => self.execution.as_ref().map(|e| e.erase_regions.as_slice()),
        }
    }

    /// The erase layout of a node of this MCU on `board`
    /// ([`McuDef::erase_regions_on`] and `[[slot_layout]]`).
    pub fn erase_layout_on(&self, board: Option<&BoardDef>) -> Option<OwnedLayout> {
        let regions = self.erase_regions_on(board)?;
        Some(EraseLayout {
            regions: regions
                .iter()
                .map(|r| EraseRegion {
                    start: r.start,
                    len: r.len,
                    unit: r.unit,
                    unit_max_ms: r.unit_max_ms.value().unwrap_or(0),
                })
                .collect(),
            slots: self.slot_layouts.iter().map(SlotLayoutDef::wire).collect(),
        })
    }

    /// The erase layout of this MCU alone ([`McuDef::erase_layout_on`]
    /// without a board).
    pub fn erase_layout(&self) -> Option<OwnedLayout> {
        self.erase_layout_on(None)
    }

    /// The time limits of a node of this MCU on `board`: its effective
    /// erase layout and the MCU's `[watchdog]` trial;
    /// [`NodeTimings::FALLBACK`] without `[watchdog]`.
    pub fn timings_on(&self, board: Option<&BoardDef>) -> NodeTimings<OwnedLayout> {
        match self.watchdog {
            Some(w) => NodeTimings {
                erase: self.erase_layout_on(board),
                trial_ms: w.trial_ms,
                trial_attempts: w.trial_attempts,
            },
            None => NodeTimings::FALLBACK,
        }
    }

    /// The time limits of this MCU alone ([`McuDef::timings_on`] without a
    /// board).
    pub fn timings(&self) -> NodeTimings<OwnedLayout> {
        self.timings_on(None)
    }
}

/// The (target, slot) pairs a `[[slot]]` makes the node receive into, the
/// first being the slot it names (an `any` binding names A).
fn received(image: ImageKind, binding: Binding) -> Vec<(TargetName, SlotName)> {
    let ab = |t| match binding {
        Binding::A => vec![(t, SlotName::A)],
        Binding::B => vec![(t, SlotName::B)],
        Binding::Any => vec![(t, SlotName::A), (t, SlotName::B)],
    };
    match image {
        ImageKind::App => ab(TargetName::App),
        ImageKind::Bootcore => ab(TargetName::Bootcore),
        ImageKind::Agent => vec![(TargetName::Agent, SlotName::Factory)],
        ImageKind::Stage0 => Vec::new(),
    }
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// `[[slot_layout]]`: one per target and slot; given only with
/// `[execution]`; each image slot of `[[slot]]` (app, boot core, agent) has
/// its layouts, the one it names agreeing with it; a named partition
/// matches the table; and the erase of the largest body of each layout,
/// header included, lies in the erase regions (planned in `u64`).
pub(super) fn slot_layouts(m: &McuDef) -> Result<(), String> {
    let Some(layout) = m.erase_layout() else {
        return fail(!m.slot_layouts.is_empty(), || {
            "slot_layout needs [execution] erase_regions to plan its erases".into()
        });
    };
    covered(m, &layout, "execution.erase_regions")?;
    for (i, l) in m.slot_layouts.iter().enumerate() {
        fail(
            m.slot_layouts[..i]
                .iter()
                .any(|o| (o.target, o.slot) == (l.target, l.slot)),
            || format!("{} is given twice", l.name()),
        )?;
        fail(
            l.target == TargetName::Topology && l.slot != SlotName::A,
            || format!("{}: the topology has slot a only", l.name()),
        )?;
        if let Some(name) = &l.partition {
            let p = m
                .partition_table
                .iter()
                .find(|p| &p.name == name)
                .ok_or_else(|| format!("{}: no partition '{name}'", l.name()))?;
            let header = if l.place() == HeaderPlace::Inline {
                HEADER
            } else {
                0
            };
            fail(
                (l.start, u64::from(l.capacity) + u64::from(header))
                    != (p.offset, u64::from(p.size)),
                || {
                    format!(
                        "{}: start and capacity are not partition '{name}'",
                        l.name()
                    )
                },
            )?;
        }
    }
    for s in &m.slots {
        for (n, (t, slot)) in received(s.image, s.binding).into_iter().enumerate() {
            let what = format!("slot {:?}/{:?}", s.image, s.binding).to_lowercase();
            let l = m
                .slot_layouts
                .iter()
                .find(|l| (l.target, l.slot) == (t, slot))
                .ok_or_else(|| {
                    format!("{what} needs a slot_layout for {t:?}/{slot:?}").to_lowercase()
                })?;
            if n > 0 {
                continue;
            }
            let header = match s.placement {
                Placement::Inline => HeaderPlace::Inline,
                Placement::Trailer | Placement::Detached => {
                    HeaderPlace::Detached { at: s.header_addr }
                }
            };
            fail(
                (l.start, l.capacity, l.place()) != (s.start, s.max_payload, header),
                || {
                    format!(
                        "{}: start, capacity and header must be {what}'s ({:#x}, {:#x}, {header:?})",
                        l.name(),
                        s.start,
                        s.max_payload
                    )
                },
            )?;
        }
    }
    Ok(())
}

/// The erase of each slot layout's largest body, header included, lies in
/// `layout`'s regions (`key`), planned in `u64`.
fn covered(m: &McuDef, layout: &OwnedLayout, key: &str) -> Result<(), String> {
    for l in &m.slot_layouts {
        let ranges = erase_ranges(
            &layout.slots,
            l.target.wire(),
            l.slot.wire(),
            l.target.header_len(),
            l.capacity,
        )
        .map_err(|e| format!("{}: {e:?}", l.name()))?;
        for r in ranges.iter().flatten() {
            fail(u64::from(r.start) + u64::from(r.len) > 1 << 32, || {
                format!("{} runs past 2^32", l.name())
            })?;
        }
        if let Err(PlanError::Uncovered { addr }) = plan_erase(&layout.regions, ranges) {
            return Err(format!("{}: {addr:#x} lies in no {key} entry", l.name()));
        }
    }
    Ok(())
}

/// A board's `[flash]`: its erase regions pass the MCU's region checks
/// against the MCU's watchdog, and cover every slot layout of the MCU.
pub(super) fn board_flash(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    let Some(f) = &b.flash else { return Ok(()) };
    fail(f.part.trim().is_empty(), || {
        "flash.part names the flash part".into()
    })?;
    super::execution::erase_regions(&f.erase_regions, m, "flash.erase_regions")?;
    let layout = m
        .erase_layout_on(Some(b))
        .expect("the board states regions");
    covered(m, &layout, "flash.erase_regions")
}
