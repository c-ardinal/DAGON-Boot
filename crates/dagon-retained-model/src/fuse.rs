//! Lockup fuse: one cell per image kind that runs sandboxed calls (the boot
//! core's cell, the app's cell shared by the app and relay layers). A cell
//! is an arm slot followed by record slots, all records of one layout.
//!
//! Only the cell's own layer stores into it; every start reads every cell.
//!
//! A call is armed before it runs (after the watchdog is fed) and disarmed
//! by the first store of its return path, also after an overrun or a fault:
//! a call has returned exactly when its disarm is stored. Only the watchdog
//! ends a lockup, and with the feed before arming and the budget far below
//! the watchdog period, a watchdog reset with a call armed means that call
//! locked up. The first start after a run died classifies every armed call
//! it finds: a *lockup* verdict when the MCU package reports a watchdog
//! reset not yet acknowledged, else *clean*. A start classifies its own
//! call directly and records verdicts for other layers' calls in its own
//! cell; the owning layer's start then trips (lockup) or just disarms
//! (clean). The watchdog flag is acknowledged only after a start stored
//! everything it had to, so a reset in between loses nothing. Clearing
//! another layer's trip goes through a clear request the agent stores in
//! its own cell.

use crate::mem::{Cut, Mem};
use dagon_format::{crc32, sha256};

pub const LAYER_BOOT: u8 = 1;
pub const LAYER_APP: u8 = 2;
pub const LAYER_RELAY: u8 = 3;

/// Everything that names one sandboxed call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallSite {
    pub layer: u8,
    /// Image slot.
    pub slot: u32,
    /// SHA-256 of the image header.
    pub header: [u8; 32],
    /// Position in the image's extension chain.
    pub chain: u32,
    /// SHA-256 of the linked extension artifact.
    pub artifact: [u8; 32],
}

