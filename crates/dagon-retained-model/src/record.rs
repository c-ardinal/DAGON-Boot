//! Boot record: per-writer append-only rings in retained memory.
//!
//! The codec is the [`RecordCodec`] trait so a production codec is checked
//! by the same harness; [`V1`] is the layout this model proves. The writer
//! ([`Writer`]) and reader ([`read`]) algorithms are layout independent.

use crate::mem::{Cut, Mem};
use dagon_format::crc32;
use std::collections::HashMap;
use std::ops::Range;

/// Largest region (VERSION 0x21 addresses it with `offset u16`).
pub const RECORD_MAX: usize = 4096;
/// Writers per record (the opener field is 4 bits).
pub const WRITERS_MAX: usize = 16;
/// First byte of a committed entry is `COMMIT_BASE | layout`.
pub const COMMIT_BASE: u8 = 0xB0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BootId {
    /// Ring index of the writer that opened the boot.
    pub opener: u8,
    /// The opener's sequence number of its first entry in that boot.
    pub boot: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// First entry of a writer's run.
    pub start: bool,
    /// First entry after the writer found no valid entry of its layout.
    pub epoch: bool,
    pub boot: BootId,
    /// The writer's `event` format number.
    pub fmt: u8,
    pub kind: u8,
    /// Writer sequence: +1 per entry, continued from the newest held entry.
    pub eseq: u32,
    pub payload: [u8; 3],
}

/// An entry layout. `append_stores` lists the byte stores of one append in
/// the order they must reach memory.
pub trait RecordCodec {
    fn layout(&self) -> u8;
    fn entry_size(&self) -> usize;
    fn append_stores(&self, writer: u8, e: &Entry) -> Vec<(usize, u8)>;
    fn decode(&self, writer: u8, slot: &[u8]) -> Option<Entry>;
}

/// Layout 1: 20-byte entries.
///
/// | off | field |
/// |---|---|
/// | 0 | commit: `0xB1` committed, anything else not an entry of layout 1 |
/// | 1 | flags: bit 0 start, bit 1 epoch, bits 2-3 zero, bits 4-7 opener |
/// | 2 | event format |
/// | 3 | kind |
/// | 4 | eseq u32 LE |
/// | 8 | boot u32 LE (opener's eseq of its first entry in the boot) |
/// | 12 | payload, 3 bytes |
/// | 15 | zero |
/// | 16 | CRC-32 (ISO-HDLC) over `[layout, writer]` then bytes 1..16 |
///
/// Append: store byte 0 = `0x00`, then bytes 1..20 in order, then byte 0 =
/// commit. Only the commit byte is stored twice, and only by its own append.
pub struct V1;

pub const V1_SIZE: usize = 20;

fn entry_crc(layout: u8, writer: u8, body: &[u8]) -> u32 {
    let mut b = Vec::with_capacity(body.len() + 2);
    b.extend_from_slice(&[layout, writer]);
    b.extend_from_slice(body);
    crc32(&b)
}

fn flags(e: &Entry) -> u8 {
    (e.start as u8) | ((e.epoch as u8) << 1) | (e.boot.opener << 4)
}

impl RecordCodec for V1 {
    fn layout(&self) -> u8 {
        1
    }

    fn entry_size(&self) -> usize {
        V1_SIZE
    }

    fn append_stores(&self, writer: u8, e: &Entry) -> Vec<(usize, u8)> {
        let mut b = [0u8; V1_SIZE];
        b[1] = flags(e);
        b[2] = e.fmt;
        b[3] = e.kind;
        b[4..8].copy_from_slice(&e.eseq.to_le_bytes());
        b[8..12].copy_from_slice(&e.boot.boot.to_le_bytes());
        b[12..15].copy_from_slice(&e.payload);
        let crc = entry_crc(1, writer, &b[1..16]);
        b[16..20].copy_from_slice(&crc.to_le_bytes());
        let mut s = vec![(0, 0)];
        s.extend((1..V1_SIZE).map(|i| (i, b[i])));
        s.push((0, COMMIT_BASE | 1));
        s
    }

