//! Link diagnostics scenarios: the state log with keyed records
//! ([`crate::journal`]) holding the diagnostics records ([`crate::diag`]),
//! on flash where power fails after any byte programmed or erased and
//! where foreign bytes land anywhere. A scenario mixes samples, clears,
//! seed requests, restarts (the boot loader's part, then the app's
//! cycle-end evaluation), compaction by the app and by the boot loader,
//! records of the image slots and keyed records of a class nothing here
//! decodes.
//!
//! An oracle holds the state the records must fold to; after a cut the
//! flash must hold the state before the record being written or the one
//! after it (`state`). On top of that, after every operation
//! (retained-formats.md 8.6):
//!
//! - L1: a peer read as ever communicated had a pass sample since the last
//!   ever-communicated clear, with the identifier read;
//! - L2: a completed ever-communicated record is still read;
//! - L3: what a completed clear removed is not read until a new sample;
//! - L4: the records appended in a cycle stay within the bound;
//! - L5: no byte outside the faces is written;
//! - L6: the fold of the image slots is the one `dagon_core::log::Log`
//!   gives on the same bytes, and an operation on them leaves what `Log`
//!   leaves, keyed records aside;
//! - L7: the evaluation, cut anywhere and run again, ends where one run ends;
//! - L8: pendingDTC, confirmedDTC, aging and the failed-cycle count follow
//!   the cycles' outcomes;
//! - L9: no seed counter is returned twice.
//!
//! Foreign bytes can take the log back to an earlier state, so after them
//! the oracle starts again from what is read; what still holds then is
//! that no peer is read that never had a pass sample.

use crate::diag::{
    evaluate, on_fail, on_pass, status, DiagRecord, DtcScope, EverScope, Id, Params, Peer,
    Retained, SeedError, Store, CLEARED,
};
use crate::harness::{Violation, World};
use crate::journal::{Error, Journal, Keyed, Layout, CLASS_MAX};
use crate::mem::Mem;
use crate::rng::Rng;
use dagon_core::log::{Log, LogError, LogLayout, LogState, SlotRef};
use dagon_format::statelog::{kind, reason, target, Record, RECORD_SIZE};
use dagon_format::Version;
use dagon_port::{Flash, FlashError, FlashRegion};
use std::collections::{BTreeMap, BTreeSet};

/// The peers a scenario samples, by `Op` index.
pub const PEERS: [Peer; 5] = [(0, 1), (0, 2), (1, 1), (1, 2), (1, 3)];
/// The DTC number the scenarios sample (any number that names a fault).
pub const DTC: u32 = 1;
/// A keyed class the model has no decoder for.
pub const OTHER_CLASS: u8 = CLASS_MAX;
/// Bytes before and after the faces that nothing may write.
pub const GUARD: usize = 64;

const SLOT_KINDS: [u8; 6] = [
    kind::PROVISION,
    kind::SLOT_PENDING,
    kind::BOOT_ATTEMPT,
    kind::CONFIRMED,
    kind::ROLLBACK,
    kind::SNAPSHOT,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Records one face takes after its header.
    pub face_records: usize,
    /// `diag_peers`.
    pub peers_max: usize,
    pub params: Params,
}

impl Config {
    pub fn layout(&self) -> Layout {
        let face_len = RECORD_SIZE * (1 + self.face_records);
        Layout {
            faces: [GUARD, GUARD + face_len],
            face_len,
        }
    }
}

/// `cut`: power fails after that many more bytes programmed or erased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// A restart: the boot loader consumes a waiting UPDATE_STAY and, with
    /// `core_compacts`, compacts; then the app starts a cycle and
    /// evaluates the one that ended.
    Boot {
        core_compacts: bool,
        cut: Option<u64>,
    },
    Pass {
        peer: u8,
        id: u8,
        cut: Option<u64>,
    },
    Fail {
        peer: u8,
        cut: Option<u64>,
    },
    /// `scope`: 0 one DTC of the peer, 1 the peer's DTCs, 2 all DTCs.
    ClearDtc {
        scope: u8,
        peer: u8,
        cut: Option<u64>,
    },
    ClearEver {
        all: bool,
        peer: u8,
        cut: Option<u64>,
    },
    Seed {
        cut: Option<u64>,
    },
    /// The app compacts.
    Compact {
        cut: Option<u64>,
    },
    /// A record of an image slot (`kind`: index into the kinds the log
    /// folds), appended as the app does or in place as the boot path does.
    Slot {
        core: bool,
        kind: u8,
        slot: u8,
        in_place: bool,
        cut: Option<u64>,
    },
    /// The app hands over to the boot loader's update mode.
    Stay {
        keep: u8,
        cut: Option<u64>,
    },
    /// A put of [`OTHER_CLASS`].
    Other {
        key: u8,
        value: u8,
        cut: Option<u64>,
    },
    /// Random bytes at `at` (an offset into the flash, wrapped); the node
    /// stops until the next restart.
    Foreign {
        at: u16,
        len: u8,
    },
}