/// `SHA-256("dagon-fuse/1" ‖ layer ‖ slot u32 LE ‖ header ‖ chain u32 LE ‖
/// artifact)`: fixed widths, so distinct call sites give distinct inputs.
pub fn fuse_id(c: &CallSite) -> [u8; 32] {
    let mut b = Vec::with_capacity(12 + 1 + 4 + 32 + 4 + 32);
    b.extend_from_slice(b"dagon-fuse/1");
    b.push(c.layer);
    b.extend_from_slice(&c.slot.to_le_bytes());
    b.extend_from_slice(&c.header);
    b.extend_from_slice(&c.chain.to_le_bytes());
    b.extend_from_slice(&c.artifact);
    sha256(&b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A call is running.
    Armed,
    /// The call site is disabled.
    Tripped,
    /// The agent asks another layer's start to clear its trip.
    ClearRequest,
    /// Another layer's armed call died by a watchdog reset.
    Lockup,
    /// Another layer's armed call died by any other reset.
    Clean,
    /// A clear request was carried out (kept until the request is gone, so
    /// one request clears one trip).
    Ack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuseRec {
    pub mark: Mark,
    pub layer: u8,
    pub id: [u8; 32],
}

/// A record layout. `stores` writes a whole record (mark last); storing 0
/// at offset 0 frees a slot.
pub trait FuseCodec {
    fn layout(&self) -> u8;
    fn size(&self) -> usize;
    fn mark_byte(&self, m: Mark) -> u8;
    fn stores(&self, r: &FuseRec) -> Vec<(usize, u8)>;
    fn decode(&self, b: &[u8]) -> Option<FuseRec>;
}

/// Layout 1: 40 bytes.
///
/// | off | field |
/// |---|---|
/// | 0 | mark: `0xA1` armed, `0xA2` tripped, `0xA3` clear request, `0xA4` lockup verdict, `0xA5` clean verdict, `0xA6` clear done; anything else: free |
/// | 1 | layer |
/// | 2 | zero, 2 bytes |
/// | 4 | fuse id, 32 bytes |
/// | 36 | CRC-32 over `[1]` then bytes 1..36 |
///
/// A write stores byte 0 = 0, bytes 1..40, then the mark.
pub struct FuseV1;

fn mark_of(base: u8, b: u8) -> Option<Mark> {
    [
        Mark::Armed,
        Mark::Tripped,
        Mark::ClearRequest,
        Mark::Lockup,
        Mark::Clean,
        Mark::Ack,
    ]
    .into_iter()
    .find(|&m| b == base + m as u8 + 1)
}

pub(crate) fn rec_crc(layout: u8, body: &[u8]) -> u32 {
    let mut v = vec![layout];
    v.extend_from_slice(body);
    crc32(&v)
}

impl FuseCodec for FuseV1 {
    fn layout(&self) -> u8 {
        1
    }

    fn size(&self) -> usize {
        40
    }

    fn mark_byte(&self, m: Mark) -> u8 {
        0xA0 + m as u8 + 1
    }

    fn stores(&self, r: &FuseRec) -> Vec<(usize, u8)> {
        let mut b = [0u8; 40];
        b[1] = r.layer;
        b[4..36].copy_from_slice(&r.id);
        let c = rec_crc(1, &b[1..36]);
        b[36..40].copy_from_slice(&c.to_le_bytes());
        let mut s = vec![(0, 0)];
        s.extend((1..40).map(|i| (i, b[i])));
        s.push((0, self.mark_byte(r.mark)));
        s
    }

    fn decode(&self, b: &[u8]) -> Option<FuseRec> {
        let mark = mark_of(0xA0, b[0])?;
        let crc = u32::from_le_bytes(b[36..40].try_into().ok()?);
        let ok = b[2] == 0 && b[3] == 0 && crc == rec_crc(1, &b[1..36]);
        ok.then(|| FuseRec {
            mark,
            layer: b[1],
            id: b[4..36].try_into().unwrap(),
        })
    }
}

/// Another layout, for the versioning property: 44 bytes, marks `0x51..`,
/// id first, CRC seeded with 2.
pub struct FuseV2;

impl FuseCodec for FuseV2 {
    fn layout(&self) -> u8 {
        2
    }

    fn size(&self) -> usize {
        44
    }

    fn mark_byte(&self, m: Mark) -> u8 {
        0x50 + m as u8 + 1
    }

    fn stores(&self, r: &FuseRec) -> Vec<(usize, u8)> {
        let mut b = [0u8; 44];
        b[1..33].copy_from_slice(&r.id);
        b[33] = r.layer;
        let c = rec_crc(2, &b[1..40]);
        b[40..44].copy_from_slice(&c.to_le_bytes());
        let mut s = vec![(0, 0)];
        s.extend((1..44).map(|i| (i, b[i])));
        s.push((0, self.mark_byte(r.mark)));
        s
    }

    fn decode(&self, b: &[u8]) -> Option<FuseRec> {
        let mark = mark_of(0x50, b[0])?;
        let crc = u32::from_le_bytes(b[40..44].try_into().ok()?);
        (crc == rec_crc(2, &b[1..40])).then(|| FuseRec {
            mark,
            layer: b[33],
            id: b[1..33].try_into().unwrap(),
        })
    }
}

/// One layer's cell: the arm slot at `base`, then `trips` trip slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub base: usize,
    pub trips: usize,
}

impl Cell {
    pub fn bytes<C: FuseCodec>(&self, c: &C) -> std::ops::Range<usize> {
        self.base..self.base + c.size() * (1 + self.trips)
    }

    fn slot<C: FuseCodec>(&self, c: &C, i: usize) -> usize {
        self.base + c.size() * i
    }
}

fn read_slot<C: FuseCodec>(c: &C, mem: &Mem, at: usize) -> Option<FuseRec> {
    c.decode(&mem.bytes()[at..at + c.size()])
}

fn write<C: FuseCodec>(c: &C, mem: &mut Mem, at: usize, r: &FuseRec) -> Result<(), Cut> {
    for (o, v) in c.stores(r) {
        mem.store(at + o, v)?;
    }
    Ok(())
}

/// Trip slots of a cell holding a record with this mark and id.
fn find<C: FuseCodec>(c: &C, mem: &Mem, cell: Cell, mark: Mark, id: &[u8; 32]) -> Option<usize> {
    (1..=cell.trips)
        .map(|i| cell.slot(c, i))
        .find(|&a| read_slot(c, mem, a).is_some_and(|r| r.mark == mark && r.id == *id))
}

fn append<C: FuseCodec>(c: &C, mem: &mut Mem, cell: Cell, r: &FuseRec) -> Result<bool, Cut> {
    if find(c, mem, cell, r.mark, &r.id).is_some() {
        return Ok(true);
    }
    let free = (1..=cell.trips)
        .map(|i| cell.slot(c, i))
        .find(|&a| read_slot(c, mem, a).is_none());
    match free {
        Some(a) => write(c, mem, a, r).map(|_| true),
        None => Ok(false),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FuseEvent {
    Tripped([u8; 32]),
    Stale([u8; 32]),
    Cleared([u8; 32]),
    /// No slot left for a trip: the layer arms nothing until the host clears.
    Full([u8; 32]),
    /// Another layer's trip, reported and left alone.
    Foreign(u8, [u8; 32]),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Start {
    /// Tripped call sites of this layer.
    pub disabled: Vec<[u8; 32]>,
    /// Call sites another cell still holds a verdict for: not armed in
    /// this run (the verdict goes at the other layer's next start).
    pub held: Vec<[u8; 32]>,
    /// False while a locked call could not be recorded as a trip, or a
    /// verdict could not be stored: the layer arms nothing in this run.
    pub can_arm: bool,
    /// Every verdict and trip was stored: the caller may acknowledge the
    /// watchdog flag.
    pub complete: bool,
    pub events: Vec<FuseEvent>,
}

fn armed<C: FuseCodec>(c: &C, mem: &Mem, cell: Cell) -> Option<FuseRec> {
    read_slot(c, mem, cell.slot(c, 0)).filter(|r| r.mark == Mark::Armed)
}

/// The verdict any cell holds for a call site.
fn verdict<C: FuseCodec>(c: &C, mem: &Mem, cells: &[Cell], id: &[u8; 32]) -> Option<bool> {
    cells.iter().find_map(|o| {
        if find(c, mem, *o, Mark::Lockup, id).is_some() {
            Some(true)
        } else {
            find(c, mem, *o, Mark::Clean, id).map(|_| false)
        }
    })
}

fn others(cells: &[Cell], me: usize) -> impl Iterator<Item = Cell> + '_ {
    cells
        .iter()
        .enumerate()
        .filter(move |&(j, _)| j != me)
        .map(|(_, o)| *o)
}

/// Records a verdict for every other layer's armed call that has none.
fn classify<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cells: &[Cell],
    me: usize,
    watchdog: bool,
) -> Result<bool, Cut> {
    let mut complete = true;
    for o in others(cells, me).collect::<Vec<_>>() {
        let Some(r) = armed(c, mem, o) else {
            continue;
        };
        if verdict(c, mem, cells, &r.id).is_none() {
            let mark = if watchdog { Mark::Lockup } else { Mark::Clean };
            complete &= append(c, mem, cells[me], &FuseRec { mark, ..r })?;
        }
    }
    Ok(complete)
}

/// Acts on the layer's own armed call.
fn own_call<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cells: &[Cell],
    me: usize,
    watchdog: bool,
    installed: &[[u8; 32]],
    out: &mut Start,
) -> Result<(), Cut> {
    let cell = cells[me];
    let arm_at = cell.slot(c, 0);
    let Some(r) = armed(c, mem, cell) else {
        if read_slot(c, mem, arm_at).is_some() {
            mem.store(arm_at, 0)?;
        }
        return Ok(());
    };
    let locked = verdict(c, mem, cells, &r.id).unwrap_or(watchdog);
    let tripped = FuseRec {
        mark: Mark::Tripped,
        ..r
    };
    if !locked {
        return mem.store(arm_at, 0);
    }
    if !installed.contains(&r.id) {
        out.events.push(FuseEvent::Stale(r.id));
        return mem.store(arm_at, 0);
    }
    if append(c, mem, cell, &tripped)? {
        out.events.push(FuseEvent::Tripped(r.id));
        if !out.disabled.contains(&r.id) {
            out.disabled.push(r.id);
        }
        return mem.store(arm_at, 0);
    }
    out.events.push(FuseEvent::Full(r.id));
    out.can_arm = false;
    out.complete = false;
    Ok(())
}

/// Keeps or frees each record slot of the layer's own cell.
fn tidy<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cells: &[Cell],
    me: usize,
    installed: &[[u8; 32]],
    out: &mut Start,
) -> Result<(), Cut> {
    let cell = cells[me];
    for i in 1..=cell.trips {
        let at = cell.slot(c, i);
        let Some(r) = read_slot(c, mem, at) else {
            continue;
        };
        let in_other =
            |mark: Mark| others(cells, me).any(|o| find(c, mem, o, mark, &r.id).is_some());
        let armed_elsewhere =
            others(cells, me).any(|o| armed(c, mem, o).is_some_and(|a| a.id == r.id));
        let free = match r.mark {
            Mark::Tripped if !installed.contains(&r.id) => {
                out.events.push(FuseEvent::Stale(r.id));
                true
            }
            Mark::Tripped
                if in_other(Mark::ClearRequest)
                    && find(c, mem, cell, Mark::Ack, &r.id).is_none() =>
            {
                // Same body and CRC: the one mark byte turns the trip into
                // the acknowledgement.
                mem.store(at, c.mark_byte(Mark::Ack))?;
                out.events.push(FuseEvent::Cleared(r.id));
                false
            }
            Mark::Tripped => {
                out.disabled.push(r.id);
                false
            }
            Mark::ClearRequest => in_other(Mark::Ack) || !in_other(Mark::Tripped),
            Mark::Ack => !in_other(Mark::ClearRequest),
            Mark::Lockup | Mark::Clean => !armed_elsewhere,
            Mark::Armed => true,
        };
        if free {
            mem.store(at, 0)?;
        }
    }
    Ok(())
}

