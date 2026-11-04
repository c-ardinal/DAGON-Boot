//! The image machine agrees with the state log's fold: every sequence of
//! what the agent and the boot core write (transfers, switches, confirms,
//! boot attempts, rollbacks), from a provisioned device, leaves each
//! slot's machine in the state STATUS shows. So no state the fold reaches
//! exists without the transitions that lead there.

use dagon_core::log::LogState;
use dagon_events::{compose_unpublished, Layer, Nop};
use dagon_format::statelog::{kind, reason, target as log_target, Record};
use dagon_format::Version;
use dagon_proto::payload::SlotState;

use super::{image, reach};

/// The boot core's attempt limit in this model.
const MAX: u8 = 3;
/// Operations per sequence; every sequence up to this length is checked.
const DEPTH: usize = 12;

#[derive(Clone, Copy, Debug)]
enum Op {
    /// The agent receives a new image into the slot (erase, then written).
    Transfer(u8),
    /// SET_PENDING.
    SetPending(u8),
    /// CONFIRM of the slot on trial.
    Confirm,
    /// A reset: the boot core starts the pending slot or rolls it back.
    Boot,
    /// The boot core refuses the pending image (its check failed).
    Revert,
}

const OPS: [Op; 7] = [
    Op::Transfer(0),
    Op::Transfer(1),
    Op::SetPending(0),
    Op::SetPending(1),
    Op::Confirm,
    Op::Boot,
    Op::Revert,
];

/// The device: the state log's fold and the header in each slot.
#[derive(Clone, Copy, Debug)]
struct Device {
    log: LogState,
    /// Header digest per slot; 0 = no header.
    digest: [u8; 2],
    /// The rolled-back slot and the header it held then.
    reverted: Option<(u8, u8)>,
    next_digest: u8,
}

fn record(k: u8, slot: u8, digest: u8, why: u8) -> Record {
    Record {
        kind: k,
        target: log_target::APP,
        slot,
        reason: why,
        seq: 0,
        version: Version::new(1, 0, digest.into()),
        digest16: [digest; 16],
    }
}

impl Device {
    fn provisioned() -> Self {
        let log = LogState::default().apply(&record(kind::PROVISION, 0, 1, reason::NONE));
        Device {
            log,
            digest: [1, 0],
            reverted: None,
            next_digest: 2,
        }
    }

    fn apply(&mut self, k: u8, slot: u8, why: u8) {
        let d = self.digest[usize::from(slot)];
        self.log = self.log.apply(&record(k, slot, d, why));
    }

    /// STATUS per slot, as a kind S store folds it: a header makes a slot
    /// Written, the log upgrades the slot whose header it names. The
    /// rolled-back marker names an image: it holds while the slot keeps
    /// the header that was rolled back, and a re-tried confirmed image is
    /// never marked (the log's `reverted` names only a slot, so a store
    /// must pair it with the header it held then).
    fn status(&self) -> [SlotState; 2] {
        let mut st = self.digest.map(|d| {
            if d == 0 {
                SlotState::Empty
            } else {
                SlotState::Written
            }
        });
        let trial = if self.log.on_trial() {
            SlotState::Trying
        } else {
            SlotState::Pending
        };
        for (r, state) in [
            (self.log.confirmed, SlotState::Confirmed),
            (self.log.pending, trial),
        ] {
            let Some(r) = r else { continue };
            let s = usize::from(r.slot);
            if st[s] != SlotState::Empty && r.digest16 == [self.digest[s]; 16] {
                st[s] = state;
            }
        }
        if let Some((s, d)) = self.reverted {
            let i = usize::from(s);
            if self.log.reverted == Some(s) && st[i] == SlotState::Written && self.digest[i] == d {
                st[i] = SlotState::Reverted;
            }
        }
        st
    }

    fn confirmed(&self) -> Option<u8> {
        self.log.confirmed.map(|c| c.slot)
    }