/// What the keyed records fold to, as the store reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Abs {
    pub ever: BTreeMap<Peer, Id>,
    pub dtc: BTreeMap<Peer, Retained>,
    pub counter: u32,
    pub other: BTreeMap<u8, u8>,
}

/// One record an operation appends, as its effect on [`Abs`].
#[derive(Clone, Copy, Debug)]
enum Step {
    Ever(Peer, Id),
    Dtc(Peer, Retained),
    EraseDtc(DtcScope),
    EraseEver(EverScope),
    Seed(u32),
    Other(u8, u8),
}

impl Abs {
    pub fn of(j: &Journal) -> Abs {
        let mut a = Abs::default();
        for k in &j.keyed {
            if k.class == OTHER_CLASS {
                a.other.insert(k.body[0], k.body[1]);
                continue;
            }
            match DiagRecord::decode(k) {
                Some(DiagRecord::Ever { peer, id }) => {
                    a.ever.insert(peer, id);
                }
                Some(DiagRecord::Dtc {
                    peer,
                    dtc: DTC,
                    state,
                }) => {
                    a.dtc.insert(peer, state);
                }
                Some(DiagRecord::Seed(c)) => a.counter = c,
                _ => {}
            }
        }
        a
    }

    fn apply(&mut self, s: &Step) {
        match *s {
            Step::Ever(p, id) => {
                self.ever.insert(p, id);
            }
            Step::Dtc(p, r) => {
                self.dtc.insert(p, r);
            }
            Step::EraseDtc(sc) => self.dtc.retain(|p, _| !sc.covers(*p)),
            Step::EraseEver(sc) => {
                self.ever.retain(|p, _| !sc.covers(*p));
                self.dtc.retain(|p, _| !sc.covers(*p));
            }
            Step::Seed(c) => self.counter = c,
            Step::Other(k, v) => {
                self.other.insert(k, v);
            }
        }
    }

