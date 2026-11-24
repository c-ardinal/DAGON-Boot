//! The image lifecycle machine around the board's slot store: the only
//! holder of the store's mutators, and the read-only view apps get.

use dagon_events::{compose_unpublished, EventRecord, Layer, Nop, Payload, Refused, Replay, Sink};
use dagon_format::header::HEADER_SIZE;
use dagon_format::Version;
use dagon_port::{SlotStore, StoreError};
use dagon_proto::payload::{target, SlotState, StatusResp};
use dagon_proto::Status;

use crate::chain::AppChain;
use crate::{core_image, image};

/// The recovery agent's slot (STATUS slot 2).
const FACTORY: u8 = 2;
/// Room for an image kind's slots: STATUS's A and B entries.
const ROOM: usize = StatusResp::IMAGE_SLOTS;

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
    /// The states of the APP slots the store has (the first `count`).
    slots: [image::State; ROOM],
    count: u8,
}

impl ImageView {
    /// The slot store's STATUS as last read.
    pub fn status(&self) -> &StatusResp {
        &self.status
    }

    /// The running slot, if any.
    pub fn active(&self) -> Option<u8> {
        (self.status.active < self.count).then_some(self.status.active)
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

    /// An APP slot's lifecycle state; `None` for a slot the store does not
    /// have.
    pub fn state(&self, slot: u8) -> Option<image::State> {
        self.slots[..usize::from(self.count)]
            .get(usize::from(slot))
            .copied()
    }
}

/// The slot store behind the image lifecycle machines.
pub(crate) struct ImageStore<S> {
    store: S,
    /// Boots the boot core gives a trial (`trial_started`'s `max`).
    max: u8,
    /// Whether the store makes its log room at each start (the app's
    /// session; outside a write the boot stages never compact or erase).
    room: bool,
    /// The APP slots the store has: the first `app_slots` of `app`.
    app: [image::Machine; ROOM],
    app_slots: u8,
    /// The boot core slots the store has (none on a board that keeps none).
    cores: [core_image::Machine; ROOM],
    core_slots: u8,
    /// The recovery agent's slot (factory) of a vendor-stage2 board.
    factory: image::Machine,
    view: ImageView,
}

/// A transition the machine's generated table (`State::leaves`) admitted
/// before the store call, so the call is made only when its transition
/// will follow, cannot be refused.
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

impl<S: SlotStore<Transition = Transition>> ImageStore<S> {
    /// Takes the store and brings each slot's machine to the state the
    /// store reports; `max` is the boot core's trial attempt limit, `room`
    /// whether each start makes the store's log room.
    pub(crate) fn open(store: S, max: u8, room: bool) -> Self {
        let mut images = ImageStore {
            store,
            max,
            room,
            view: ImageView {
                status: StatusResp::default(),
                slots: [image::State::Empty; ROOM],
                count: 0,
            },
            app: core::array::from_fn(|_| image::Machine::new()),
            app_slots: 0,
            cores: core::array::from_fn(|_| core_image::Machine::new()),
            core_slots: 0,
            factory: image::Machine::new(),
        };
        images.reopen();
        images
    }

    /// The store's log room for a write about to start
    /// ([`SlotStore::keep_room`]); no image state changes.
    pub(crate) fn keep_room(&mut self) -> Result<(), Status> {
        self.store.keep_room().map_err(store_status)
    }

    /// The hand-over to the boot core's update mode
    /// ([`SlotStore::request_update_mode`]); no image state changes.
    pub(crate) fn request_update_mode(&mut self) -> Result<(), Status> {
        self.store.request_update_mode().map_err(store_status)
    }

