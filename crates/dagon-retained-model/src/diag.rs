//! The link diagnostics a node keeps in the state log: the five records as
//! keyed records ([`DiagRecord`], retained-formats.md 8.3), the retained
//! part of the DTC status and its transitions ([`on_pass`], [`on_fail`],
//! [`evaluate`], 8.4), and the store that writes them ([`Store`]).
//!
//! Every change is one appended record; nothing is rewritten in place.

use crate::journal::{Error, Journal, Keyed};
use crate::mem::Mem;
use std::collections::BTreeMap;

/// Keyed classes. A peer's records start their key with `link, addr`, so
/// one erase over [`CLASS_EVER`]`..=`[`CLASS_DTC`] removes all of a peer.
pub const CLASS_EVER: u8 = 0;
pub const CLASS_DTC: u8 = 1;
pub const CLASS_SEED: u8 = 2;

/// `(link, addr)`.
pub type Peer = (u8, u8);
/// The instance identifier; 16 zero bytes: absent.
pub type Id = [u8; 16];

/// statusOfDTC bits.
pub mod status {
    pub const TEST_FAILED: u8 = 0x01;
    pub const FAILED_THIS_CYCLE: u8 = 0x02;
    pub const PENDING: u8 = 0x04;
    pub const CONFIRMED: u8 = 0x08;
    pub const NOT_COMPLETED_SINCE_CLEAR: u8 = 0x10;
    pub const FAILED_SINCE_CLEAR: u8 = 0x20;
    pub const NOT_COMPLETED_THIS_CYCLE: u8 = 0x40;
    /// Bits 1 to 6: the ones a record holds.
    pub const RETAINED: u8 = 0x7E;
}
use status::*;

/// What a DTC state record holds of one DTC of one peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retained {
    /// Status bits 1 to 6.
    pub status: u8,
    /// Cycles passed since the DTC was confirmed.
    pub age: u8,
    /// Failed cycles in a row.
    pub fails: u8,
}

/// The state with no record: after a clear, or never sampled.
pub const CLEARED: Retained = Retained {
    status: NOT_COMPLETED_SINCE_CLEAR | NOT_COMPLETED_THIS_CYCLE,
    age: 0,
    fails: 0,
};

/// FDC step, confirmation threshold and aging threshold, each 1 to 255.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub step: u8,
    pub confirm: u8,
    pub aging: u8,
}

/// After a pass sample. Bit 6 is cleared only where the cycle-end
/// evaluation has something to decide (a pending or confirmed DTC), so a
/// healthy peer costs no record per cycle.
pub fn on_pass(r: Retained) -> Retained {
    let mut s = r.status & !NOT_COMPLETED_SINCE_CLEAR;
    if s & (PENDING | CONFIRMED) != 0 {
        s &= !NOT_COMPLETED_THIS_CYCLE;
    }
    Retained { status: s, ..r }
}

/// After a failed test (the FDC reached +127). Only the first one of a
/// cycle changes anything; it also restarts aging.
pub fn on_fail(r: Retained, p: &Params) -> Retained {
    if r.status & FAILED_THIS_CYCLE != 0 {
        return r;
    }
    let fails = r.fails.saturating_add(1);
    let confirmed = if fails >= p.confirm { CONFIRMED } else { 0 };
    Retained {
        status: (r.status & !(NOT_COMPLETED_SINCE_CLEAR | NOT_COMPLETED_THIS_CYCLE))
            | FAILED_THIS_CYCLE
            | PENDING
            | FAILED_SINCE_CLEAR
            | confirmed,
        age: 0,
        fails,
    }
}

/// The cycle-end evaluation, run at the next start from bits 1 and 6 of
/// the cycle that ended. Its result has bit 6 set, which is also what
/// makes a second run change nothing.
pub fn evaluate(r: Retained, p: &Params) -> Retained {
    if r.status & NOT_COMPLETED_THIS_CYCLE != 0 {
        return r;
    }
    let mut n = Retained {
        status: (r.status & !FAILED_THIS_CYCLE) | NOT_COMPLETED_THIS_CYCLE,
        ..r
    };
    if r.status & FAILED_THIS_CYCLE == 0 {
        n.status &= !PENDING;
        n.fails = 0;
        if n.status & CONFIRMED != 0 {
            n.age = n.age.saturating_add(1);
            if n.age >= p.aging {
                n.status &= !CONFIRMED;
                n.age = 0;
            }
        }
    }
    n
}

/// What a DTC clear names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DtcScope {
    One(Peer, u32),
    Peer(Peer),
    All,
}

/// What an ever-communicated clear names; it clears the DTCs too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EverScope {
    Peer(Peer),
    All,
}

impl DtcScope {
    pub fn covers(&self, peer: Peer) -> bool {
        match *self {
            DtcScope::One(p, _) | DtcScope::Peer(p) => p == peer,
            DtcScope::All => true,
        }
    }
}