    fn decode(&self, writer: u8, b: &[u8]) -> Option<Entry> {
        let crc = u32::from_le_bytes(b[16..20].try_into().ok()?);
        let ok = b[0] == COMMIT_BASE | 1
            && b[1] & 0x0C == 0
            && b[15] == 0
            && crc == entry_crc(1, writer, &b[1..16]);
        ok.then(|| Entry {
            start: b[1] & 1 != 0,
            epoch: b[1] & 2 != 0,
            boot: BootId {
                opener: b[1] >> 4,
                boot: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            },
            fmt: b[2],
            kind: b[3],
            eseq: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            payload: [b[12], b[13], b[14]],
        })
    }
}

/// Boot identity passed from one stage to the next in three registers at
/// the jump (never through retained memory, so a hung earlier boot cannot
/// leave a stale value).
pub mod handoff {
    use super::BootId;
    use dagon_format::crc32;

    pub const MAGIC: u32 = u32::from_le_bytes(*b"DGBR");

    fn check(w0: u32, w1: u32, lo: u16) -> u16 {
        let mut b = [0u8; 10];
        b[0..4].copy_from_slice(&w0.to_le_bytes());
        b[4..8].copy_from_slice(&w1.to_le_bytes());
        b[8..10].copy_from_slice(&lo.to_le_bytes());
        crc32(&b) as u16
    }

    /// `[MAGIC, boot, opener | layout << 8 | check << 16]`.
    pub fn encode(id: BootId, layout: u8) -> [u32; 3] {
        let lo = id.opener as u16 | (layout as u16) << 8;
        [
            MAGIC,
            id.boot,
            lo as u32 | (check(MAGIC, id.boot, lo) as u32) << 16,
        ]
    }

    /// Accepted only with the magic, the check, the reader's own layout and
    /// an opener that runs before `me`.
    pub fn decode(w: [u32; 3], layout: u8, me: u8) -> Option<BootId> {
        let lo = w[2] as u16;
        let ok = w[0] == MAGIC
            && (w[2] >> 16) as u16 == check(w[0], w[1], lo)
            && (lo >> 8) as u8 == layout
            && (lo as u8) < me;
        ok.then_some(BootId {
            opener: lo as u8,
            boot: w[1],
        })
    }
}

/// One writer's ring: `slots` entries from `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring {
    pub base: usize,
    pub slots: usize,
}

/// The record's place in memory (`[boot.record] at` and `rings`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub region: Range<usize>,
    /// Ring `i` belongs to writer `i`, in boot order.
    pub rings: Vec<Ring>,
}

impl Geometry {
    pub fn ring_bytes(&self, w: usize, size: usize) -> Range<usize> {
        let r = self.rings[w];
        r.base..r.base + r.slots * size
    }

    /// The definition validator's checks.
    pub fn validate(&self, size: usize) -> Result<(), &'static str> {
        if self.region.len() > RECORD_MAX {
            return Err("region larger than RECORD_MAX");
        }
        if self.rings.is_empty() || self.rings.len() > WRITERS_MAX {
            return Err("writer count");
        }
        for (i, r) in self.rings.iter().enumerate() {
            let b = self.ring_bytes(i, size);
            if r.slots < 2 || b.start < self.region.start || b.end > self.region.end {
                return Err("ring outside the region or under 2 slots");
            }
            let overlap = (0..i).any(|j| {
                let o = self.ring_bytes(j, size);
                o.start < b.end && b.start < o.end
            });
            if overlap {
                return Err("rings overlap");
            }
        }
        Ok(())
    }
}

/// Valid entries of one ring with their slot index.
pub fn scan<C: RecordCodec>(c: &C, mem: &[u8], ring: Ring, w: u8) -> Vec<(usize, Entry)> {
    let n = c.entry_size();
    (0..ring.slots)
        .filter_map(|s| {
            let a = ring.base + s * n;
            c.decode(w, &mem[a..a + n]).map(|e| (s, e))
        })
        .collect()
}