    fn state(&self, p: Peer) -> Retained {
        *self.dtc.get(&p).unwrap_or(&CLEARED)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Outcome {
    #[default]
    NotTested,
    Passed,
    Failed,
}

/// L8's reference: one DTC followed by the outcome of each cycle, with no
/// status bits.
#[derive(Clone, Copy, Debug, Default)]
struct Cyc {
    pending: bool,
    confirmed: bool,
    age: u8,
    fails: u8,
    outcome: Outcome,
}

impl Cyc {
    fn of(r: Retained) -> Cyc {
        Cyc {
            pending: r.status & status::PENDING != 0,
            confirmed: r.status & status::CONFIRMED != 0,
            age: r.age,
            fails: r.fails,
            outcome: if r.status & status::NOT_COMPLETED_THIS_CYCLE != 0 {
                Outcome::NotTested
            } else if r.status & status::FAILED_THIS_CYCLE != 0 {
                Outcome::Failed
            } else {
                Outcome::Passed
            },
        }
    }

    fn failed(&mut self, p: &Params) {
        self.outcome = Outcome::Failed;
        self.pending = true;
        self.fails = self.fails.saturating_add(1);
        self.confirmed |= self.fails >= p.confirm;
        self.age = 0;
    }

    fn cycle_end(&mut self, p: &Params) {
        if self.outcome == Outcome::Passed {
            self.pending = false;
            self.fails = 0;
            if self.confirmed {
                self.age += 1;
                if self.age >= p.aging {
                    self.confirmed = false;
                    self.age = 0;
                }
            }
        }
        self.outcome = Outcome::NotTested;
    }

    fn agrees(&self, r: Retained) -> bool {
        let c = Cyc::of(r);
        (self.pending, self.confirmed, self.age, self.fails)
            == (c.pending, c.confirmed, c.age, c.fails)
    }
}

#[derive(Default)]
struct Oracle {
    abs: Abs,
    /// FDC of the running cycle.
    fdc: BTreeMap<Peer, i8>,
    /// Every pass sample of the scenario.
    seen: BTreeSet<(Peer, Id)>,
    /// L1: the pass samples since the last ever-communicated clear.
    passed: BTreeSet<(Peer, Id)>,
    /// L2: peers whose ever-communicated append completed.
    kept: BTreeSet<Peer>,
    /// L3: what a completed clear removed, until the next sample.
    gone_ever: BTreeSet<Peer>,
    gone_dtc: BTreeSet<Peer>,
    /// L4, this cycle: clears and seeds asked for, and the peers the
    /// clears covered.
    clears: u32,
    seeds: u32,
    reopened: u32,
    /// L7: where the evaluation of the cycle that ended must arrive.
    goal: Option<BTreeMap<Peer, Retained>>,
    cyc: BTreeMap<Peer, Cyc>,
    /// L9.
    returned: BTreeSet<u32>,
}

type V = (&'static str, String);

fn bad<T>(property: &'static str, detail: impl Into<String>) -> Result<T, V> {
    Err((property, detail.into()))
}

/// The production log's flash: plain NOR bytes, no faults.
struct Plain(Vec<u8>);

impl Flash for Plain {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.0[addr as usize..][..buf.len()]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.0[addr as usize..][..len as usize].fill(0xFF);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        for (b, d) in self.0[addr as usize..].iter_mut().zip(data) {
            *b &= d;
        }
        Ok(())
    }
}

/// What `Log` folds the image slots' records to.
type View = Option<(LogState, LogState, bool, u32)>;

fn log_layout(l: &Layout) -> LogLayout {
    LogLayout {
        faces: l.faces.map(|f| f as u32),
        face_len: l.face_len as u32,
    }
}

fn view(bytes: &[u8], l: &Layout) -> View {
    Log::load(&mut Plain(bytes.to_vec()), &log_layout(l))
        .ok()
        .map(|g| (g.state, g.core, g.update_stay, g.generation))
}

/// `op` run by the production log on a copy of `bytes`: the fold and the
/// bytes it leaves.
fn reference(
    bytes: &[u8],
    l: &Layout,
    op: impl FnOnce(&mut Log, &mut Plain) -> Result<(), LogError>,
) -> (View, Vec<u8>) {
    let mut f = Plain(bytes.to_vec());
    if let Ok(mut log) = Log::load(&mut f, &log_layout(l)) {
        let _ = op(&mut log, &mut f);
    }
    (view(&f.0, l), f.0)
}

/// `Log`'s compaction, forced: no face has room for `u32::MAX` records.
fn force_compaction(log: &mut Log, f: &mut Plain) -> Result<(), LogError> {
    let _ = log.reserve(f, u32::MAX);
    Ok(())
}

/// The faces' 32-byte slots that are neither erased nor a keyed record,
/// records without their sequence number: no fold reads it, and a `Log`
/// kept in RAM numbers on where one loaded after a compaction starts again.
fn unkeyed(bytes: &[u8], l: &Layout) -> Vec<Vec<[u8; RECORD_SIZE]>> {
    l.faces
        .iter()
        .map(|&f| {
            bytes[f..f + l.face_len]
                .chunks_exact(RECORD_SIZE)
                .map(|c| <[u8; RECORD_SIZE]>::try_from(c).expect("32 bytes"))
                .filter(|b| b.iter().any(|&x| x != 0xFF) && Keyed::parse(b).is_none())
                .map(|b| Record::parse(&b).map_or(b, |r| Record { seq: 0, ..r }.to_bytes()))
                .collect()
        })
        .collect()
}

/// An operation on the image slots' records, by the model's journal and by
/// `Log` from the same bytes (L6). `Ok(false)`: power failed.
fn slot_op(
    mem: &mut Mem,
    l: &Layout,
    j: &mut Journal,
    by_log: impl FnOnce(&mut Log, &mut Plain) -> Result<(), LogError>,
    by_model: impl FnOnce(&mut Journal, &mut Mem) -> Result<(), Error>,
) -> Result<bool, V> {
    let pre = mem.bytes().to_vec();
    let before = view(&pre, l);
    let (wanted, left) = reference(&pre, l, by_log);
    let (compacted, _) = reference(&pre, l, force_compaction);
    let keyed = !j.keyed.is_empty();
    let r = by_model(j, mem);
    let now = view(mem.bytes(), l);
    let detail = || format!("{r:?}: {now:?}, Log leaves {wanted:?}");
    match r {
        Ok(()) if now != wanted => bad("L6", detail()),
        Ok(()) if unkeyed(mem.bytes(), l) != unkeyed(&left, l) => {
            bad("L6", "the bytes differ from those Log leaves")
        }
        Ok(()) => Ok(true),
        Err(Error::Full) if (now != before && now != compacted) || (!keyed && now != wanted) => {
            bad("L6", detail())
        }
        Err(Error::Full) => Ok(true),
        Err(Error::Cut) if now != before && now != compacted && now != wanted => {
            bad("L6", detail())
        }
        Err(Error::Cut) => Ok(false),
        Err(Error::NoValidFace) => bad("L6", detail()),
    }
}

struct Run<'a> {
    cfg: &'a Config,
    layout: Layout,
    mem: Mem,
    /// The running app's store; `None` while the node is stopped.
    store: Option<Store>,
    o: Oracle,
}

fn peer(i: u8) -> Peer {
    PEERS[usize::from(i) % PEERS.len()]
}

impl Run<'_> {
    fn observe(&self) -> Option<Abs> {
        Journal::load(&self.mem, &self.layout)
            .ok()
            .map(|j| Abs::of(&j))
    }