    /// The log's confirmed image is the one in `slot`.
    fn is_confirmed(&self, slot: u8) -> bool {
        self.log
            .confirmed
            .is_some_and(|c| c.slot == slot && c.digest16 == [self.digest[usize::from(slot)]; 16])
    }
}

fn as_machine(s: SlotState) -> image::State {
    match s {
        SlotState::Empty => image::State::Empty,
        SlotState::Written => image::State::Written,
        SlotState::Pending => image::State::Pending,
        SlotState::Trying => image::State::Trial,
        SlotState::Confirmed => image::State::Confirmed,
        SlotState::Reverted => image::State::Reverted,
    }
}

struct Model {
    dev: Device,
    m: [image::Machine; 2],
}

impl Model {
    fn new() -> Self {
        let dev = Device::provisioned();
        let mut m = [image::Machine::new(), image::Machine::new()];
        let mut out = compose_unpublished(Layer::App, Nop, Nop);
        let st = dev.status();
        let status = dagon_proto::payload::StatusResp {
            active: dev.log.active(),
            ..Default::default()
        };
        for (s, mm) in (0u8..).zip(m.iter_mut()) {
            reach(
                mm,
                &mut out,
                s,
                st[usize::from(s)],
                Version::default(),
                &status,
            );
        }
        Model { dev, m }
    }

    fn copy(&self) -> Self {
        // A machine is not Clone; rebuild each through its transitions.
        let mut m = [image::Machine::new(), image::Machine::new()];
        let mut out = compose_unpublished(Layer::App, Nop, Nop);
        let status = dagon_proto::payload::StatusResp::default();
        for (i, mm) in m.iter_mut().enumerate() {
            let s = match self.m[i].state() {
                image::State::Empty => SlotState::Empty,
                image::State::Written => SlotState::Written,
                image::State::Pending => SlotState::Pending,
                image::State::Trial => SlotState::Trying,
                image::State::Confirmed => SlotState::Confirmed,
                image::State::Reverted => SlotState::Reverted,
            };
            reach(mm, &mut out, i as u8, s, Version::default(), &status);
        }
        Model { dev: self.dev, m }
    }