impl EverScope {
    pub fn covers(&self, peer: Peer) -> bool {
        match *self {
            EverScope::Peer(p) => p == peer,
            EverScope::All => true,
        }
    }
}

/// The five records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagRecord {
    Ever {
        peer: Peer,
        id: Id,
    },
    Dtc {
        peer: Peer,
        dtc: u32,
        state: Retained,
    },
    DtcClear(DtcScope),
    EverClear(EverScope),
    Seed(u32),
}

fn dtc_key((link, addr): Peer, dtc: u32) -> [u8; 5] {
    let d = dtc.to_be_bytes();
    [link, addr, d[1], d[2], d[3]]
}

impl DiagRecord {
    pub fn encode(&self) -> Keyed {
        match *self {
            DiagRecord::Ever { peer, id } => Keyed::put(CLASS_EVER, &[peer.0, peer.1], &id),
            DiagRecord::Dtc { peer, dtc, state } => Keyed::put(
                CLASS_DTC,
                &dtc_key(peer, dtc),
                &[state.status, state.age, state.fails],
            ),
            DiagRecord::DtcClear(scope) => {
                let key = match scope {
                    DtcScope::One(p, dtc) => dtc_key(p, dtc).to_vec(),
                    DtcScope::Peer(p) => vec![p.0, p.1],
                    DtcScope::All => vec![],
                };
                Keyed::erase(CLASS_DTC, CLASS_DTC, &key)
            }
            DiagRecord::EverClear(scope) => {
                let key = match scope {
                    EverScope::Peer(p) => vec![p.0, p.1],
                    EverScope::All => vec![],
                };
                Keyed::erase(CLASS_EVER, CLASS_DTC, &key)
            }
            DiagRecord::Seed(counter) => Keyed::put(CLASS_SEED, &[], &counter.to_le_bytes()),
        }
    }