    /// Runs an operation that appends the keyed records of `plan`, in
    /// order. Returns how many of them the flash holds and whether the
    /// operation completed.
    fn keyed_op(
        &mut self,
        plan: &[Step],
        run: impl FnOnce(&mut Store, &mut Mem) -> Result<(), Error>,
    ) -> Result<(usize, bool), V> {
        let pre = self.mem.bytes().to_vec();
        let store = self.store.as_mut().expect("the app runs");
        let w0 = store.j.writes;
        let r = run(store, &mut self.mem);
        let done = (store.j.writes - w0) as usize;
        let after = |n: usize| {
            let mut a = self.o.abs.clone();
            plan[..n].iter().for_each(|s| a.apply(s));
            a
        };
        let applied = match r {
            Ok(()) | Err(Error::Full) if done > plan.len() || (r.is_ok() && done < plan.len()) => {
                return bad(
                    "state",
                    format!("{done} records appended, {} expected", plan.len()),
                );
            }
            Ok(()) | Err(Error::Full) => done,
            Err(Error::NoValidFace) => return bad("state", "the active face was lost"),
            Err(Error::Cut) => {
                let read = self.observe();
                let landed = (done..=(done + 1).min(plan.len()))
                    .find(|&n| n <= plan.len() && read.as_ref() == Some(&after(n)));
                match landed {
                    Some(n) => n,
                    None => return bad("state", format!("after a cut in record {done}: {read:?}")),
                }
            }
        };
        if r == Err(Error::Cut) {
            self.store = None;
        }
        self.o.abs = after(applied);
        // The slots fold as before, or as compaction leaves them (an
        // operation of two records can compact twice).
        let fold = |v: View| v.map(|(app, core, stay, generation)| ((app, core, stay), generation));
        let before = fold(view(&pre, &self.layout));
        let compacted = fold(reference(&pre, &self.layout, force_compaction).0);
        let now = fold(view(self.mem.bytes(), &self.layout));
        let kept = match (now, before, compacted) {
            (Some(n), Some(b), _) if n.1 == b.1 => n.0 == b.0,
            (Some(n), Some(b), Some(c)) => n.1 > b.1 && n.0 == c.0,
            _ => false,
        };
        if !kept {
            return bad("L6", format!("a keyed append changed the fold to {now:?}"));
        }
        Ok((applied, r.is_ok()))
    }

