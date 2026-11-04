//! The image lifecycle machine around the board's slot store: the only
//! holder of the store's mutators, and the read-only view apps get.

use dagon_events::{compose_unpublished, EventRecord, Layer, Nop, Payload, Refused, Sink};
use dagon_format::header::HEADER_SIZE;
use dagon_format::Version;
use dagon_proto::payload::{target, SlotState, StatusResp};
use dagon_proto::Status;

use crate::agent::AppChain;
use crate::{image, SlotStore, StoreError};

/// Proof that the image lifecycle machine is changing a slot: the
/// [`SlotStore`] mutators take one, and only this crate makes one, inside
/// the transition that emits the change's event. Code that holds the store
/// for its other resources cannot change image state with it.
///
/// ```compile_fail,E0423
/// let t = dagon_agent::Transition(());
/// ```
#[derive(Debug)]
pub struct Transition(());

impl Transition {
    /// A transition for a [`SlotStore`] implementation's own tests, which
    /// call its mutators directly. Host test builds only (feature
    /// `store-tests`).
    #[cfg(feature = "store-tests")]
    pub fn for_store_tests() -> Self {
        Transition(())
    }
}

/// What an app may know about its slots, read-only: the last STATUS read
/// and each APP slot's lifecycle state.
#[derive(Clone, Debug)]
pub struct ImageView {
    status: StatusResp,
    slots: [image::State; 2],
}

impl ImageView {
    /// The slot store's STATUS as last read.
    pub fn status(&self) -> &StatusResp {
        &self.status
    }

    /// The running slot, if any.
    pub fn active(&self) -> Option<u8> {
        (usize::from(self.status.active) < self.slots.len()).then_some(self.status.active)
    }

    /// Whether the running slot is on trial (not confirmed yet).
    pub fn on_trial(&self) -> bool {
        self.active()
            .is_some_and(|s| self.slots[usize::from(s)] == image::State::Trial)
    }

    /// The running image's version.
    pub fn own_version(&self) -> Option<Version> {
        self.active()
            .map(|s| self.status.slots[usize::from(s)].version)
    }

    /// An APP slot's lifecycle state.
    pub fn state(&self, slot: u8) -> Option<image::State> {
        self.slots.get(usize::from(slot)).copied()
    }
}

/// The slot store behind the image lifecycle machines.
pub(crate) struct ImageStore<S> {
    store: S,
    app: [image::Machine; 2],
    topology: image::Machine,
    view: ImageView,
}

/// Whether `trigger` leaves from `state`, read from the machine's catalog
/// so a store call is made only when its transition will follow.
fn allows(trigger: &str, state: image::State) -> bool {
    image::CATALOG
        .entries
        .iter()
        .filter(|e| e.trigger == trigger)
        .any(|e| e.from.iter().any(|f| *f == "*" || *f == state.name()))
}

/// A transition [`allows`] admitted cannot be refused.
fn done(r: Result<(), Refused>) {
    debug_assert!(r.is_ok(), "{r:?}");
}

pub(crate) fn store_status(e: StoreError) -> Status {
    match e {
        StoreError::Flash => Status::Flash,
        StoreError::Range => Status::Range,
        StoreError::Unsupported => Status::Unsupported,
    }
}

impl<S: SlotStore> ImageStore<S> {
    /// Takes the store and brings each slot's machine to the state the
    /// store reports.
    pub(crate) fn open(mut store: S) -> Self {
        let status = store.status();
        let mut app = [image::Machine::new(), image::Machine::new()];
        // The transitions that led here were observed when they happened;
        // reaching their state again delivers to no one.
        let mut silent = compose_unpublished(Layer::App, Nop, Nop);
        for (s, m) in (0u8..).zip(app.iter_mut()) {
            let info = status.slots[usize::from(s)];
            reach(m, &mut silent, s, info.state, info.version, &status);
        }
        ImageStore {
            store,
            view: ImageView {
                status,
                slots: [app[0].state(), app[1].state()],
            },
            app,
            topology: image::Machine::new(),
        }
    }