    /// `None` for a keyed record that is none of the five in their one
    /// form (another class, another key length, a DTC number that names no
    /// fault, a status bit outside 1 to 6, a byte that must be zero).
    pub fn decode(k: &Keyed) -> Option<DiagRecord> {
        let (key, v) = (k.key(), k.value());
        let peer = |key: &[u8]| (key[0], key[1]);
        let r = match (k.erase, k.class, key.len()) {
            (false, CLASS_EVER, 2) => DiagRecord::Ever {
                peer: peer(key),
                id: v[..16].try_into().ok()?,
            },
            (false, CLASS_DTC, 5) => DiagRecord::Dtc {
                peer: peer(key),
                dtc: u32::from_be_bytes([0, key[2], key[3], key[4]]),
                state: Retained {
                    status: v[0],
                    age: v[1],
                    fails: v[2],
                },
            },
            (false, CLASS_SEED, 0) => DiagRecord::Seed(u32::from_le_bytes(v[..4].try_into().ok()?)),
            (true, CLASS_DTC, 5) => DiagRecord::DtcClear(DtcScope::One(
                peer(key),
                u32::from_be_bytes([0, key[2], key[3], key[4]]),
            )),
            (true, CLASS_DTC, 2) => DiagRecord::DtcClear(DtcScope::Peer(peer(key))),
            (true, CLASS_DTC, 0) => DiagRecord::DtcClear(DtcScope::All),
            (true, CLASS_EVER, 2) => DiagRecord::EverClear(EverScope::Peer(peer(key))),
            (true, CLASS_EVER, 0) => DiagRecord::EverClear(EverScope::All),
            _ => return None,
        };
        let named = match r {
            DiagRecord::Dtc { dtc, state, .. } => {
                (1..0xFF_FFFF).contains(&dtc) && state.status & !RETAINED == 0
            }
            DiagRecord::DtcClear(DtcScope::One(_, dtc)) => (1..0xFF_FFFF).contains(&dtc),
            _ => true,
        };
        (named
            && Keyed {
                seq: k.seq,
                ..r.encode()
            } == *k)
            .then_some(r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedError {
    /// The counter is at `u32::MAX`; it never wraps.
    Exhausted,
    Store(Error),
}

/// The diagnostics store of one running app over its journal.
#[derive(Clone, Debug)]
pub struct Store {
    pub j: Journal,
    params: Params,
    /// `diag_peers`: peers the table takes.
    peers_max: usize,
    /// The DTC number the monitor samples.
    dtc: u32,
    /// FDC of this cycle per peer (RAM; 0 until a sample).
    fdc: BTreeMap<Peer, i8>,
    /// Diagnostics records appended in this cycle.
    pub appends: u32,
    /// A new peer was not recorded: the table was full.
    pub full: bool,
}

impl Store {
    pub fn open(j: Journal, params: Params, peers_max: usize, dtc: u32) -> Store {
        Store {
            j,
            params,
            peers_max,
            dtc,
            fdc: BTreeMap::new(),
            appends: 0,
            full: false,
        }
    }

    fn decoded(&self) -> impl Iterator<Item = DiagRecord> + '_ {
        self.j.keyed.iter().filter_map(DiagRecord::decode)
    }

    /// REPORT 2: the peers ever communicated with.
    pub fn ever(&self) -> BTreeMap<Peer, Id> {
        self.decoded()
            .filter_map(|r| match r {
                DiagRecord::Ever { peer, id } => Some((peer, id)),
                _ => None,
            })
            .collect()
    }

    /// The retained DTC states, by `(peer, dtc)`.
    pub fn dtcs(&self) -> BTreeMap<(Peer, u32), Retained> {
        self.decoded()
            .filter_map(|r| match r {
                DiagRecord::Dtc { peer, dtc, state } => Some(((peer, dtc), state)),
                _ => None,
            })
            .collect()
    }

    /// The last seed counter committed (0: none).
    pub fn counter(&self) -> u32 {
        self.decoded()
            .find_map(|r| match r {
                DiagRecord::Seed(c) => Some(c),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// FDC of this cycle.
    pub fn fdc(&self, peer: Peer) -> i8 {
        self.fdc.get(&peer).copied().unwrap_or(0)
    }

    fn put(&mut self, mem: &mut Mem, r: DiagRecord) -> Result<(), Error> {
        self.j.append_keyed(mem, r.encode())?;
        self.appends += 1;
        Ok(())
    }

    fn state(&self, peer: Peer) -> Retained {
        *self.dtcs().get(&(peer, self.dtc)).unwrap_or(&CLEARED)
    }

    fn set(&mut self, mem: &mut Mem, peer: Peer, to: Retained) -> Result<(), Error> {
        if to == self.state(peer) {
            return Ok(());
        }
        let dtc = self.dtc;
        self.put(
            mem,
            DiagRecord::Dtc {
                peer,
                dtc,
                state: to,
            },
        )
    }

    /// The app's start: a new cycle begins and the one that ended is
    /// evaluated, one record per DTC whose retained state changes.
    pub fn cycle_start(&mut self, mem: &mut Mem) -> Result<(), Error> {
        self.fdc.clear();
        self.appends = 0;
        for ((peer, dtc), r) in self.dtcs() {
            let to = evaluate(r, &self.params);
            if to != r {
                self.put(
                    mem,
                    DiagRecord::Dtc {
                        peer,
                        dtc,
                        state: to,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// A pass sample: the peer's ever-communicated record when the fold
    /// holds none (not only the first time: a torn one is written again),
    /// then its DTC state when that changes.
    pub fn pass(&mut self, mem: &mut Mem, peer: Peer, id: Id) -> Result<(), Error> {
        let ever = self.ever();
        if !ever.contains_key(&peer) {
            if ever.len() >= self.peers_max {
                self.full = true;
                return Ok(());
            }
            self.fdc.insert(peer, i8::MIN);
            self.put(mem, DiagRecord::Ever { peer, id })?;
        }
        self.fdc.insert(peer, i8::MIN);
        self.set(mem, peer, on_pass(self.state(peer)))
    }

    /// A fail sample. A peer never communicated with gives none.
    pub fn fail(&mut self, mem: &mut Mem, peer: Peer) -> Result<(), Error> {
        if !self.ever().contains_key(&peer) {
            return Ok(());
        }
        let fdc = (i16::from(self.fdc(peer)) + i16::from(self.params.step)).min(127) as i8;
        self.fdc.insert(peer, fdc);
        if fdc < i8::MAX {
            return Ok(());
        }
        self.set(mem, peer, on_fail(self.state(peer), &self.params))
    }

    fn erase(&mut self, mem: &mut Mem, r: DiagRecord) -> Result<(), Error> {
        let e = r.encode();
        if !self.j.keyed.iter().any(|o| e.supersedes(o)) {
            return Ok(());
        }
        self.put(mem, r)
    }

    pub fn clear_dtc(&mut self, mem: &mut Mem, scope: DtcScope) -> Result<(), Error> {
        self.fdc.retain(|p, _| !scope.covers(*p));
        self.erase(mem, DiagRecord::DtcClear(scope))
    }

    pub fn clear_ever(&mut self, mem: &mut Mem, scope: EverScope) -> Result<(), Error> {
        self.fdc.retain(|p, _| !scope.covers(*p));
        self.erase(mem, DiagRecord::EverClear(scope))?;
        self.full = false;
        Ok(())
    }

    /// The next seed counter, committed before it is returned.
    pub fn seed(&mut self, mem: &mut Mem) -> Result<u32, SeedError> {
        let next = self.counter().checked_add(1).ok_or(SeedError::Exhausted)?;
        self.put(mem, DiagRecord::Seed(next))
            .map_err(SeedError::Store)?;
        Ok(next)
    }
}
