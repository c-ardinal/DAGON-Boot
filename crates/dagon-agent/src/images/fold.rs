//! The image machines, driven by [`ImageStore`] itself, agree with the
//! state log's fold: every sequence of what the agent writes (transfers,
//! switches, confirms, through `ImageStore`) and what the boot core writes
//! (boot attempts, rollbacks, followed by a restart), from a provisioned
//! device, leaves each slot's machine in the state STATUS shows. So no
//! state the fold reaches exists without the transitions that lead there,
//! and the agent takes them, the second slot's included.

extern crate std;

use std::collections::BTreeSet;
use std::vec::Vec;

use dagon_core::log::LogState;
use dagon_events::{compose_unpublished, Layer, Nop};
use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{kind, reason, slot as log_slot, target as log_target, Record};
use dagon_format::Version;
use dagon_proto::payload::{target, SlotState, StatusResp};

use super::{image, ImageStore, Transition};
use crate::{SlotStore, StoreError};

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

/// A kind S slot store over the state log's fold: a header per slot (its
/// first byte stands for its digest; 0xFF = erased) and the log folded
/// into [`LogState`].
#[derive(Clone, Copy, Debug)]
struct FoldStore {
    log: LogState,
    header: [u8; 2],
    /// The rolled-back slot and the header it held then.
    reverted: Option<(u8, u8)>,
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
            reverted: None,
        }
    }

    fn append(&mut self, k: u8, slot: u8, why: u8) {
        let d = self.header[usize::from(slot)];
        self.log = self.log.apply(&record(k, slot, d, why));
    }

    fn is_confirmed(&self, slot: u8) -> bool {
        self.log
            .confirmed
            .is_some_and(|c| c.slot == slot && c.digest16 == [self.header[usize::from(slot)]; 16])
    }

    /// The boot core at a reset: start the pending slot (when its image is
    /// intact, attempts are left and `start`), else roll it back.
    fn boot(&mut self, start: bool) -> bool {
        let Some(p) = self.log.pending else {
            return false;
        };
        let intact = p.digest16 == [self.header[usize::from(p.slot)]; 16];
        if start && intact && self.log.attempts < MAX {
            self.append(kind::BOOT_ATTEMPT, p.slot, reason::NONE);
        } else {
            // A re-tried confirmed image is not rolled back: it stays the
            // confirmed one.
            self.reverted = (!self.is_confirmed(p.slot)).then_some((p.slot, p.digest16[0]));
            let back = self.log.confirmed.map_or(log_slot::NONE, |c| c.slot);
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
    /// A header makes a slot Written, the log upgrades the slot whose
    /// header it names. The rolled-back marker names an image: it holds
    /// while the slot keeps the header that was rolled back (the log's
    /// `reverted` names only a slot, so the store pairs it with the header
    /// it held then).
    fn status(&mut self) -> StatusResp {
        let mut st = StatusResp {
            active: self.log.active(),
            confirmed: self.log.confirmed.map_or(log_slot::NONE, |c| c.slot),
            attempts: self.log.attempts,
            last_reason: self.log.last_reason,
            ..Default::default()
        };
        for (info, d) in st.slots.iter_mut().zip(self.header) {
            if d != ERASED {
                info.state = SlotState::Written;
                info.version = Version::new(1, 0, d.into());
            }
        }
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
            if st.slots[s].state != SlotState::Empty && r.digest16 == [self.header[s]; 16] {
                st.slots[s].state = state;
            }
        }
        if let Some((s, d)) = self.reverted {
            let i = usize::from(s);
            let info = &mut st.slots[i];
            if self.log.reverted == Some(s)
                && info.state == SlotState::Written
                && self.header[i] == d
            {
                info.state = SlotState::Reverted;
            }
        }
        st
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
            images: ImageStore::open(FoldStore::provisioned()),
            next_digest: 2,
        }
    }

    /// The same device; its machines start from the store, which they
    /// agree with ([`Model::agrees`] held before).
    fn copy(&mut self) -> Self {
        Model {
            images: ImageStore::open(*self.images.store()),
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
                let r = self
                    .images
                    .write_header(s, &hdr, (v, 1), |_| true, &mut out);
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
                let hdr = self.images.store().header_bytes(a);
                let r = self.images.confirm(a, &hdr, v, &mut out);
                assert_eq!(r, Ok(()), "confirmed {a}");
            }
            Op::Boot | Op::Revert => {
                if !self.images.store().boot(matches!(op, Op::Boot)) {
                    return false;
                }
                self.images.reopen();
            }
        }
        true
    }

    fn agrees(&mut self, path: &[Op]) {
        let st = self.images.status();
        for (s, info) in (0u8..).zip(&st.slots[..2]) {
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