    /// The boot core's changes the store shows (a slot on trial, a
    /// rolled-back slot), replayed into `chain` as `trial_started` and
    /// `rolled_back` with `replayed: true`.
    pub(crate) fn replay_boot(&self, chain: &mut impl AppChain) {
        let st = &self.view.status;
        for (s, m) in (0u8..).zip(self.app.iter()) {
            let info = st.slots[usize::from(s)];
            let payload: Option<&dyn Payload> = match m.state() {
                image::State::Trial => Some(&image::TrialStarted {
                    slot: s,
                    version: info.version,
                    attempt: st.attempts,
                }),
                image::State::Reverted => Some(&image::RolledBack {
                    from: s,
                    to: st.active,
                    reason: st.last_reason,
                }),
                _ => None,
            };
            if let Some(p) = payload {
                let e = p.entry();
                let rec = EventRecord {
                    layer: e.layer.code(),
                    kind: e.code,
                    payload: p.compact(),
                    ..EventRecord::default()
                };
                let replayed = chain.replay(&[image::CATALOG], &rec);
                debug_assert!(replayed.is_ok());
            }
        }
    }

    pub(crate) fn view(&self) -> &ImageView {
        &self.view
    }

    /// The store, for the board's other resources. Its mutators need a
    /// [`Transition`], which this crate makes only inside a transition.
    pub(crate) fn store(&mut self) -> &mut S {
        &mut self.store
    }

    fn refresh_view(&mut self, status: StatusResp) {
        self.view = ImageView {
            status,
            slots: [self.app[0].state(), self.app[1].state()],
        };
    }

    /// STATUS from the store (the view follows it).
    pub(crate) fn status(&mut self) -> StatusResp {
        let st = self.store.status();
        self.refresh_view(st);
        st
    }

    pub(crate) fn capacity(&self, t: u8, slot: u8) -> u32 {
        self.store.capacity(t, slot)
    }

    pub(crate) fn read(&mut self, t: u8, slot: u8, off: u32, buf: &mut [u8]) -> Result<(), Status> {
        self.store.read(t, slot, off, buf).map_err(store_status)
    }

    pub(crate) fn read_header(
        &mut self,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), Status> {
        self.store.read_header(slot, out).map_err(store_status)
    }

    fn machine(&mut self, t: u8, slot: u8) -> Result<&mut image::Machine, Status> {
        match t {
            target::APP => self.app.get_mut(usize::from(slot)).ok_or(Status::Arg),
            target::TOPOLOGY if slot == 0 => Ok(&mut self.topology),
            _ => Err(Status::Unsupported),
        }
    }

    /// `erase`: the slot's old image is gone, a new body may be written.
    pub(crate) fn erase(
        &mut self,
        t: u8,
        slot: u8,
        len: u32,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        if !allows("erase", self.machine(t, slot)?.state()) {
            return Err(Status::State);
        }
        self.store
            .erase(t, slot, len, &Transition(()))
            .map_err(store_status)?;
        done(self.machine(t, slot)?.erase(out, t, slot, len));
        Ok(())
    }

    /// Body bytes into a slot erased by [`ImageStore::erase`].
    pub(crate) fn write_body(
        &mut self,
        t: u8,
        slot: u8,
        off: u32,
        data: &[u8],
    ) -> Result<(), Status> {
        if self.machine(t, slot)?.state() != image::State::Empty {
            return Err(Status::State);
        }
        self.store
            .write(t, slot, off, data, &Transition(()))
            .map_err(store_status)
    }