    fn boot(&mut self, core_compacts: bool) -> Result<(), V> {
        self.store = None;
        let Ok(mut j) = Journal::load(&self.mem, &self.layout) else {
            return Ok(());
        };
        // The boot loader: it folds and compacts with no keyed class known.
        let alive = slot_op(
            &mut self.mem,
            &self.layout,
            &mut j,
            |log, f| log.take_stay(f).map(|_| ()),
            |j, m| j.take_stay(m).map(|_| ()),
        )?;
        if !alive {
            return Ok(());
        }
        if core_compacts {
            let alive = slot_op(
                &mut self.mem,
                &self.layout,
                &mut j,
                force_compaction,
                |j, m| j.compact(m),
            )?;
            if !alive {
                return Ok(());
            }
        }
        // The app: a new cycle, and the evaluation of the one that ended.
        let p = self.cfg.params;
        let o = &mut self.o;
        o.fdc.clear();
        (o.clears, o.seeds, o.reopened) = (0, 0, 0);
        let evaluated: BTreeMap<Peer, Retained> = o
            .abs
            .dtc
            .iter()
            .map(|(peer, r)| (*peer, evaluate(*r, &p)))
            .collect();
        let plan: Vec<(Peer, Step)> = evaluated
            .iter()
            .filter(|(peer, n)| o.abs.dtc[*peer] != **n)
            .map(|(peer, n)| (*peer, Step::Dtc(*peer, *n)))
            .collect();
        let goal = o.goal.get_or_insert(evaluated).clone();
        self.store = Some(Store::open(j, p, self.cfg.peers_max, DTC));
        let steps: Vec<Step> = plan.iter().map(|(_, s)| *s).collect();
        let (applied, completed) = self.keyed_op(&steps, |s, m| s.cycle_start(m))?;
        for (peer, c) in self.o.cyc.iter_mut() {
            if plan
                .iter()
                .position(|(q, _)| q == peer)
                .is_none_or(|i| i < applied)
            {
                c.cycle_end(&p);
            }
        }
        if !completed {
            // A log that cannot take the evaluation gives no store this cycle.
            self.store = None;
            return Ok(());
        }
        if self.o.abs.dtc != goal {
            return bad(
                "L7",
                format!(
                    "evaluation ended at {:?}, one run gives {goal:?}",
                    self.o.abs.dtc
                ),
            );
        }
        self.o.goal = None;
        Ok(())
    }

    fn foreign(&mut self, at: u16, len: u8) -> Result<(), V> {
        let at = usize::from(at) % self.mem.len();
        let end = (at + usize::from(len).max(1)).min(self.mem.len());
        self.mem.randomise(at..end);
        self.store = None;
        let read = self.observe().unwrap_or_default();
        let o = &mut self.o;
        if let Some(e) = read
            .ever
            .iter()
            .find(|(p, id)| !o.seen.contains(&(**p, **id)))
        {
            return bad("L1", format!("{e:?} read with no such pass sample"));
        }
        o.passed.extend(read.ever.iter().map(|(p, id)| (*p, *id)));
        o.kept.retain(|p| read.ever.contains_key(p));
        o.gone_ever.clear();
        o.gone_dtc.clear();
        o.returned.clear();
        o.goal = None;
        o.cyc = read.dtc.iter().map(|(p, r)| (*p, Cyc::of(*r))).collect();
        o.abs = read;
        Ok(())
    }