    /// Fresh machines in the states the store reports, as at start.
    pub(crate) fn reopen(&mut self) {
        // A store that cannot make room refuses SET_PENDING later (its
        // reservation fails); nothing else depends on it here.
        if self.room {
            let _ = self.store.keep_room();
        }
        self.app_slots = self.slot_count(target::APP);
        self.core_slots = self.slot_count(target::BOOTCORE);
        let status = self.store.status();
        let mut app: [image::Machine; ROOM] = core::array::from_fn(|_| image::Machine::new());
        // The transitions that led here were observed when they happened;
        // reaching their state again delivers to no one.
        let mut silent = compose_unpublished(Layer::App, Nop, Nop);
        for (s, m) in (0..self.app_slots).zip(app.iter_mut()) {
            let info = status.slots[usize::from(s)];
            reach(
                m,
                &mut silent,
                (s, self.max),
                info.state,
                info.version,
                &status,
            );
        }
        self.app = app;
        let mut cores: [core_image::Machine; ROOM] =
            core::array::from_fn(|_| core_image::Machine::new());
        if let Some(st) = self.store.target_status(target::BOOTCORE) {
            for (s, m) in (0..self.core_slots).zip(cores.iter_mut()) {
                reach_core(m, &mut silent, s, &st);
            }
        }
        self.cores = cores;
        let mut factory = image::Machine::new();
        let f = status.slots[usize::from(FACTORY)];
        if f.state != SlotState::Empty {
            done(factory.written(&mut silent, target::AGENT, FACTORY, f.version));
        }
        self.factory = factory;
        self.refresh_view(status);
    }

    /// How many slots of target `t` the store has: slot A, then B, each
    /// while the store can receive into it.
    pub(crate) fn slot_count(&self, t: u8) -> u8 {
        (0..ROOM as u8)
            .take_while(|&s| self.store.capacity(t, s) > 0)
            .count() as u8
    }

    /// A boot core slot's lifecycle state.
    pub(crate) fn core_state(&self, slot: u8) -> Option<core_image::State> {
        self.cores[..usize::from(self.core_slots)]
            .get(usize::from(slot))
            .map(|m| m.state())
    }

    /// STATUS of `t`'s slots, `None` when the board keeps none.
    pub(crate) fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        if t == target::APP {
            return Some(self.status());
        }
        self.store.target_status(t)
    }

    /// The boot core's changes the store shows (a slot on trial, a
    /// rolled-back slot), replayed into `chain` as `trial_started` and
    /// `rolled_back` with `replayed: true`.
    pub(crate) fn replay_boot(&mut self, chain: &mut impl AppChain) {
        if let Some(cs) = self.store.target_status(target::BOOTCORE) {
            for (s, m) in (0..self.core_slots).zip(self.cores.iter()) {
                if m.state() == core_image::State::Reverted {
                    let p = core_image::CoreRolledBack {
                        from: s,
                        to: cs.active,
                        reason: cs.last_reason,
                        version: cs.slots[usize::from(s)].version,
                    };
                    replay(chain, core_image::REPLAY, &p);
                }
            }
        }
        let st = &self.view.status;
        for (s, m) in (0..self.app_slots).zip(self.app.iter()) {
            let info = st.slots[usize::from(s)];
            let payload: Option<&dyn Payload> = match m.state() {
                image::State::Trial => Some(&image::TrialStarted {
                    slot: s,
                    version: info.version,
                    attempt: st.attempts,
                    max: self.max,
                }),
                image::State::Reverted => Some(&image::RolledBack {
                    from: s,
                    to: st.active,
                    reason: st.last_reason,
                    version: info.version,
                }),
                _ => None,
            };
            if let Some(p) = payload {
                replay(chain, image::REPLAY, p);
            }
        }
    }

    pub(crate) fn view(&self) -> &ImageView {
        &self.view
    }

    /// The stage0 version mark the store reads ([`SlotStore::stage0_version`]).
    pub(crate) fn stage0_version(&mut self) -> Option<Version> {
        self.store.stage0_version()
    }

    /// The store, for the fold test's boot core.
    #[cfg(test)]
    pub(crate) fn store(&mut self) -> &mut S {
        &mut self.store
    }

