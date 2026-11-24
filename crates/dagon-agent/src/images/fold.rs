//! The image machines, driven by [`ImageStore`] itself, agree with the
//! state log's fold: every sequence of what the agent writes (transfers,
//! switches, confirms, through `ImageStore`) and what the boot core writes
//! (boot attempts, rollbacks, or nothing when it cannot append, followed
//! by a restart), from a provisioned device, leaves each slot's machine in
//! the state STATUS shows, and a CONFIRM confirms only the slot the boot
//! core started. So no state the fold reaches exists without the
//! transitions that lead there, and the agent takes them, the second
//! slot's included. STATUS is the kind S stores' own mapping
//! ([`LogState::status`]), so a rolled-back marker that outlives its image
//! (a new image written into the slot, a confirmed image demoted after its
//! re-trial was rolled back) fails here.

extern crate std;

use dagon_port::{SlotStore, StoreError};
use std::collections::BTreeSet;
use std::vec::Vec;

use dagon_core::log::{LogState, SlotRef};
use dagon_events::{compose_unpublished, Layer, Nop};
use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{kind, reason, slot as log_slot, target as log_target, Record};
use dagon_format::Version;
use dagon_proto::payload::{target, SlotState, StatusResp};

use super::{image, ImageStore, Transition, ROOM};

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
    /// A reset where the boot core refuses the pending image.
    Revert,
    /// A reset where the boot core cannot append to the log (its tail cut
    /// by a power loss) and starts the confirmed slot, the trial the log
    /// names left open.
    Stuck,
}

const OPS: [Op; 8] = [
    Op::Transfer(0),
    Op::Transfer(1),
    Op::SetPending(0),
    Op::SetPending(1),
    Op::Confirm,
    Op::Boot,
    Op::Revert,
    Op::Stuck,
];

/// A kind S slot store over the state log's fold: a header per slot (its
/// first byte stands for its digest; 0xFF = erased) and the log folded
/// into [`LogState`], and the slot the boot core started.
#[derive(Clone, Copy, Debug)]
struct FoldStore {
    log: LogState,
    header: [u8; ROOM],
    running: u8,
}

const ERASED: u8 = 0xFF;

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

impl FoldStore {
    fn provisioned() -> Self {
        FoldStore {
            log: LogState::default().apply(&record(kind::PROVISION, 0, 1, reason::NONE)),
            header: [1, ERASED],
            running: 0,
        }
    }

    fn append(&mut self, k: u8, slot: u8, why: u8) {
        let d = self.header[usize::from(slot)];
        self.log = self.log.apply(&record(k, slot, d, why));
    }

    /// The boot core at a reset: start the pending slot (when its image is
    /// intact, attempts are left and `start`), else roll it back; `stuck`:
    /// it appends nothing and starts the confirmed slot.
    fn boot(&mut self, start: bool, stuck: bool) -> bool {
        let Some(p) = self.log.pending else {
            return false;
        };
        let back = self.log.confirmed.map_or(log_slot::NONE, |c| c.slot);
        let intact = p.digest16 == [self.header[usize::from(p.slot)]; 16];
        if stuck {
            self.running = back;
        } else if start && intact && self.log.attempts < MAX {
            self.append(kind::BOOT_ATTEMPT, p.slot, reason::NONE);
            self.running = p.slot;
        } else {
            self.running = back;
            self.log = self
                .log
                .apply(&record(kind::ROLLBACK, back, 0, reason::TRIAL_EXHAUSTED));
        }
        true
    }

    fn header_bytes(&self, slot: u8) -> [u8; HEADER_SIZE] {
        [self.header[usize::from(slot)]; HEADER_SIZE]
    }
}

fn digest_of(hdr: &[u8; HEADER_SIZE]) -> u8 {
    hdr[0]
}

impl SlotStore for FoldStore {
    type Transition = Transition;

    fn status(&mut self) -> StatusResp {
        let heads: [_; ROOM] = core::array::from_fn(|i| {
            let s = i as u8;
            let d = self.header[i];
            (d != ERASED).then(|| SlotRef {
                slot: s,
                version: Version::new(1, 0, d.into()),
                digest16: [d; 16],
            })
        });
        self.log.status(&heads, self.running, 0)
    }

    fn capacity(&self, t: u8, slot: u8) -> u32 {
        u32::from(t == target::APP && slot < 2)
    }

    fn erase(&mut self, _: u8, slot: u8, _: u32, _: &Transition) -> Result<(), StoreError> {
        self.header[usize::from(slot)] = ERASED;
        Ok(())
    }

    fn write(&mut self, _: u8, _: u8, _: u32, _: &[u8], _: &Transition) -> Result<(), StoreError> {
        Ok(())
    }

    fn read(&mut self, _: u8, _: u8, _: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        buf.fill(0);
        Ok(())
    }

    fn write_header(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.header[usize::from(slot)] = digest_of(hdr);
        Ok(())
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        *out = self.header_bytes(slot);
        Ok(())
    }

    fn set_pending(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.log = self.log.apply(&record(
            kind::SLOT_PENDING,
            slot,
            digest_of(hdr),
            reason::NONE,
        ));
        Ok(())
    }