/// Index of the newest entry: the one with the most entries within
/// `slots` sequence numbers behind it (ties: higher slot). For a ring the
/// writer filled this is the entry with the largest sequence number, also
/// across wrap; foreign bytes can only pick another slot of the same ring.
fn newest(es: &[(usize, Entry)], slots: usize) -> Option<usize> {
    let score = |x: &Entry| {
        es.iter()
            .filter(|(_, y)| (x.eseq.wrapping_sub(y.eseq) as usize) < slots)
            .count()
    };
    (0..es.len()).max_by_key(|&i| (score(&es[i].1), es[i].0))
}

/// FNV-1a, 32 bits: the start of a fresh sequence from the region's raw
/// bytes. Not a CRC: the region is made of CRC-32-protected entries, and
/// CRC-32 over such a region is nearly independent of their contents.
pub fn fnv1a(b: &[u8]) -> u32 {
    b.iter().fold(0x811C_9DC5, |h: u32, &x| {
        (h ^ x as u32).wrapping_mul(0x0100_0193)
    })
}

/// One stage's appender for one run.
pub struct Writer<'c, C: RecordCodec> {
    codec: &'c C,
    ring: Ring,
    me: u8,
    cursor: usize,
    next: u32,
    boot: BootId,
    fresh: bool,
    started: bool,
}

impl<'c, C: RecordCodec> Writer<'c, C> {
    /// Reads only the writer's own ring. `handoff` is the register triple
    /// the previous stage passed (`None` when nothing ran before);
    /// `entropy` is the MCU package's start-up word (0 where none).
    pub fn open(
        codec: &'c C,
        mem: &Mem,
        geo: &Geometry,
        me: u8,
        handoff: Option<[u32; 3]>,
        entropy: u32,
    ) -> Self {
        let ring = geo.rings[me as usize];
        let es = scan(codec, mem.bytes(), ring, me);
        let (cursor, next, fresh) = match newest(&es, ring.slots) {
            Some(i) => (
                (es[i].0 + 1) % ring.slots,
                es[i].1.eseq.wrapping_add(1),
                false,
            ),
            None => (0, fnv1a(&mem.bytes()[geo.region.clone()]) ^ entropy, true),
        };
        let linked = handoff.and_then(|h| handoff::decode(h, codec.layout(), me));
        Writer {
            codec,
            ring,
            me,
            cursor,
            next,
            boot: linked.unwrap_or(BootId {
                opener: me,
                boot: next,
            }),
            fresh,
            started: false,
        }
    }

    pub fn boot(&self) -> BootId {
        self.boot
    }

    /// True when the ring held no valid entry of this layout at open.
    pub fn fresh(&self) -> bool {
        self.fresh
    }

    /// Address of the slot the next append writes.
    pub fn next_slot(&self) -> usize {
        self.ring.base + self.cursor * self.codec.entry_size()
    }

    pub fn next_eseq(&self) -> u32 {
        self.next
    }

    pub fn handoff(&self) -> [u32; 3] {
        handoff::encode(self.boot, self.codec.layout())
    }