    /// `written` for an APP slot: the verified header goes in last and must
    /// read back equal and pass `verify`.
    pub(crate) fn write_header(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        version: Version,
        verify: impl FnOnce(&[u8; HEADER_SIZE]) -> bool,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        if !allows("written", self.machine(target::APP, slot)?.state()) {
            return Err(Status::State);
        }
        self.store
            .write_header(slot, hdr, &Transition(()))
            .map_err(store_status)?;
        let mut back = [0u8; HEADER_SIZE];
        self.store
            .read_header(slot, &mut back)
            .map_err(store_status)?;
        if back != *hdr || !verify(&back) {
            return Err(Status::Flash);
        }
        done(self.app[usize::from(slot)].written(out, target::APP, slot, version));
        self.status();
        Ok(())
    }

    /// `written` for a target without a header (the topology copy).
    pub(crate) fn body_written(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        let m = self.machine(t, slot)?;
        if !allows("written", m.state()) {
            return Err(Status::State);
        }
        done(m.written(out, t, slot, Version::default()));
        Ok(())
    }

    /// `pending_set`; the slot that was pending before follows what the
    /// store now shows: `pending_moved` (an image again) or
    /// `pending_withdrawn` (the confirmed one again).
    pub(crate) fn set_pending(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        version: Version,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        let i = usize::from(slot);
        if !allows("pending_set", self.machine(target::APP, slot)?.state()) {
            return Err(Status::State);
        }
        self.store
            .set_pending(slot, hdr, &Transition(()))
            .map_err(store_status)?;
        done(self.app[i].pending_set(out, slot, version));
        let after = self.status();
        let other = 1 - i;
        let o = other as u8;
        if matches!(
            self.app[other].state(),
            image::State::Pending | image::State::Trial
        ) {
            match after.slots[other].state {
                SlotState::Written => done(self.app[other].pending_moved(out, o, slot)),
                SlotState::Confirmed => {
                    let v = after.slots[other].version;
                    done(self.app[other].pending_withdrawn(out, o, v));
                }
                _ => {}
            }
        }
        self.refresh_view(after);
        Ok(())
    }

    /// `confirmed`, and `demoted` for the slot that was confirmed before
    /// when the store no longer shows it so. The caller asked `self_check`.
    pub(crate) fn confirm(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        version: Version,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        let i = usize::from(slot);
        if !allows("confirmed", self.machine(target::APP, slot)?.state()) {
            return Err(Status::State);
        }
        self.store
            .confirm(slot, hdr, &Transition(()))
            .map_err(store_status)?;
        done(self.app[i].confirmed(out, slot, version));
        let after = self.status();
        let other = 1 - i;
        if self.app[other].state() == image::State::Confirmed
            && after.slots[other].state != SlotState::Confirmed
        {
            let v = after.slots[other].version;
            done(self.app[other].demoted(out, other as u8, v));
        }
        self.refresh_view(after);
        Ok(())
    }
}

/// Moves a fresh machine to `state` through the declared transitions.
fn reach(
    m: &mut image::Machine,
    out: &mut impl Sink,
    slot: u8,
    state: SlotState,
    version: Version,
    st: &StatusResp,
) {
    use image::State as I;
    let path: &[I] = match state {
        SlotState::Empty => &[],
        SlotState::Written => &[I::Written],
        SlotState::Pending => &[I::Written, I::Pending],
        SlotState::Trying => &[I::Written, I::Pending, I::Trial],
        SlotState::Confirmed => &[I::Written, I::Pending, I::Trial, I::Confirmed],
        SlotState::Reverted => &[I::Written, I::Pending, I::Reverted],
    };
    for step in path {
        let r = match step {
            I::Written => m.written(out, target::APP, slot, version),
            I::Pending => m.pending_set(out, slot, version),
            I::Trial => m.trial_started(out, slot, version, st.attempts),
            I::Confirmed => m.confirmed(out, slot, version),
            I::Reverted => m.rolled_back(out, slot, st.active, st.last_reason),
            I::Empty => Ok(()),
        };
        done(r);
    }
}

#[cfg(test)]
mod fold;