    fn confirm(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.log = self
            .log
            .apply(&record(kind::CONFIRMED, slot, digest_of(hdr), reason::NONE));
        Ok(())
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

/// One device: the store, the agent's image machines over it, and the
/// next header digest a transfer writes.
struct Model {
    images: ImageStore<FoldStore>,
    next_digest: u8,
}

impl Model {
    fn new() -> Self {
        Model {
            images: ImageStore::open(FoldStore::provisioned(), MAX, true),
            next_digest: 2,
        }
    }

    /// The same device; its machines start from the store, which they
    /// agree with ([`Model::agrees`] held before).
    fn copy(&mut self) -> Self {
        Model {
            images: ImageStore::open(*self.images.store(), MAX, true),
            next_digest: self.next_digest,
        }
    }

    /// Runs `op` the way the agent and the boot core do; `false` when a
    /// writer's guard keeps it from happening. A store call the agent's
    /// guards allow must be accepted by the machine.
    fn step(&mut self, op: Op) -> bool {
        let mut out = compose_unpublished(Layer::App, Nop, Nop);
        let st = self.images.status();
        let v = Version::default();
        match op {
            Op::Transfer(s) => {
                if s == st.active || s == st.confirmed {
                    return false;
                }
                let hdr = [self.next_digest; HEADER_SIZE];
                self.next_digest += 1;
                let r = self.images.erase(target::APP, s, 1, &mut out);
                assert_eq!(r, Ok(()), "erase {s}");
                let r =
                    self.images
                        .write_header((target::APP, s), &hdr, (v, 1), |_| true, &mut out);
                assert_eq!(r, Ok(()), "written {s}");
            }
            Op::SetPending(s) => {
                let hdr = self.images.store().header_bytes(s);
                if s == st.active || hdr[0] == ERASED {
                    return false;
                }
                let r = self.images.set_pending(s, &hdr, v, &mut out);
                assert_eq!(r, Ok(()), "pending_set {s}");
            }
            Op::Confirm => {
                let a = st.active;
                if st.slots.get(usize::from(a)).map(|i| i.state) != Some(SlotState::Trying) {
                    return false;
                }
                // Only the image that runs is confirmed: never a pending
                // one the boot core could not start.
                assert_eq!(a, self.images.store().running, "confirm of {a}");
                let hdr = self.images.store().header_bytes(a);
                let r = self.images.confirm(a, &hdr, v, &mut out);
                assert_eq!(r, Ok(()), "confirmed {a}");
            }
            Op::Boot | Op::Revert | Op::Stuck => {
                let store = self.images.store();
                if !store.boot(matches!(op, Op::Boot), matches!(op, Op::Stuck)) {
                    return false;
                }
                self.images.reopen();
            }
        }
        true
    }

    fn agrees(&mut self, path: &[Op]) {
        let st = self.images.status();
        for (s, info) in (0u8..).zip(&st.slots[..ROOM]) {
            assert_eq!(
                self.images.view().state(s),
                Some(as_machine(info.state)),
                "slot {s} after {path:?} ({:?})",
                self.images.store()
            );
        }
    }
}

fn explore(model: &mut Model, path: &mut Vec<Op>, seen: &mut BTreeSet<&'static str>) -> usize {
    model.agrees(path);
    for s in 0..2 {
        if let Some(state) = model.images.view().state(s) {
            seen.insert(state.name());
        }
    }
    let mut checked = 1;
    if path.len() == DEPTH {
        return checked;
    }
    for op in OPS {
        let mut next = model.copy();
        if next.step(op) {
            path.push(op);
            checked += explore(&mut next, path, seen);
            path.pop();
        }
    }
    checked
}

#[test]
fn the_machines_agree_with_the_state_log_fold() {
    let mut seen = BTreeSet::new();
    let checked = explore(&mut Model::new(), &mut Vec::new(), &mut seen);
    assert!(checked > 300_000, "{checked} sequences");
    assert_eq!(seen.len(), image::CATALOG.states.len(), "{seen:?}");
}

/// The two ways a rolled-back marker could outlive its image: a new image
/// written into the rolled-back slot, and a confirmed image whose re-trial
/// was rolled back, demoted later. Neither slot shows Reverted then.
#[test]
fn a_rolled_back_marker_names_one_image() {
    use Op::*;
    let cases: [&[Op]; 2] = [
        &[Transfer(1), SetPending(1), Revert, Transfer(1)],
        &[
            Transfer(1),
            SetPending(1),
            Boot,
            SetPending(0),
            Revert,
            Transfer(1),
            SetPending(1),
            Boot,
            Confirm,
        ],
    ];
    for path in cases {
        let mut m = Model::new();
        for (i, op) in path.iter().enumerate() {
            assert!(m.step(*op), "{op:?} refused after {:?}", &path[..i]);
            m.agrees(&path[..=i]);
        }
        let st = m.images.status();
        assert!(
            st.slots[..ROOM]
                .iter()
                .all(|s| s.state != SlotState::Reverted),
            "{path:?}: {st:?}"
        );
    }
}