    fn refresh_view(&mut self, status: StatusResp) {
        self.view = ImageView {
            status,
            slots: core::array::from_fn(|s| self.app[s].state()),
            count: self.app_slots,
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
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), Status> {
        self.store
            .read_target_header(t, slot, out)
            .map_err(store_status)
    }

    fn machine(&mut self, t: u8, slot: u8) -> Result<&mut image::Machine, Status> {
        match t {
            target::APP => self.app[..usize::from(self.app_slots)]
                .get_mut(usize::from(slot))
                .ok_or(Status::Arg),
            target::AGENT if slot == FACTORY => Ok(&mut self.factory),
            _ => Err(Status::Unsupported),
        }
    }

    fn core(&mut self, slot: u8) -> Result<&mut core_image::Machine, Status> {
        self.cores[..usize::from(self.core_slots)]
            .get_mut(usize::from(slot))
            .ok_or(Status::Arg)
    }

    /// The slot is empty (erased for a new body).
    fn is_empty(&mut self, t: u8, slot: u8) -> Result<bool, Status> {
        Ok(match t {
            target::BOOTCORE => self.core(slot)?.state() == core_image::State::Empty,
            _ => self.machine(t, slot)?.state() == image::State::Empty,
        })
    }

    /// `erase`: the slot's old image is gone, a new body may be written.
    pub(crate) fn erase(
        &mut self,
        t: u8,
        slot: u8,
        len: u32,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        let ok = match t {
            target::BOOTCORE => self.core(slot)?.state().leaves(core_image::Trigger::erase),
            _ => self.machine(t, slot)?.state().leaves(image::Trigger::erase),
        };
        if !ok {
            return Err(Status::State);
        }
        self.erase_slot(t, slot, len).map_err(store_status)?;
        match t {
            target::BOOTCORE => done(self.core(slot)?.erase(out, slot, len)),
            _ => done(self.machine(t, slot)?.erase(out, t, slot, len)),
        }
        Ok(())
    }

    /// The store's erase for a new body of `len` bytes into `slot` of `t`:
    /// the shared plan from its erase layout when it has one.
    fn erase_slot(&mut self, t: u8, slot: u8, len: u32) -> Result<(), StoreError> {
        let Some(layout) = self.store.erase_layout() else {
            return self.store.erase(t, slot, len, &Transition(()));
        };
        let plan = crate::erase_plan(&layout, t, slot, len)?;
        self.store
            .erase_planned(t, slot, len, plan, &Transition(()))
    }

    /// Body bytes into a slot erased by [`ImageStore::erase`].
    pub(crate) fn write_body(
        &mut self,
        t: u8,
        slot: u8,
        off: u32,
        data: &[u8],
    ) -> Result<(), Status> {
        if !self.is_empty(t, slot)? {
            return Err(Status::State);
        }
        self.store
            .write(t, slot, off, data, &Transition(()))
            .map_err(store_status)
    }

    /// `written` for an APP or boot core slot of `len` body bytes: the
    /// verified header goes in last and must read back equal and pass
    /// `verify`. When it does not, the slot is erased again, so flash holds
    /// no image the machine (still `Empty`) has not recorded.
    pub(crate) fn write_header(
        &mut self,
        (t, slot): (u8, u8),
        hdr: &[u8; HEADER_SIZE],
        (version, len): (Version, u32),
        verify: impl FnOnce(&[u8; HEADER_SIZE]) -> bool,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        let ok = match t {
            target::BOOTCORE => self
                .core(slot)?
                .state()
                .leaves(core_image::Trigger::written),
            _ => self
                .machine(t, slot)?
                .state()
                .leaves(image::Trigger::written),
        };
        if !ok {
            return Err(Status::State);
        }
        if let Err(s) = self.place_header(t, slot, hdr, verify) {
            let undone = self.erase_slot(t, slot, len);
            self.status();
            return Err(if undone.is_ok() { s } else { Status::Flash });
        }
        match t {
            target::BOOTCORE => done(self.cores[usize::from(slot)].written(out, slot, version)),
            _ => done(self.machine(t, slot)?.written(out, t, slot, version)),
        }
        self.status();
        Ok(())
    }

    fn place_header(
        &mut self,
        t: u8,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        verify: impl FnOnce(&[u8; HEADER_SIZE]) -> bool,
    ) -> Result<(), Status> {
        self.store
            .write_target_header(t, slot, hdr, &Transition(()))
            .map_err(store_status)?;
        let mut back = [0u8; HEADER_SIZE];
        self.store
            .read_target_header(t, slot, &mut back)
            .map_err(store_status)?;
        if back != *hdr || !verify(&back) {
            return Err(Status::Flash);
        }
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
        let state = self.machine(target::APP, slot)?.state();
        if !state.leaves(image::Trigger::pending_set) {
            return Err(Status::State);
        }
        self.store
            .set_pending(slot, hdr, &Transition(()))
            .map_err(store_status)?;
        done(self.app[i].pending_set(out, slot, version));
        let after = self.status();
        for o in (0..self.app_slots).filter(|&o| o != slot) {
            let other = usize::from(o);
            if !matches!(
                self.app[other].state(),
                image::State::Pending | image::State::Trial
            ) {
                continue;
            }
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

    /// `pending_set` of a boot core slot: stage0 tries it at the next reset.
    /// Any other slot holds the running core, which the agent never sets
    /// pending, so nothing else moves.
    pub(crate) fn set_core_pending(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        version: Version,
        out: &mut impl Sink,
    ) -> Result<(), Status> {
        if !self
            .core(slot)?
            .state()
            .leaves(core_image::Trigger::pending_set)
        {
            return Err(Status::State);
        }
        self.store
            .set_target_pending(target::BOOTCORE, slot, hdr, &Transition(()))
            .map_err(store_status)?;
        done(self.cores[usize::from(slot)].pending_set(out, slot, version));
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
        let state = self.machine(target::APP, slot)?.state();
        if !state.leaves(image::Trigger::confirmed) {
            return Err(Status::State);
        }
        self.store
            .confirm(slot, hdr, &Transition(()))
            .map_err(store_status)?;
        done(self.app[i].confirmed(out, slot, version));
        let after = self.status();
        for o in (0..self.app_slots).filter(|&o| o != slot) {
            let other = usize::from(o);
            if self.app[other].state() == image::State::Confirmed
                && after.slots[other].state != SlotState::Confirmed
            {
                let v = after.slots[other].version;
                done(self.app[other].demoted(out, o, v));
            }
        }
        self.refresh_view(after);
        Ok(())
    }
}

/// Delivers `p`, a state the boot stage made, to `chain` as a replay.
fn replay(chain: &mut impl AppChain, machine: Replay, p: &dyn Payload) {
    let rec = EventRecord {
        layer: p.layer().code(),
        kind: p.kind(),
        payload: p.compact(),
        ..EventRecord::default()
    };
    let replayed = chain.replay(&[machine], &rec);
    debug_assert!(replayed.is_ok());
}

/// Moves a fresh boot core machine to the state STATUS shows for `slot`.
fn reach_core(m: &mut core_image::Machine, out: &mut impl Sink, slot: u8, st: &StatusResp) {
    use core_image::State as C;
    let info = st.slots[usize::from(slot)];
    let path: &[C] = match info.state {
        SlotState::Empty => &[],
        SlotState::Written => &[C::Written],
        SlotState::Pending | SlotState::Trying => &[C::Written, C::Pending],
        SlotState::Confirmed => &[C::Written, C::Pending, C::Confirmed],
        SlotState::Reverted => &[C::Written, C::Pending, C::Reverted],
    };
    let v = info.version;
    for step in path {
        let r = match step {
            C::Written => m.written(out, slot, v),
            C::Pending => m.pending_set(out, slot, v),
            C::Confirmed => m.confirmed(out, slot, v),
            C::Reverted => m.rolled_back(out, slot, st.active, st.last_reason, v),
            C::Empty => Ok(()),
        };
        done(r);
    }
}

/// Moves a fresh machine to `state` through the declared transitions.
fn reach(
    m: &mut image::Machine,
    out: &mut impl Sink,
    (slot, max): (u8, u8),
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
            I::Trial => m.trial_started(out, slot, version, st.attempts, max),
            I::Confirmed => m.confirmed(out, slot, version),
            I::Reverted => m.rolled_back(out, slot, st.active, st.last_reason, version),
            I::Empty => Ok(()),
        };
        done(r);
    }
}

#[cfg(test)]
mod fold;