/// A layer's start. `cells[me]` is its own cell; `watchdog` is the MCU
/// package's unacknowledged watchdog-reset flag; `installed` are the fuse
/// ids of the call sites its images now have.
pub fn start<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cells: &[Cell],
    me: usize,
    watchdog: bool,
    installed: &[[u8; 32]],
) -> Result<Start, Cut> {
    let mut out = Start {
        can_arm: true,
        complete: true,
        ..Start::default()
    };
    // Free what is done first, so a verdict or trip finds the room.
    tidy(c, mem, cells, me, installed, &mut out)?;
    // A verdict this start could not store keeps the watchdog flag set; a
    // call armed meanwhile could be misread as a lockup, so none is armed.
    let classified = classify(c, mem, cells, me, watchdog)?;
    out.can_arm &= classified;
    out.complete &= classified;
    own_call(c, mem, cells, me, watchdog, installed, &mut out)?;
    for (j, o) in cells.iter().enumerate().filter(|&(j, _)| j != me) {
        for i in 1..=o.trips {
            match read_slot(c, mem, o.slot(c, i)) {
                Some(r) if r.mark == Mark::Tripped => {
                    out.events.push(FuseEvent::Foreign(j as u8, r.id))
                }
                Some(r) if matches!(r.mark, Mark::Lockup | Mark::Clean) => out.held.push(r.id),
                _ => {}
            }
        }
    }
    Ok(out)
}

