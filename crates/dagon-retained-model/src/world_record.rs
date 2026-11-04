//! Boot record scenarios: boots of stage0 and the boot core in any version
//! order, torn writes after any byte, resets and power loss, compressed
//! runs of up to 2^32 entries, foreign bytes, and an app that replays.
//!
//! The oracle keeps every committed append with the bytes it left in
//! memory; an append is held while those bytes are intact. Properties:
//! P1 observability, P2 torn-write safety, P3 writer independence,
//! P4 identity uniqueness, P5 ordering (never a newer boot before an
//! older one; pairs the reader cannot order are marked uncertain),
//! P6 current-boot attribution,
//! P7 loss accounting, P9 versioning, P10 size and paging,
//! P11 at-least-once replay with consumer de-duplication.
//!
//! One file on purpose: every check reads the oracle's private state.

use crate::harness::{Violation, World};
use crate::mem::{Cut, Mem, Survives};
use crate::record::{
    self, paged_read, record_page, BootId, Entry, Geometry, HandOff, Loss, RecordCodec, Replay,
    Ring, Writer, RECORD_MAX, V1 as RecV1,
};
use crate::skew::{Weak, V2 as RecV2};
use dagon_proto::max_payload;
use dagon_sim::SimRng;
use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ver {
    /// No such stage runs.
    Absent,
    /// A stage built before the record: writes nothing, passes registers
    /// with arbitrary content.
    Old,
    V1,
    V2,
}
pub use Ver::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Hands off to the next stage (the boot core: jumps to the app).
    Next,
    /// Halts for good after its last event.
    Halt,
    /// Stops without a further event (a hang ended by the watchdog).
    Hang,
}
pub use End::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub ver: Ver,
    /// Events appended in this run.
    pub events: u8,
    pub end: End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// One boot. `cut`: the run stops after that many byte stores.
    /// `app`: the reader layout of the app's agent (`Old`/`Absent`: none).
    /// `power`: the boot ends in power loss instead of a reset.
    /// `more`: the stages after the boot core, one per further writer.
    Boot {
        s0: Stage,
        bc: Stage,
        more: Vec<Stage>,
        app: Ver,
        cut: Option<u32>,
        power: bool,
    },
    /// `boots` boots in compressed form: two first-writer entries each
    /// and, `linked`, three linked second-writer entries each.
    FastForward { boots: u64, linked: bool },
    /// Random bytes inside one writer's ring.
    Fuzz { writer: u8, at: u16, len: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub survives: Survives,
    /// The definition has a stage0 ring (writer 0); the boot core is the
    /// next writer.
    pub stage0: bool,
    pub s0_bytes: usize,
    pub bc_bytes: usize,
    /// Rings of the writers after the boot core, in boot order.
    pub more: Vec<usize>,
    /// Reserved bytes between the rings and around the region.
    pub gap: usize,
    /// Negative control: `V1` stages and readers use [`Weak`] instead.
    pub weak: bool,
}

impl Config {
    fn ranges(&self) -> Vec<Range<usize>> {
        let mut at = self.gap;
        let mut v = Vec::new();
        if self.stage0 {
            v.push(at..at + self.s0_bytes);
            at += self.s0_bytes + self.gap;
        }
        v.push(at..at + self.bc_bytes);
        at += self.bc_bytes + self.gap;
        for &m in &self.more {
            v.push(at..at + m);
            at += m + self.gap;
        }
        v
    }

    fn region(&self) -> Range<usize> {
        let r = self.ranges();
        r[0].start..r.last().unwrap().end
    }

    fn mem_len(&self) -> usize {
        self.region().end + self.gap
    }