    /// Runs `op` on the device and the matching transitions on the
    /// machines, each derived from the log records written (not from
    /// STATUS). `false` when a writer's guard keeps it from happening.
    fn step(&mut self, op: Op) -> bool {
        let mut out = compose_unpublished(Layer::App, Nop, Nop);
        let before = self.dev.status();
        let ok = |r: Result<(), dagon_events::Refused>, what: &str| {
            assert!(r.is_ok(), "{what}: {r:?}");
        };
        match op {
            Op::Transfer(s) => {
                let i = usize::from(s);
                if s == self.dev.log.active() || Some(s) == self.dev.confirmed() {
                    return false;
                }
                ok(self.m[i].erase(&mut out, 0, s, 1), "erase");
                ok(
                    self.m[i].written(&mut out, 0, s, Version::default()),
                    "written",
                );
                self.dev.digest[i] = self.dev.next_digest;
                self.dev.next_digest += 1;
            }
            Op::SetPending(s) => {
                let i = usize::from(s);
                if s == self.dev.log.active() || self.dev.digest[i] == 0 {
                    return false;
                }
                let was = self.dev.log.pending.map(|p| p.slot);
                self.dev.apply(kind::SLOT_PENDING, s, reason::NONE);
                ok(
                    self.m[i].pending_set(&mut out, s, Version::default()),
                    "pending_set",
                );
                if let Some(o) = was.filter(|&o| o != s) {
                    let oi = usize::from(o);
                    if matches!(before[oi], SlotState::Pending | SlotState::Trying) {
                        let r = if self.dev.is_confirmed(o) {
                            self.m[oi].pending_withdrawn(&mut out, o, Version::default())
                        } else {
                            self.m[oi].pending_moved(&mut out, o, s)
                        };
                        ok(r, "pending_moved / pending_withdrawn");
                    }
                }
            }
            Op::Confirm => {
                let a = self.dev.log.active();
                let ai = usize::from(a);
                if ai >= 2 || before[ai] != SlotState::Trying {
                    return false;
                }
                let was = self.dev.confirmed();
                self.dev.apply(kind::CONFIRMED, a, reason::NONE);
                ok(
                    self.m[ai].confirmed(&mut out, a, Version::default()),
                    "confirmed",
                );
                if let Some(o) = was.filter(|&o| o != a) {
                    if before[usize::from(o)] == SlotState::Confirmed {
                        ok(
                            self.m[usize::from(o)].demoted(&mut out, o, Version::default()),
                            "demoted",
                        );
                    }
                }
            }
            Op::Boot | Op::Revert => {
                let Some(p) = self.dev.log.pending else {
                    return false;
                };
                let pi = usize::from(p.slot);
                let intact = p.digest16 == [self.dev.digest[pi]; 16];
                let startable = matches!(op, Op::Boot) && intact && self.dev.log.attempts < MAX;
                if startable {
                    self.dev.apply(kind::BOOT_ATTEMPT, p.slot, reason::NONE);
                    let r = self.m[pi].trial_started(&mut out, p.slot, Version::default(), 0);
                    ok(r, "trial_started");
                } else {
                    let back = self.dev.confirmed().unwrap_or(0xFF);
                    self.dev.log = self.dev.log.apply(&record(
                        kind::ROLLBACK,
                        back,
                        0,
                        reason::TRIAL_EXHAUSTED,
                    ));
                    // A re-tried confirmed image is not rolled back: it
                    // stays the confirmed one.
                    self.dev.reverted =
                        (!self.dev.is_confirmed(p.slot)).then_some((p.slot, p.digest16[0]));
                    if matches!(before[pi], SlotState::Pending | SlotState::Trying) {
                        let r = if self.dev.is_confirmed(p.slot) {
                            self.m[pi].pending_withdrawn(&mut out, p.slot, Version::default())
                        } else {
                            self.m[pi].rolled_back(&mut out, p.slot, back, 0)
                        };
                        ok(r, "rolled_back / pending_withdrawn");
                    }
                }
            }
        }
        true
    }

    fn agrees(&self, path: &[Op]) {
        let st = self.dev.status();
        for (i, (m, s)) in self.m.iter().zip(st).enumerate() {
            assert_eq!(
                m.state(),
                as_machine(s),
                "slot {i} after {path:?} ({:?})",
                self.dev
            );
        }
    }
}

fn explore(model: &Model, path: &mut Vec<Op>, checked: &mut usize) {
    model.agrees(path);
    *checked += 1;
    if path.len() == DEPTH {
        return;
    }
    for op in OPS {
        let mut next = model.copy();
        if next.step(op) {
            path.push(op);
            explore(&next, path, checked);
            path.pop();
        }
    }
}

extern crate std;
use std::vec::Vec;

#[test]
fn the_machine_agrees_with_the_state_log_fold() {
    let mut checked = 0;
    explore(&Model::new(), &mut Vec::new(), &mut checked);
    assert!(checked > 300_000, "{checked} sequences");
}

#[test]
fn every_fold_state_is_reached() {
    let mut seen = std::collections::BTreeSet::new();
    fn walk(m: &Model, depth: usize, seen: &mut std::collections::BTreeSet<&'static str>) {
        for s in m.dev.status() {
            seen.insert(as_machine(s).name());
        }
        if depth == 0 {
            return;
        }
        for op in OPS {
            let mut n = m.copy();
            if n.step(op) {
                walk(&n, depth - 1, seen);
            }
        }
    }
    walk(&Model::new(), DEPTH, &mut seen);
    assert_eq!(seen.len(), image::CATALOG.states.len(), "{seen:?}");
}