    fn running(&mut self, op: &Op) -> Result<(), V> {
        let params = self.cfg.params;
        let l = self.layout;
        match *op {
            Op::Boot { .. } | Op::Foreign { .. } => unreachable!("not an app operation"),
            Op::Pass { peer: i, id, .. } => {
                let (p, id) = (peer(i), [id; 16]);
                let o = &mut self.o;
                o.seen.insert((p, id));
                o.passed.insert((p, id));
                o.gone_ever.remove(&p);
                o.gone_dtc.remove(&p);
                let known = o.abs.ever.contains_key(&p);
                let mut plan = Vec::new();
                if known || o.abs.ever.len() < self.cfg.peers_max {
                    if !known {
                        plan.push(Step::Ever(p, id));
                    }
                    o.fdc.insert(p, i8::MIN);
                    let r = o.abs.state(p);
                    if on_pass(r) != r {
                        plan.push(Step::Dtc(p, on_pass(r)));
                    }
                }
                let (applied, completed) = self.keyed_op(&plan, |s, m| s.pass(m, p, id))?;
                let o = &mut self.o;
                if completed && !known && applied > 0 {
                    o.kept.insert(p);
                }
                let recorded = plan
                    .iter()
                    .position(|s| matches!(s, Step::Dtc(..)))
                    .is_none_or(|i| i < applied);
                if o.abs.ever.contains_key(&p) && recorded {
                    let c = o.cyc.entry(p).or_default();
                    if c.outcome != Outcome::Failed {
                        c.outcome = Outcome::Passed;
                    }
                }
            }
            Op::Fail { peer: i, .. } => {
                let p = peer(i);
                let o = &mut self.o;
                let mut plan = Vec::new();
                if o.abs.ever.contains_key(&p) {
                    o.gone_dtc.remove(&p);
                    let fdc = o.fdc.entry(p).or_insert(0);
                    *fdc = (i16::from(*fdc) + i16::from(params.step)).min(127) as i8;
                    let r = o.abs.state(p);
                    if *fdc == i8::MAX && on_fail(r, &params) != r {
                        plan.push(Step::Dtc(p, on_fail(r, &params)));
                    }
                }
                let (applied, _) = self.keyed_op(&plan, |s, m| s.fail(m, p))?;
                if applied == 1 {
                    self.o.cyc.entry(p).or_default().failed(&params);
                }
            }
            Op::ClearDtc { scope, peer: i, .. } => {
                let sc = match scope % 3 {
                    0 => DtcScope::One(peer(i), DTC),
                    1 => DtcScope::Peer(peer(i)),
                    _ => DtcScope::All,
                };
                let o = &mut self.o;
                o.fdc.retain(|p, _| !sc.covers(*p));
                o.clears += 1;
                o.reopened += o.abs.ever.keys().filter(|p| sc.covers(**p)).count() as u32;
                let plan: Vec<Step> = o
                    .abs
                    .dtc
                    .keys()
                    .any(|p| sc.covers(*p))
                    .then_some(Step::EraseDtc(sc))
                    .into_iter()
                    .collect();
                let (applied, completed) = self.keyed_op(&plan, |s, m| s.clear_dtc(m, sc))?;
                let o = &mut self.o;
                if applied == 1 {
                    o.cyc.retain(|p, _| !sc.covers(*p));
                }
                if completed {
                    o.gone_dtc.extend(PEERS.iter().filter(|p| sc.covers(**p)));
                }
            }
            Op::ClearEver { all, peer: i, .. } => {
                let sc = if all {
                    EverScope::All
                } else {
                    EverScope::Peer(peer(i))
                };
                let o = &mut self.o;
                o.kept.retain(|p| !sc.covers(*p));
                o.fdc.retain(|p, _| !sc.covers(*p));
                o.clears += 1;
                o.reopened += o.abs.ever.keys().filter(|p| sc.covers(**p)).count() as u32;
                let plan: Vec<Step> = o
                    .abs
                    .ever
                    .keys()
                    .chain(o.abs.dtc.keys())
                    .any(|p| sc.covers(*p))
                    .then_some(Step::EraseEver(sc))
                    .into_iter()
                    .collect();
                let (applied, completed) = self.keyed_op(&plan, |s, m| s.clear_ever(m, sc))?;
                let o = &mut self.o;
                if applied == 1 {
                    o.cyc.retain(|p, _| !sc.covers(*p));
                }
                if applied == 1 || completed {
                    o.passed.retain(|(p, _)| !sc.covers(*p));
                }
                if completed {
                    o.gone_ever.extend(PEERS.iter().filter(|p| sc.covers(**p)));
                    o.gone_dtc.extend(PEERS.iter().filter(|p| sc.covers(**p)));
                }
            }
            Op::Seed { .. } => {
                self.o.seeds += 1;
                let plan = [Step::Seed(self.o.abs.counter + 1)];
                let mut got = None;
                self.keyed_op(&plan, |s, m| match s.seed(m) {
                    Ok(n) => {
                        got = Some(n);
                        Ok(())
                    }
                    Err(SeedError::Store(e)) => Err(e),
                    Err(SeedError::Exhausted) => Ok(()),
                })?;
                if let Some(n) = got {
                    if !self.o.returned.insert(n) {
                        return bad("L9", format!("seed counter {n} returned twice"));
                    }
                }
            }
            Op::Other { key, value, .. } => {
                let k = key % 2;
                self.keyed_op(&[Step::Other(k, value)], |s, m| {
                    s.j.append_keyed(m, Keyed::put(OTHER_CLASS, &[k], &[value]))
                })?;
            }
            Op::Compact { .. } => {
                let j = &mut self.store.as_mut().expect("the app runs").j;
                if !slot_op(&mut self.mem, &l, j, force_compaction, |j, m| j.compact(m))? {
                    self.store = None;
                }
            }
            Op::Slot {
                core,
                kind: k,
                slot,
                in_place,
                ..
            } => {
                let k = SLOT_KINDS[usize::from(k) % SLOT_KINDS.len()];
                let t = if core { target::BOOTCORE } else { target::APP };
                let why = if k == kind::ROLLBACK {
                    reason::TRIAL_EXHAUSTED
                } else {
                    reason::NONE
                };
                let at = SlotRef {
                    slot: slot % 2,
                    version: Version::new(1, 0, u16::from(slot)),
                    digest16: [slot; 16],
                };
                let j = &mut self.store.as_mut().expect("the app runs").j;
                let alive = slot_op(
                    &mut self.mem,
                    &l,
                    j,
                    |log, f| match (in_place, core) {
                        (true, _) => log.append_in_place(f, t, k, at, why),
                        (false, true) => log.append_core(f, k, at, why),
                        (false, false) => log.append(f, k, at, why),
                    },
                    |j, m| j.append(m, t, k, at, why, in_place),
                )?;
                if !alive {
                    self.store = None;
                }
            }
            Op::Stay { keep, .. } => {
                let j = &mut self.store.as_mut().expect("the app runs").j;
                let alive = slot_op(
                    &mut self.mem,
                    &l,
                    j,
                    |log, f| log.request_stay(f, u32::from(keep)),
                    |j, m| j.request_stay(m, usize::from(keep)),
                )?;
                if !alive {
                    self.store = None;
                }
            }
        }
        Ok(())
    }