    pub fn geometry(&self, entry: usize) -> Geometry {
        Geometry {
            region: self.region(),
            rings: self
                .ranges()
                .iter()
                .map(|r| Ring {
                    base: r.start,
                    slots: r.len() / entry,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    Own,
    Foreign,
    Power,
}

#[derive(Clone, Debug)]
struct Rec {
    true_boot: u64,
    writer: u8,
    layout: u8,
    addr: usize,
    bytes: Vec<u8>,
    entry: Entry,
    dropped: Option<Cause>,
    accounted: bool,
    /// Appends of this writer and layout before this one.
    ordinal: u64,
    /// Appends of the boot's opener (same layout) when this was appended.
    opener_count: u64,
    /// The opener's sequence (restarts from an empty ring) at that time.
    opener_epoch: u64,
    /// The writer's appends when this was found destroyed by other bytes.
    lost_at_count: u64,
}

/// Who wrote during a phase, for the cause of a lost append.
#[derive(Clone, Copy)]
enum Phase {
    Writer(u8, u8),
    Foreign,
    Power,
}

struct Sim<'a> {
    cfg: &'a Config,
    mem: Mem,
    rng: SimRng,
    recs: Vec<Rec>,
    true_boot: u64,
    consumer: HashMap<(u8, u8, u32, u32), Entry>,
    step: usize,
    appended: HashMap<(u8, u8), u64>,
    /// Restarts from an empty ring per writer and layout.
    epochs: HashMap<(u8, u8), u64>,
    /// Opens per writer and layout.
    opens: HashMap<(u8, u8), u64>,
    /// Boots whose hand-off chain an old stage in the middle cut.
    broken: Vec<u64>,
    /// Append count at each writer's last jump.
    skips: HashMap<(u8, u8), u64>,
    /// Numbers a writer may reuse: its newest entries were destroyed again
    /// within one lap of its ring (writer, layout, from, to).
    excused: Vec<(u8, u8, u32, u32)>,
}

fn committed<C: RecordCodec>(c: &C, w: u8, e: &Entry) -> Vec<u8> {
    let mut b = vec![0u8; c.entry_size()];
    for (o, v) in c.append_stores(w, e) {
        b[o] = v;
    }
    b
}

impl<'a> Sim<'a> {
    fn new(cfg: &'a Config, seed: u64) -> Self {
        let mem = match cfg.survives {
            Survives::ResetRandom => Mem::random(cfg.mem_len(), seed),
            _ => Mem::new(cfg.mem_len(), 0, seed),
        };
        Sim {
            cfg,
            mem,
            rng: SimRng::new(seed ^ 0x5EED),
            recs: Vec::new(),
            true_boot: 0,
            consumer: HashMap::new(),
            step: 0,
            appended: HashMap::new(),
            epochs: HashMap::new(),
            opens: HashMap::new(),
            skips: HashMap::new(),
            broken: Vec::new(),
            excused: Vec::new(),
        }
    }

    fn fail(&self, p: &'static str, d: impl Into<String>) -> Violation {
        Violation::new(p, self.step, d)
    }

    fn held(&self, r: &Rec) -> bool {
        self.mem.bytes()[r.addr..r.addr + r.bytes.len()] == r.bytes[..]
    }

    fn refresh(&mut self, phase: Phase) {
        for i in 0..self.recs.len() {
            if self.recs[i].dropped.is_none() && !self.held(&self.recs[i]) {
                let r = &self.recs[i];
                let cause = match phase {
                    Phase::Writer(w, l) if w == r.writer && l == r.layout => Cause::Own,
                    Phase::Writer(..) | Phase::Foreign => Cause::Foreign,
                    Phase::Power => Cause::Power,
                };
                let opens = self
                    .appended
                    .get(&(r.writer, r.layout))
                    .copied()
                    .unwrap_or(0);
                self.recs[i].dropped = Some(cause);
                self.recs[i].lost_at_count = opens;
            }
        }
    }

    /// An append at `addr` replaces whatever was committed there, even with
    /// identical bytes (same slot and number after 2^32 appends).
    fn supersede(&mut self, addr: usize, w: u8, layout: u8) {
        let same = |r: &Rec| r.addr == addr && r.writer == w && r.layout == layout;
        for r in self
            .recs
            .iter_mut()
            .filter(|r| same(r) && r.dropped.is_none())
        {
            r.dropped = Some(Cause::Own);
        }
    }

    /// Advances the append count of a writer and layout; returns the old.
    fn count(&mut self, w: u8, layout: u8, n: u64) -> u64 {
        let c = self.appended.entry((w, layout)).or_insert(0);
        *c += n;
        *c - n
    }

    /// The MCU package's start-up word: different at every start (the
    /// region alone can return to an earlier state, e.g. after another
    /// layout's writer cleared a ring).
    fn entropy(&mut self) -> u32 {
        self.rng.next_u64() as u32
    }

    /// Runs one recording stage; `Ok(Some(_))` is the hand-off to the next.
    fn stage<C: RecordCodec>(
        &mut self,
        c: &C,
        w: u8,
        st: Stage,
        handoff: Option<HandOff>,
    ) -> Result<Result<Option<HandOff>, Cut>, Violation> {
        let geo = self.cfg.geometry(c.entry_size());
        self.mem.allow(geo.ring_bytes(w as usize, c.entry_size()));
        let ent = self.entropy();
        let mut wr = Writer::open(c, &self.mem, &geo, w, handoff, ent);
        self.opened(w, c.layout(), geo.rings[w as usize].slots, &wr)?;
        if wr.fresh() {
            let own_held = self
                .recs
                .iter()
                .any(|r| r.writer == w && r.layout == c.layout() && r.dropped.is_none());
            if own_held {
                return Err(self.fail("P4", "writer restarted its sequence over held entries"));
            }
        }
        for _ in 0..st.events {
            let addr = wr.next_slot();
            let (kind, p) = (self.rng.byte(), self.rng.next_u64().to_le_bytes());
            match wr.append(&mut self.mem, 1, kind, [p[0], p[1], p[2]]) {
                Ok(e) => {
                    self.check_unique(w, c.layout(), e.eseq)?;
                    let ordinal = self.count(w, c.layout(), 1);
                    let opener_count = self.count(e.boot.opener, c.layout(), 0);
                    let opener_epoch = self.epoch(e.boot.opener, c.layout());
                    self.supersede(addr, w, c.layout());
                    self.recs.push(Rec {
                        true_boot: self.true_boot,
                        writer: w,
                        layout: c.layout(),
                        addr,
                        bytes: committed(c, w, &e),
                        entry: e,
                        dropped: None,
                        accounted: false,
                        ordinal,
                        opener_count,
                        opener_epoch,
                        lost_at_count: 0,
                    });
                }
                Err(cut) => {
                    self.refresh(Phase::Writer(w, c.layout()));
                    return Ok(Err(cut));
                }
            }
        }
        self.refresh(Phase::Writer(w, c.layout()));
        Ok(Ok((st.end == Next).then(|| wr.handoff())))
    }

    fn is_excused(&self, w: u8, l: u8, eseq: u32) -> bool {
        self.excused.iter().any(|&(ew, el, from, to)| {
            ew == w && el == l && eseq.wrapping_sub(from) <= to.wrapping_sub(from)
        })
    }

    fn epoch(&self, w: u8, l: u8) -> u64 {
        self.epochs.get(&(w, l)).copied().unwrap_or(0)
    }

    /// P4: a writer that held anything continues ahead of every number it
    /// ever appended in its sequence (after destroyed entries too); one that
    /// held nothing starts a new sequence. Stated limit: when its newest
    /// entries are destroyed again within one lap of its ring, the jump may
    /// fall short; those numbers are recorded as excused.
    fn opened<C: RecordCodec>(
        &mut self,
        w: u8,
        l: u8,
        slots: usize,
        wr: &Writer<C>,
    ) -> Result<(), Violation> {
        *self.opens.entry((w, l)).or_insert(0) += 1;
        let now = self.count(w, l, 0);
        let mut repeated = false;
        if wr.skipped() {
            repeated = self
                .skips
                .insert((w, l), now)
                .is_some_and(|prev| now - prev < slots as u64);
        }
        if wr.fresh() {
            *self.epochs.entry((w, l)).or_insert(0) += 1;
            return Ok(());
        }
        let last = self
            .recs
            .iter()
            .filter(|x| x.writer == w && x.layout == l)
            .max_by_key(|x| x.ordinal);
        if let Some(last) = last {
            let d = wr.next_eseq().wrapping_sub(last.entry.eseq);
            if (d == 0 || d >= record::SPAN) && repeated {
                let to = last.entry.eseq;
                self.excused.push((w, l, wr.next_eseq(), to));
            } else if d == 0 || d >= record::SPAN {
                return Err(self.fail("P4", format!("writer {w} did not continue its sequence")));
            }
        }
        Ok(())
    }

    /// P4: a new append never takes the identity of a held one.
    fn check_unique(&self, w: u8, layout: u8, eseq: u32) -> Result<(), Violation> {
        let dup = self.recs.iter().any(|r| {
            r.writer == w && r.layout == layout && r.entry.eseq == eseq && r.dropped.is_none()
        });
        if dup {
            return Err(self.fail("P4", format!("identity ({w}, {eseq}) appended twice")));
        }
        Ok(())
    }

    fn run_stage(
        &mut self,
        w: u8,
        st: Stage,
        handoff: Option<HandOff>,
    ) -> Result<Result<Option<HandOff>, Cut>, Violation> {
        match st.ver {
            V1 if self.cfg.weak => self.stage(&Weak, w, st, handoff),
            V1 => self.stage(&RecV1, w, st, handoff),
            V2 => self.stage(&RecV2, w, st, handoff),
            Old => {
                let r = &mut self.rng;
                let words = [
                    r.next_u64() as u32,
                    r.next_u64() as u32,
                    r.next_u64() as u32,
                    r.next_u64() as u32,
                ];
                Ok(Ok((st.end == Next).then_some(words)))
            }
            Absent => Ok(Ok((st.end == Next).then_some([0; 4]))),
        }
    }

    fn boot(&mut self, op: &Op) -> Result<(), Violation> {
        let Op::Boot {
            s0,
            bc,
            more,
            app,
            cut,
            power,
        } = op
        else {
            unreachable!()
        };
        self.true_boot += 1;
        self.mem.cut_after(cut.map(u64::from));
        let mut stages = Vec::new();
        if self.cfg.stage0 {
            stages.push(*s0);
        }
        stages.push(*bc);
        stages.extend(more.iter().take(self.cfg.more.len()).copied());
        let mut handoff = None;
        let mut alive = true;
        for (w, st) in stages.iter().enumerate() {
            // A stage that predates the record in the middle, or stages of
            // two record layouts, can cut the hand-off chain: a later stage
            // then opens the boot again (stated: the boot may be split).
            let layouts: Vec<Ver> = stages
                .iter()
                .map(|s| s.ver)
                .filter(|v| matches!(v, V1 | V2))
                .collect();
            let mixed = layouts.windows(2).any(|p| p[0] != p[1]);
            if (st.ver == Old && w > 0) || mixed {
                self.broken.push(self.true_boot);
            }
            match self.run_stage(w as u8, *st, handoff)? {
                // No such stage: the previous one jumps to the next directly.
                Ok(Some(_)) if st.ver == Absent => {}
                Ok(Some(h)) => handoff = Some(h),
                Ok(None) | Err(_) => {
                    alive = false;
                    break;
                }
            }
        }
        self.mem.cut_after(None);
        self.mem.allow(0..0);
        if alive {
            match app {
                V1 if self.cfg.weak => self.replay(&Weak)?,
                V1 => self.replay(&RecV1)?,
                V2 => self.replay(&RecV2)?,
                Old | Absent => {}
            }
        }
        if *power {
            self.mem.power_loss(self.cfg.survives);
            self.refresh(Phase::Power);
        }
        Ok(())
    }

    fn rec_of(&self, layout: u8, w: u8, e: &Entry) -> Option<usize> {
        self.recs.iter().position(|r| {
            r.layout == layout && r.writer == w && r.entry == *e && r.dropped.is_none()
        })
    }

    fn replay<C: RecordCodec>(&mut self, c: &C) -> Result<(), Violation> {
        let geo = self.cfg.geometry(c.entry_size());
        let r = record::read(c, self.mem.bytes(), &geo);
        let l = c.layout();
        // P2 / P9: every replayed event is one held append of this layout.
        let mut idx = Vec::new();
        for ev in r.events() {
            match self.rec_of(l, ev.writer, &ev.entry) {
                Some(i) => idx.push(i),
                None => return Err(self.fail("P2", format!("not appended: {ev:?}"))),
            }
        }
        // P4: no identity twice in one replay.
        let mut ids: Vec<(u8, u32)> = r.events().map(|e| (e.writer, e.entry.eseq)).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|p| p[0] == p[1]) {
            return Err(self.fail("P4", "identity repeated in one replay"));
        }
        // P1: every held append of this layout is replayed.
        for (i, rec) in self.recs.iter().enumerate() {
            if rec.layout == l && rec.dropped.is_none() && !idx.contains(&i) {
                return Err(self.fail("P1", format!("held, not replayed: {rec:?}")));
            }
        }
        self.check_losses(&r, l)?;
        self.check_order(&r, l)?;
        self.check_size(&r, l, &geo)?;
        self.consume(&r, l)?;
        for i in idx {
            self.recs[i].accounted = true;
        }
        Ok(())
    }

    /// P7: an append lost before it was replayed is covered by a loss
    /// statement: at once for the writer's own overwrite; for a loss to
    /// other bytes, once an entry the writer appended later is held.
    fn check_losses(&mut self, r: &Replay, l: u8) -> Result<(), Violation> {
        for i in 0..self.recs.len() {
            let rec = &self.recs[i];
            if rec.layout != l || rec.accounted || rec.dropped.is_none() {
                continue;
            }
            // Other bytes' losses are stated once an entry the writer
            // appended after the loss is held (its jump makes the gap).
            let later_held = self.recs.iter().any(|x| {
                x.writer == rec.writer
                    && x.layout == l
                    && x.dropped.is_none()
                    && x.ordinal >= rec.lost_at_count
            });
            let due = match rec.dropped {
                Some(Cause::Own) => true,
                Some(Cause::Foreign) => later_held,
                _ => false,
            };
            let excused = self.is_excused(rec.writer, l, rec.entry.eseq);
            if due && !excused && !self.covered(r, rec) {
                return Err(self.fail("P7", format!("lost without a marker: {rec:?}")));
            }
            self.recs[i].accounted = true;
        }
        Ok(())
    }

    fn covered(&self, r: &Replay, rec: &Rec) -> bool {
        let e = rec.entry.eseq;
        r.losses.iter().any(|loss| match *loss {
            Loss::Empty { writer } => writer == rec.writer,
            Loss::Gap { writer, from, to } => {
                writer == rec.writer && e.wrapping_sub(from) <= to.wrapping_sub(from)
            }
            Loss::Window { writer, from, .. } => {
                if writer != rec.writer {
                    return false;
                }
                // "Appended before the entry `from`": oracle append order.
                let me = self.recs.iter().position(|x| std::ptr::eq(x, rec));
                let oldest = self.recs.iter().position(|x| {
                    x.writer == writer
                        && x.layout == rec.layout
                        && x.entry.eseq == from
                        && x.dropped.is_none()
                });
                matches!((me, oldest), (Some(a), Some(b)) if a < b)
            }
        })
    }

    /// P4: a boot number is held for one boot only.
    fn reused(&self, id: BootId, l: u8) -> Result<(), Violation> {
        let mut boots: Vec<u64> = self
            .recs
            .iter()
            .filter(|x| x.layout == l && x.entry.boot == id && x.dropped.is_none())
            .map(|x| x.true_boot)
            .collect();
        boots.sort_unstable();
        boots.dedup();
        if boots.len() < 2 {
            return Ok(());
        }
        Err(self.fail("P4", format!("boot number {id:?} held for two boots")))
    }

    /// P5 and P6. P5 per pair: a misordered pair is a pair the record does
    /// not order; a pair the record does not order has different openers,
    /// or one opener in two of its sequences, or numbers of one opener at
    /// least 2^30 apart.
    fn check_order(&self, r: &Replay, l: u8) -> Result<(), Violation> {
        // A boot number comes back after 2^32 appends of its opener: a
        // later stage's link from that long ago may alias a new boot.
        let aliased = self.recs.iter().any(|x| {
            x.layout == l
                && x.dropped.is_none()
                && x.writer != x.entry.boot.opener
                && self
                    .appended
                    .get(&(x.entry.boot.opener, l))
                    .copied()
                    .unwrap_or(0)
                    - x.opener_count
                    >= (1 << 32) - 512
        });
        let excused = r
            .boots
            .iter()
            .any(|b| self.is_excused(b.id.opener, l, b.id.boot));
        // Serial order cannot tell two numbers 2^31 or more apart: a link
        // that old into a sequence the opener no longer holds is not ordered
        // by numbers (stated limit).
        let stale_far = self.recs.iter().any(|x| {
            let o = x.entry.boot.opener;
            x.layout == l
                && x.dropped.is_none()
                && self.appended.get(&(o, l)).copied().unwrap_or(0) - x.opener_count >= 1 << 31
                && !self.recs.iter().any(|y| {
                    y.writer == o
                        && y.layout == l
                        && y.dropped.is_none()
                        && y.entry.tag == x.entry.boot.tag
                })
        });
        if aliased || excused || stale_far {
            return Ok(());
        }
        for b in &r.boots {
            self.reused(b.id, l)?;
        }
        let mut truth = Vec::new();
        for b in &r.boots {
            let recs: Vec<&Rec> = b
                .events
                .iter()
                .map(|e| &self.recs[self.rec_of(l, e.writer, &e.entry).unwrap()])
                .collect();
            if recs.windows(2).any(|p| p[0].true_boot != p[1].true_boot) {
                let tb: Vec<u64> = recs.iter().map(|x| x.true_boot).collect();
                return Err(self.fail("P6", format!("boot {:?} mixes boots {tb:?}", b.id)));
            }
            truth.push((recs[0].true_boot, recs[0].opener_epoch));
        }
        let mut sorted: Vec<u64> = truth.iter().map(|t| t.0).collect();
        sorted.sort_unstable();
        let split = sorted.windows(2).find(|p| p[0] == p[1]).map(|p| p[0]);
        if split.is_some_and(|b| !self.broken.contains(&b)) {
            return Err(self.fail("P6", "one boot split into two"));
        }
        for (i, a) in r.boots.iter().enumerate() {
            for (j, b) in r.boots.iter().enumerate().skip(i + 1) {
                let known = r.ordered[i][j];
                if truth[i].0 > truth[j].0 && known {
                    return Err(self.fail(
                        "P5",
                        format!("boot {:?} replayed before older {:?}", a.id, b.id),
                    ));
                }
                // Numbers of one opener 2^30 or more apart in a sequence that
                // is no longer its current one are not ordered.
                let gap = a.id.boot.wrapping_sub(b.id.boot);
                let far = gap.min(gap.wrapping_neg()) >= 1 << 30;
                let same = a.id.opener == b.id.opener && truth[i].1 == truth[j].1 && !far;
                if !known && same {
                    return Err(self.fail(
                        "P5",
                        format!(
                            "boots {:?} and {:?} of one sequence not ordered",
                            a.id, b.id
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// P10.
    fn check_size(&self, r: &Replay, l: u8, geo: &Geometry) -> Result<(), Violation> {
        let region = &self.mem.bytes()[geo.region.clone()];
        if region.len() > RECORD_MAX {
            return Err(self.fail("P10", "region over RECORD_MAX"));
        }
        for link in [max_payload::I2C, max_payload::UART, max_payload::USB] {
            if paged_read(region, record_page(link)) != region {
                return Err(self.fail("P10", "paged read differs"));
            }
        }
        let s = record::summary(l, r);
        if s.len() > record_page(max_payload::I2C) {
            return Err(self.fail("P10", "summary larger than a page"));
        }
        Ok(())
    }

    /// P11: a consumer that de-duplicates on (layout, writer, eseq) against
    /// the previous replay it read never meets one identity with two
    /// contents (short of 2^32 appends between the two replays). An identity
    /// absent from a replay is never replayed again, so the previous replay
    /// is all a consumer keeps.
    fn consume(&mut self, r: &Replay, l: u8) -> Result<(), Violation> {
        let now: HashMap<(u8, u8, u32, u32), Entry> = r
            .events()
            .map(|ev| ((l, ev.writer, ev.entry.tag, ev.entry.eseq), ev.entry))
            .collect();
        for (key, entry) in &now {
            let Some(seen) = self.consumer.get(key) else {
                continue;
            };
            if seen == entry {
                continue;
            }
            // Identities are 32-bit sequence numbers: the same one comes back
            // after 2^32 appends of the writer.
            let new = self.rec_of(l, key.1, entry).map(|i| self.recs[i].ordinal);
            let excused = self.recs.iter().any(|x| {
                x.layout == l
                    && x.writer == key.1
                    && x.entry == *seen
                    && new.is_some_and(|n| n - x.ordinal >= (1 << 32) - 256)
            });
            if !excused && !self.is_excused(key.1, l, key.3) {
                return Err(self.fail("P11", format!("identity {key:?} changed")));
            }
        }
        self.consumer.retain(|k, _| k.0 != l);
        self.consumer.extend(now);
        Ok(())
    }

    fn fast_forward(&mut self, boots: u64, linked: bool) -> Result<(), Violation> {
        if self.cfg.weak {
            return Ok(());
        }
        let geo = self.cfg.geometry(RecV1.entry_size());
        let first = self.ff_writer(&geo, 0, boots, 2, None)?;
        self.refresh(Phase::Writer(0, 1));
        if linked && geo.rings.len() > 1 {
            self.ff_writer(&geo, 1, boots, 3, Some(first))?;
            self.refresh(Phase::Writer(1, 1));
        }
        self.true_boot += boots;
        Ok(())
    }

    /// Writes the last ring-full of `boots` boots of `per` entries each, as
    /// the writer would have; returns its first eseq (the boot numbering).
    fn ff_writer(
        &mut self,
        geo: &Geometry,
        w: u8,
        boots: u64,
        per: u64,
        link: Option<(u32, u32)>,
    ) -> Result<(u32, u32), Violation> {
        let ent = self.entropy();
        let wr = Writer::open(&RecV1, &self.mem, geo, w, None, ent);
        self.opened(w, 1, geo.rings[w as usize].slots, &wr)?;
        let ring = geo.rings[w as usize];
        let cursor = (wr.next_slot() - ring.base) / V1_SIZE;
        let (next, fresh, tag) = (wr.next_eseq(), wr.fresh(), wr.boot().tag);
        if fresh {
            self.mem.poke(ring.base, &vec![0; ring.slots * V1_SIZE]);
        }
        let total = boots * per;
        let base = self.count(w, 1, total);
        let opener_count = self.count(0, 1, 0);
        let opener_epoch = self.epoch(0, 1);
        let keep = total.min(ring.slots as u64);
        for i in total - keep..total {
            let boot_start = (i / per) as u32;
            let e = Entry {
                start: i % per == 0,
                epoch: fresh && i == 0,
                boot: match link {
                    None => BootId {
                        opener: w,
                        boot: next.wrapping_add(boot_start.wrapping_mul(per as u32)),
                        tag,
                    },
                    Some((b0, t0)) => BootId {
                        opener: 0,
                        boot: b0.wrapping_add(boot_start.wrapping_mul(2)),
                        tag: t0,
                    },
                },
                fmt: 1,
                kind: 0xFF,
                eseq: next.wrapping_add(i as u32),
                tag,
                payload: [0; 3],
            };
            let addr = ring.base + ((cursor as u64 + i) % ring.slots as u64) as usize * V1_SIZE;
            let bytes = committed(&RecV1, w, &e);
            self.mem.poke(addr, &bytes);
            self.supersede(addr, w, 1);
            self.recs.push(Rec {
                true_boot: self.true_boot + 1 + i / per,
                writer: w,
                layout: 1,
                addr,
                bytes,
                entry: e,
                dropped: None,
                accounted: false,
                ordinal: base + i,
                opener_count,
                opener_epoch,
                lost_at_count: 0,
            });
        }
        Ok((next, tag))
    }

    fn fuzz(&mut self, w: u8, at: u16, len: u8) {
        let ranges = self.cfg.ranges();
        let Some(r) = ranges.get(w as usize) else {
            return;
        };
        let start = r.start + at as usize % r.len();
        let end = (start + len as usize).min(r.end);
        self.mem.randomise(start..end);
        self.refresh(Phase::Foreign);
    }
}

use crate::record::V1_SIZE;

pub struct RecordWorld;

impl World for RecordWorld {
    type Config = Config;
    type Op = Op;

    fn generate(rng: &mut SimRng) -> (Config, Vec<Op>) {
        let pick = |rng: &mut SimRng, xs: &[usize]| xs[rng.below(xs.len() as u64) as usize];
        let cfg = Config {
            survives: [
                Survives::ResetRandom,
                Survives::ResetCleared,
                Survives::Power,
            ][rng.below(3) as usize],
            stage0: rng.chance(0.85),
            s0_bytes: pick(rng, &[3, 4, 8]) * V1_SIZE,
            bc_bytes: pick(rng, &[4, 8, 16]) * V1_SIZE + pick(rng, &[0, 4]),
            more: (0..match rng.below(10) {
                0..=4 => 0,
                5..=8 => 1 + rng.below(2),
                _ => 1 + rng.below(14),
            })
                .map(|_| pick(rng, &[3, 4]) * V1_SIZE)
                .collect(),
            gap: pick(rng, &[0, 4, 12]),
            weak: false,
        };
        let writers = cfg.stage0 as u64 + 1 + cfg.more.len() as u64;
        let vers = [V1, V1, V1, V1, V2, Old, Absent];
        let ver = |rng: &mut SimRng| vers[rng.below(vers.len() as u64) as usize];
        let end = |rng: &mut SimRng| [Next, Next, Next, Next, Halt, Hang][rng.below(6) as usize];
        let (mut v0, mut vb, mut va) = (V1, V1, V1);
        let n = 1 + rng.below(40);
        let mut ops = Vec::new();
        for _ in 0..n {
            let roll = rng.below(100);
            if roll < 4 {
                let boots = match rng.below(5) {
                    0 => 1 + rng.below(20),
                    1 => (1 << 15) + rng.below(64),
                    2 => (1 << 30) + rng.below(64),
                    3 => (1 << 31) + rng.below(64),
                    _ => (1 << 31) - 1 - rng.below(64),
                };
                ops.push(Op::FastForward {
                    boots,
                    linked: rng.chance(0.5),
                });
                continue;
            }
            if roll < 7 {
                ops.push(Op::Fuzz {
                    writer: rng.below(writers) as u8,
                    at: rng.below(400) as u16,
                    len: 1 + rng.below(30) as u8,
                });
                continue;
            }
            if rng.chance(0.08) {
                v0 = ver(rng);
            }
            if rng.chance(0.05) {
                vb = ver(rng);
            }
            if rng.chance(0.05) {
                va = [V1, V1, V2, Absent][rng.below(4) as usize];
            }
            let s0 = Stage {
                ver: v0,
                events: 1 + rng.below(4) as u8,
                end: end(rng),
            };
            let most = if rng.chance(0.1) { 40 } else { 10 };
            let bc = Stage {
                ver: if vb == Absent { Old } else { vb },
                events: 1 + rng.below(most) as u8,
                end: end(rng),
            };
            let more: Vec<Stage> = cfg
                .more
                .iter()
                .map(|_| Stage {
                    ver: [V1, V1, V1, V1, V1, V2, Old][rng.below(7) as usize],
                    events: 1 + rng.below(3) as u8,
                    end: end(rng),
                })
                .collect();
            let all = s0.events as u64 + bc.events as u64;
            let est = (all + more.iter().map(|m| m.events as u64).sum::<u64>()) * 26;
            ops.push(Op::Boot {
                s0,
                bc,
                more,
                app: va,
                cut: rng.chance(0.3).then(|| rng.below(est) as u32),
                power: rng.chance(0.04),
            });
        }
        (cfg, ops)
    }

    fn run(cfg: &Config, ops: &[Op], seed: u64) -> Result<(), Violation> {
        for size in [V1_SIZE, 32] {
            cfg.geometry(size)
                .validate(size)
                .map_err(|e| Violation::new("P10", 0, e))?;
        }
        let mut sim = Sim::new(cfg, seed);
        for (i, op) in ops.iter().enumerate() {
            sim.step = i;
            match *op {
                Op::Boot { .. } => sim.boot(op)?,
                Op::FastForward { boots, linked } => sim.fast_forward(boots, linked)?,
                Op::Fuzz { writer, at, len } => sim.fuzz(writer, at, len),
            }
            if let Some(a) = sim.mem.stray().first() {
                return Err(sim.fail("P3", format!("store outside the writer's ring at {a}")));
            }
        }
        Ok(())
    }

    fn simpler(op: &Op) -> Vec<Op> {
        match op {
            Op::Boot {
                s0,
                bc,
                more,
                app,
                cut,
                power,
            } => {
                let (s0, bc, app, cut, power) = (*s0, *bc, *app, *cut, *power);
                let mk = |s0, bc, app, cut, power| Op::Boot {
                    s0,
                    bc,
                    more: more.clone(),
                    app,
                    cut,
                    power,
                };
                let fewer = (!more.is_empty()).then(|| Op::Boot {
                    s0,
                    bc,
                    more: more[..more.len() - 1].to_vec(),
                    app,
                    cut,
                    power,
                });
                let one = |s: Stage| Stage { events: 1, ..s };
                let next = |s: Stage| Stage { end: Next, ..s };
                let v1 = |s: Stage| Stage { ver: V1, ..s };
                vec![
                    mk(s0, bc, app, None, power),
                    mk(s0, bc, app, cut, false),
                    mk(one(s0), bc, app, cut, power),
                    mk(s0, one(bc), app, cut, power),
                    mk(next(s0), bc, app, cut, power),
                    mk(s0, next(bc), app, cut, power),
                    mk(v1(s0), bc, app, cut, power),
                    mk(s0, v1(bc), app, cut, power),
                    mk(s0, bc, V1, cut, power),
                ]
                .into_iter()
                .chain(fewer)
                .filter(|o| o != op)
                .collect()
            }
            &Op::FastForward { boots, linked } if boots > 1 => vec![
                Op::FastForward { boots: 1, linked },
                Op::FastForward {
                    boots: boots / 2,
                    linked,
                },
            ],
            &Op::Fuzz { writer, at, len } if len > 1 => vec![Op::Fuzz { writer, at, len: 1 }],
            _ => Vec::new(),
        }
    }
}