    pub fn append(
        &mut self,
        mem: &mut Mem,
        fmt: u8,
        kind: u8,
        payload: [u8; 3],
    ) -> Result<Entry, Cut> {
        let e = Entry {
            start: !self.started,
            epoch: self.fresh && !self.started,
            boot: self.boot,
            fmt,
            kind,
            eseq: self.next,
            payload,
        };
        let at = self.next_slot();
        for (off, v) in self.codec.append_stores(self.me, &e) {
            mem.store(at + off, v)?;
        }
        self.started = true;
        self.cursor = (self.cursor + 1) % self.ring.slots;
        self.next = self.next.wrapping_add(1);
        Ok(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replayed {
    pub writer: u8,
    pub entry: Entry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootReplay {
    pub id: BootId,
    /// No held data orders this boot against at least one other boot.
    pub uncertain: bool,
    /// Writer order, then sequence order.
    pub events: Vec<Replayed>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    /// Nothing of the writer is held.
    Empty { writer: u8 },
    /// Entries the writer appended before `from` are not held; with
    /// `epoch_start` the oldest held entry opened the writer's sequence.
    Window {
        writer: u8,
        from: u32,
        epoch_start: bool,
    },
    /// Sequence numbers `from..=to` are not held.
    Gap { writer: u8, from: u32, to: u32 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    /// Oldest first.
    pub boots: Vec<BootReplay>,
    pub losses: Vec<Loss>,
    /// Slots that look committed by this layout but fail a check.
    pub invalid: usize,
    /// Slots whose first byte is the commit of another layout.
    pub foreign: usize,
}

impl Replay {
    pub fn events(&self) -> impl Iterator<Item = &Replayed> {
        self.boots.iter().flat_map(|b| b.events.iter())
    }
}

struct Run {
    boot: BootId,
    events: Vec<Replayed>,
}

/// One writer's held entries oldest first, its runs and its loss statements.
fn writer_runs<C: RecordCodec>(c: &C, mem: &[u8], ring: Ring, w: u8, out: &mut Replay) -> Vec<Run> {
    let mut es = scan(c, mem, ring, w);
    let Some(top) = newest(&es, ring.slots).map(|i| es[i].1.eseq) else {
        out.losses.push(Loss::Empty { writer: w });
        return Vec::new();
    };
    let before = es.len();
    es.retain(|(_, e)| (top.wrapping_sub(e.eseq) as usize) < ring.slots);
    es.sort_by_key(|(_, e)| std::cmp::Reverse(top.wrapping_sub(e.eseq)));
    es.dedup_by_key(|(_, e)| e.eseq);
    out.invalid += before - es.len();
    let first = es[0].1;
    out.losses.push(Loss::Window {
        writer: w,
        from: first.eseq,
        epoch_start: first.epoch && first.start,
    });
    for p in es.windows(2) {
        let (a, b) = (p[0].1.eseq, p[1].1.eseq);
        if b.wrapping_sub(a) > 1 {
            out.losses.push(Loss::Gap {
                writer: w,
                from: a.wrapping_add(1),
                to: b.wrapping_sub(1),
            });
        }
    }
    let mut runs: Vec<Run> = Vec::new();
    for (_, e) in es {
        let r = Replayed {
            writer: w,
            entry: e,
        };
        match runs.last_mut() {
            Some(run) if !e.start && run.boot == e.boot => run.events.push(r),
            _ => runs.push(Run {
                boot: e.boot,
                events: vec![r],
            }),
        }
    }
    runs
}

/// Reads the record with one layout. Pure: the agent's only access.
pub fn read<C: RecordCodec>(c: &C, mem: &[u8], geo: &Geometry) -> Replay {
    let mut out = Replay::default();
    let n = c.entry_size();
    let mut per_writer = Vec::with_capacity(geo.rings.len());
    for (w, ring) in geo.rings.iter().enumerate() {
        out.foreign += (0..ring.slots)
            .map(|s| mem[ring.base + s * n])
            .filter(|&b| b & 0xF0 == COMMIT_BASE && b != COMMIT_BASE | c.layout())
            .count();
        let runs = writer_runs(c, mem, *ring, w as u8, &mut out);
        let chain: Vec<BootId> = runs.iter().map(|r| r.boot).collect();
        let top = runs
            .last()
            .and_then(|r| r.events.last())
            .map(|e| e.entry.eseq);
        let mut by_boot: HashMap<BootId, Vec<Replayed>> = HashMap::new();
        for r in runs {
            by_boot.entry(r.boot).or_default().extend(r.events);
        }
        per_writer.push(View {
            chain,
            by_boot,
            top,
            slots: ring.slots as u32,
        });
    }
    out.boots = merge(per_writer);
    out
}

struct View {
    /// Boots of the writer's runs, oldest first.
    chain: Vec<BootId>,
    by_boot: HashMap<BootId, Vec<Replayed>>,
    /// Newest held sequence number.
    top: Option<u32>,
    slots: u32,
}

/// Where a boot its opener's ring no longer holds sits among the boots the
/// ring holds, from its number: up to `slots` above the newest held entry
/// it is newer (newest entries destroyed by foreign bytes), inside the held
/// range it sits by number, anywhere else it is older (overwritten).
fn place_stale(b: BootId, own: &View, nodes: &[BootId], edge: &mut [Vec<bool>]) {
    let ix = |x: &BootId| nodes.iter().position(|y| y == x).unwrap();
    let Some(top) = own.top else {
        return;
    };
    let ahead = b.boot.wrapping_sub(top);
    let back = top.wrapping_sub(b.boot);
    for h in own.chain.iter().filter(|h| h.opener == b.opener) {
        let h_back = top.wrapping_sub(h.boot);
        let newer = if ahead != 0 && ahead <= own.slots {
            true
        } else if back < own.slots {
            h_back > back
        } else {
            false
        };
        if newer {
            edge[ix(h)][ix(&b)] = true;
        } else {
            edge[ix(&b)][ix(h)] = true;
        }
    }
}

/// Orders boots by every writer's own sequence order, a boot its opener no
/// longer holds by [`place_stale`]. Boots no chain orders against each
/// other are marked uncertain.
fn merge(per: Vec<View>) -> Vec<BootReplay> {
    let mut nodes: Vec<BootId> = Vec::new();
    for View { chain, .. } in &per {
        for b in chain {
            if !nodes.contains(b) {
                nodes.push(*b);
            }
        }
    }
    let ix = |b: &BootId| nodes.iter().position(|x| x == b).unwrap();
    let k = nodes.len();
    let mut edge = vec![vec![false; k]; k];
    for View { chain, .. } in &per {
        for p in chain.windows(2) {
            if p[0] != p[1] {
                edge[ix(&p[0])][ix(&p[1])] = true;
            }
        }
    }
    for b in &nodes {
        if let Some(own) = per.get(b.opener as usize) {
            if !own.chain.contains(b) {
                place_stale(*b, own, &nodes, &mut edge);
            }
        }
    }
    let reach = closure(&edge);
    let uncertain: Vec<bool> = (0..k)
        .map(|i| (0..k).any(|j| i != j && reach[i][j] == reach[j][i]))
        .collect();
    let order = topo(&edge, &nodes);
    order
        .into_iter()
        .map(|i| {
            let id = nodes[i];
            let mut events: Vec<Replayed> = per
                .iter()
                .filter_map(|v| v.by_boot.get(&id))
                .flatten()
                .copied()
                .collect();
            events.sort_by_key(|r| r.writer);
            BootReplay {
                id,
                uncertain: uncertain[i],
                events,
            }
        })
        .collect()
}

fn closure(edge: &[Vec<bool>]) -> Vec<Vec<bool>> {
    let k = edge.len();
    let mut r = edge.to_vec();
    for m in 0..k {
        let via = r[m].clone();
        for row in r.iter_mut().filter(|row| row[m]) {
            row.iter_mut().zip(&via).for_each(|(x, &v)| *x |= v);
        }
    }
    r
}

/// Kahn's order; among free boots, later openers' boots first, then first
/// seen. A cycle (only from foreign bytes) is broken at the same key.
fn topo(edge: &[Vec<bool>], nodes: &[BootId]) -> Vec<usize> {
    let k = edge.len();
    let mut done = vec![false; k];
    let mut out = Vec::with_capacity(k);
    let key = |i: usize| (std::cmp::Reverse(nodes[i].opener), i);
    while out.len() < k {
        let free = (0..k)
            .filter(|&i| !done[i] && (0..k).all(|j| done[j] || !edge[j][i]))
            .min_by_key(|&i| key(i));
        let pick = free.unwrap_or_else(|| {
            (0..k)
                .filter(|&i| !done[i])
                .min_by_key(|&i| key(i))
                .unwrap()
        });
        done[pick] = true;
        out.push(pick);
    }
    out
}

/// VERSION 0x20: `layout, boots, flags (bit 0 uncertain, bit 1 loss),
/// newest boot opener u8, boot u32 LE, count u8`, then per writer of the
/// newest boot its last entry: `writer, fmt, kind, payload[3]`.
pub fn summary(layout: u8, r: &Replay) -> Vec<u8> {
    let lossy = r.losses.iter().any(|l| match l {
        Loss::Gap { .. } => true,
        Loss::Window { epoch_start, .. } => !epoch_start,
        Loss::Empty { .. } => false,
    });
    let uncertain = r.boots.iter().any(|b| b.uncertain);
    let mut s = vec![
        layout,
        r.boots.len().min(255) as u8,
        uncertain as u8 | (lossy as u8) << 1,
    ];
    if let Some(b) = r.boots.last() {
        s.push(b.id.opener);
        s.extend_from_slice(&b.id.boot.to_le_bytes());
        let mut last: Vec<&Replayed> = Vec::new();
        for e in &b.events {
            match last.last_mut() {
                Some(l) if l.writer == e.writer => *l = e,
                _ => last.push(e),
            }
        }
        s.push(last.len() as u8);
        for e in last {
            s.extend_from_slice(&[e.writer, e.entry.fmt, e.entry.kind]);
            s.extend_from_slice(&e.entry.payload);
        }
    }
    s
}

/// VERSION 0x21 fixed overhead: status, TLV count, id, len, offset u16.
pub const VERSION_PAGE_OVERHEAD: usize = 6;

/// `min(255 - 2, max_payload - overhead)`.
pub const fn record_page(max_payload: usize) -> usize {
    let a = 255 - 2;
    let b = max_payload - VERSION_PAGE_OVERHEAD;
    if a < b {
        a
    } else {
        b
    }
}

/// Reads the region page by page as the host does and joins the pages.
pub fn paged_read(region: &[u8], page: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(region.len());
    let mut off = 0usize;
    while off < region.len() {
        let len = page.min(region.len() - off);
        let offset = u16::try_from(off).expect("offset fits u16");
        out.extend_from_slice(&region[offset as usize..offset as usize + len]);
        off += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo() -> Geometry {
        Geometry {
            region: 0..400,
            rings: vec![
                Ring { base: 0, slots: 4 },
                Ring {
                    base: 100,
                    slots: 8,
                },
            ],
        }
    }

    #[test]
    fn v1_round_trips_and_binds_the_writer() {
        let e = Entry {
            start: true,
            epoch: false,
            boot: BootId {
                opener: 3,
                boot: 77,
            },
            fmt: 1,
            kind: 9,
            eseq: 0xFFFF_FFFF,
            payload: [1, 2, 3],
        };
        let mut b = [0u8; V1_SIZE];
        for (o, v) in V1.append_stores(5, &e) {
            b[o] = v;
        }
        assert_eq!(V1.decode(5, &b), Some(e));
        assert_eq!(V1.decode(4, &b), None);
    }

    #[test]
    fn handoff_refuses_other_layouts_and_later_openers() {
        let id = BootId { opener: 0, boot: 5 };
        let w = handoff::encode(id, 1);
        assert_eq!(handoff::decode(w, 1, 1), Some(id));
        assert_eq!(handoff::decode(w, 2, 1), None);
        assert_eq!(handoff::decode(w, 1, 0), None);
    }

    #[test]
    fn linked_boot_groups_two_writers() {
        let g = geo();
        let mut m = Mem::new(400, 0, 1);
        let mut s0 = Writer::open(&V1, &m, &g, 0, None, 0);
        s0.append(&mut m, 1, 1, [0; 3]).unwrap();
        let h = s0.handoff();
        let mut bc = Writer::open(&V1, &m, &g, 1, Some(h), 0);
        bc.append(&mut m, 1, 2, [0; 3]).unwrap();
        let r = read(&V1, m.bytes(), &g);
        assert_eq!(r.boots.len(), 1);
        assert_eq!(r.boots[0].events.len(), 2);
        assert!(!r.boots[0].uncertain);
    }

    #[test]
    fn page_size_per_link() {
        use dagon_proto::max_payload;
        assert_eq!(record_page(max_payload::I2C), 250);
        assert_eq!(record_page(max_payload::UART), 253);
        assert_eq!(record_page(max_payload::USB), 253);
    }
}