    fn op(&mut self, op: &Op) -> Result<(), V> {
        let cut = match *op {
            Op::Foreign { at, len } => return self.foreign(at, len),
            Op::Boot { cut, .. }
            | Op::Pass { cut, .. }
            | Op::Fail { cut, .. }
            | Op::ClearDtc { cut, .. }
            | Op::ClearEver { cut, .. }
            | Op::Seed { cut }
            | Op::Compact { cut }
            | Op::Slot { cut, .. }
            | Op::Stay { cut, .. }
            | Op::Other { cut, .. } => cut,
        };
        self.mem.cut_after(cut);
        let r = match *op {
            Op::Boot { core_compacts, .. } => self.boot(core_compacts),
            _ if self.store.is_some() => self.running(op),
            _ => Ok(()),
        };
        self.mem.cut_after(None);
        r
    }

    /// The properties that hold after every operation.
    fn check(&self) -> Result<(), V> {
        let (o, p) = (&self.o, &self.cfg.params);
        if let Some(a) = self.mem.stray().first() {
            return bad("L5", format!("byte {a} written outside the faces"));
        }
        let Ok(j) = Journal::load(&self.mem, &self.layout) else {
            return if o.abs == Abs::default() {
                Ok(())
            } else {
                bad("state", "no valid face")
            };
        };
        let read = Abs::of(&j);
        if read != o.abs {
            return bad("state", format!("read {read:?}, expected {:?}", o.abs));
        }
        let folded = Some((j.state, j.core, j.update_stay, j.generation));
        if folded != view(self.mem.bytes(), &self.layout) {
            return bad("L6", format!("the model folds the slots to {folded:?}"));
        }
        if let Some(s) = &self.store {
            if Abs::of(&s.j) != read
                || (s.j.state, s.j.core, s.j.update_stay) != (j.state, j.core, j.update_stay)
            {
                return bad("state", "the running store and the flash differ");
            }
            let bound = 3 * self.cfg.peers_max as u32 + o.clears + o.seeds + 3 * o.reopened;
            if s.appends > bound {
                return bad(
                    "L4",
                    format!("{} appends in the cycle, bound {bound}", s.appends),
                );
            }
        }
        if let Some(e) = read
            .ever
            .iter()
            .find(|(q, id)| !o.passed.contains(&(**q, **id)))
        {
            return bad(
                "L1",
                format!("{e:?} read with no pass sample since the last clear"),
            );
        }
        if let Some(q) = o.kept.iter().find(|q| !read.ever.contains_key(q)) {
            return bad(
                "L2",
                format!("the ever-communicated record of {q:?} is gone"),
            );
        }
        let back = o.gone_ever.iter().find(|q| read.ever.contains_key(q));
        if let Some(q) = back.or(o.gone_dtc.iter().find(|q| read.dtc.contains_key(q))) {
            return bad("L3", format!("{q:?} is back after a completed clear"));
        }
        for (q, r) in &read.dtc {
            use status::*;
            let s = r.status;
            let has = |bits: u8| s & bits != 0;
            let sound = s & !RETAINED == 0
                && !has(NOT_COMPLETED_SINCE_CLEAR)
                && (!has(FAILED_THIS_CYCLE | PENDING | CONFIRMED) || has(FAILED_SINCE_CLEAR))
                && (!has(FAILED_THIS_CYCLE) || (has(PENDING) && !has(NOT_COMPLETED_THIS_CYCLE)))
                && r.age < p.aging
                && (r.age == 0 || has(CONFIRMED))
                && (r.fails == 0 || has(PENDING));
            if !sound {
                return bad("L8", format!("{q:?} holds {r:?}"));
            }
        }
        for q in read.ever.keys().chain(read.dtc.keys()) {
            let c = o.cyc.get(q).copied().unwrap_or_default();
            if !c.agrees(read.state(*q)) {
                return bad(
                    "L8",
                    format!("{q:?}: {:?}, the cycles give {c:?}", read.state(*q)),
                );
            }
        }
        Ok(())
    }
}