/// Before a sandboxed call (after feeding the watchdog).
pub fn arm<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cell: Cell,
    layer: u8,
    id: [u8; 32],
) -> Result<(), Cut> {
    let r = FuseRec {
        mark: Mark::Armed,
        layer,
        id,
    };
    write(c, mem, cell.slot(c, 0), &r)
}

/// After the call returned, overran or faulted: one byte.
pub fn disarm<C: FuseCodec>(c: &C, mem: &mut Mem, cell: Cell) -> Result<(), Cut> {
    mem.store(cell.slot(c, 0), 0)
}

/// The host's clear through the agent, which owns `cells[me]`. Returns
/// false when the agent's cell has no slot for a clear request.
pub fn host_clear<C: FuseCodec>(
    c: &C,
    mem: &mut Mem,
    cells: &[Cell],
    me: usize,
    layer: u8,
    id: [u8; 32],
) -> Result<bool, Cut> {
    if let Some(at) = find(c, mem, cells[me], Mark::Tripped, &id) {
        mem.store(at, 0)?;
        return Ok(true);
    }
    let elsewhere = cells
        .iter()
        .enumerate()
        .any(|(j, o)| j != me && find(c, mem, *o, Mark::Tripped, &id).is_some());
    if !elsewhere {
        return Ok(true);
    }
    let r = FuseRec {
        mark: Mark::ClearRequest,
        layer,
        id,
    };
    append(c, mem, cells[me], &r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(chain: u32) -> CallSite {
        CallSite {
            layer: LAYER_APP,
            slot: 0,
            header: [1; 32],
            chain,
            artifact: [2; 32],
        }
    }

    #[test]
    fn identity_changes_with_every_field() {
        let a = site(0);
        let ids = [
            fuse_id(&a),
            fuse_id(&CallSite { chain: 1, ..a }),
            fuse_id(&CallSite { slot: 1, ..a }),
            fuse_id(&CallSite {
                layer: LAYER_RELAY,
                ..a
            }),
            fuse_id(&CallSite {
                header: [3; 32],
                ..a
            }),
            fuse_id(&CallSite {
                artifact: [3; 32],
                ..a
            }),
        ];
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn other_resets_never_trip() {
        let cells = [Cell { base: 0, trips: 2 }];
        let id = fuse_id(&site(0));
        let mut m = Mem::new(200, 0, 1);
        arm(&FuseV1, &mut m, cells[0], LAYER_APP, id).unwrap();
        let s = start(&FuseV1, &mut m, &cells, 0, false, &[id]).unwrap();
        assert!(s.disabled.is_empty() && s.complete);
    }

    #[test]
    fn armed_at_start_trips_and_disarmed_does_not() {
        let cells = [Cell { base: 0, trips: 2 }];
        let id = fuse_id(&site(0));
        for (returned, tripped) in [(false, true), (true, false)] {
            let mut m = Mem::new(200, 0, 1);
            arm(&FuseV1, &mut m, cells[0], LAYER_APP, id).unwrap();
            if returned {
                disarm(&FuseV1, &mut m, cells[0]).unwrap();
            }
            let s = start(&FuseV1, &mut m, &cells, 0, true, &[id]).unwrap();
            assert_eq!(s.disabled.contains(&id), tripped);
        }
    }
}