/// A flash with erased faces between two guards, face 0 valid.
pub fn flash(cfg: &Config, seed: u64) -> Mem {
    let l = cfg.layout();
    let mut mem = Mem::new(2 * GUARD + 2 * l.face_len, 0xFF, seed);
    mem.poke(
        l.faces[0],
        &dagon_format::FaceHeader { generation: 1 }.to_bytes(),
    );
    if seed & 1 == 1 {
        // A face never erased holds anything.
        mem.randomise(l.faces[1]..l.faces[1] + l.face_len);
    }
    mem.allow(l.faces[0]..l.faces[1] + l.face_len);
    mem
}

pub struct DiagWorld;

impl World for DiagWorld {
    type Config = Config;
    type Op = Op;

    fn generate(rng: &mut Rng) -> (Config, Vec<Op>) {
        let cfg = Config {
            face_records: 6 + rng.below(30) as usize,
            peers_max: 1 + rng.below(4) as usize,
            params: Params {
                step: [1, 40, 127, 128, 255][rng.below(5) as usize],
                confirm: 1 + rng.below(3) as u8,
                aging: 1 + rng.below(3) as u8,
            },
        };
        let face = cfg.layout().face_len as u64;
        let ops = (0..rng.below(60))
            .map(|_| {
                let cut = match rng.below(10) {
                    0 | 1 => Some(rng.below(70)),
                    2 => Some(rng.below(face + 400)),
                    _ => None,
                };
                let peer = rng.below(PEERS.len() as u64) as u8;
                match rng.below(100) {
                    0..14 => Op::Boot {
                        core_compacts: rng.chance(0.3),
                        cut,
                    },
                    14..36 => Op::Pass {
                        peer,
                        id: if rng.chance(0.7) { 0 } else { 1 + peer },
                        cut,
                    },
                    36..58 => Op::Fail { peer, cut },
                    58..63 => Op::ClearDtc {
                        scope: rng.below(3) as u8,
                        peer,
                        cut,
                    },
                    63..68 => Op::ClearEver {
                        all: rng.chance(0.3),
                        peer,
                        cut,
                    },
                    68..74 => Op::Seed { cut },
                    74..78 => Op::Compact { cut },
                    78..90 => Op::Slot {
                        core: rng.chance(0.4),
                        kind: rng.below(SLOT_KINDS.len() as u64) as u8,
                        slot: rng.below(2) as u8,
                        in_place: rng.chance(0.3),
                        cut,
                    },
                    90..92 => Op::Stay {
                        keep: rng.below(4) as u8,
                        cut,
                    },
                    92..96 => Op::Other {
                        key: rng.below(2) as u8,
                        value: rng.byte(),
                        cut,
                    },
                    _ => Op::Foreign {
                        at: rng.next_u64() as u16,
                        len: 1 + rng.below(40) as u8,
                    },
                }
            })
            .collect();
        (cfg, ops)
    }

    fn run(cfg: &Config, ops: &[Op], seed: u64) -> Result<(), Violation> {
        let mut run = Run {
            cfg,
            layout: cfg.layout(),
            mem: flash(cfg, seed),
            store: None,
            o: Oracle::default(),
        };
        let start = Op::Boot {
            core_compacts: false,
            cut: None,
        };
        for (step, op) in std::iter::once(&start).chain(ops).enumerate() {
            run.op(op)
                .and_then(|()| run.check())
                .map_err(|(property, detail)| {
                    Violation::new(property, step.saturating_sub(1), detail)
                })?;
        }
        Ok(())
    }

    fn simpler(op: &Op) -> Vec<Op> {
        let mut out = Vec::new();
        let mut o = *op;
        match &mut o {
            Op::Boot { cut, .. }
            | Op::Pass { cut, .. }
            | Op::Fail { cut, .. }
            | Op::ClearDtc { cut, .. }
            | Op::ClearEver { cut, .. }
            | Op::Seed { cut }
            | Op::Compact { cut }
            | Op::Slot { cut, .. }
            | Op::Stay { cut, .. }
            | Op::Other { cut, .. } => {
                if cut.take().is_some() {
                    out.push(o);
                }
            }
            Op::Foreign { len, .. } => {
                if *len > 1 {
                    *len = 1;
                    out.push(o);
                }
            }
        }
        if let Op::Boot {
            core_compacts: true,
            cut,
        } = *op
        {
            out.push(Op::Boot {
                core_compacts: false,
                cut,
            });
        }
        out
    }
}
