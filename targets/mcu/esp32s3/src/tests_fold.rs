//! The agent's image machines over the ESP32-S3's own store (`EspStore`,
//! kind V) agree with the STATUS that store maps from otadata, for every
//! sequence of what the host writes (a transfer into either OTA slot,
//! SET_PENDING of either, CONFIRM) and what the 2nd stage does at a reset
//! (a start, or a trial never confirmed and aborted), from the factory
//! agent over blank otadata. `targets/mcu/stm32f401re/tests/fold.rs` checks
//! the same over the state log of a kind S store; here no log is kept and
//! otadata holds no digest, so a marker that outlived its image (a new
//! image in the slot whose trial was aborted) would show here.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::layout::{region_for, REGIONS, SLOT_FACTORY};
use crate::otadata::ENTRY_LEN;
use crate::store::{EspStore, Otadata};
use crate::tests::{FakeOta, Feeds};
use crate::tests_factory::{chain, image, keypair, wire, Chain, TAG};
use dagon_agent::{image::State as Image, Agent};
use dagon_ed25519::Ed25519Compact;
use dagon_format::header::*;
use dagon_port::{Flash, FlashError, FlashRegion, SlotStore, StoreError};
use dagon_proto::payload::*;
use dagon_proto::{split_status, Cmd, Frame, Status};

/// Operations per sequence; every sequence up to this length is checked.
const DEPTH: usize = 6;
/// Body bytes of the images the host writes: two chunks.
const BODY: usize = 1500;
const MAX: usize = 1024;
const PAGE: u32 = 4096;

/// NOR flash that keeps only the erase units written to (the rest reads
/// erased), shared by every handle: the search copies it at each step.
#[derive(Clone, Default)]
struct Sparse(Rc<RefCell<BTreeMap<u32, Vec<u8>>>>);

impl Sparse {
    fn copy(&self) -> Self {
        Sparse(Rc::new(RefCell::new(self.0.borrow().clone())))
    }
}

impl Flash for Sparse {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        region_for(addr, buf.len() as u32).ok_or(FlashError::Range)?;
        let pages = self.0.borrow();
        for (a, b) in (addr..).zip(buf.iter_mut()) {
            *b = pages
                .get(&(a / PAGE))
                .map_or(0xFF, |p| p[(a % PAGE) as usize]);
        }
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        region_for(addr, len).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(PAGE) || !len.is_multiple_of(PAGE) {
            return Err(FlashError::Align);
        }
        let mut pages = self.0.borrow_mut();
        for p in addr / PAGE..(addr + len) / PAGE {
            pages.remove(&p);
        }
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        region_for(addr, data.len() as u32).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let mut pages = self.0.borrow_mut();
        for (a, d) in (addr..).zip(data) {
            let b = &mut pages
                .entry(a / PAGE)
                .or_insert_with(|| vec![0xFF; PAGE as usize])[(a % PAGE) as usize];
            if *b != 0xFF {
                return Err(FlashError::Hw);
            }
            *b = *d;
        }
        Ok(())
    }
}

/// The otadata and the 2nd stage's choice, shared by every handle.
#[derive(Clone)]
struct Ota(Rc<RefCell<FakeOta>>);

impl Ota {
    fn copy(&self) -> Self {
        let o = self.0.borrow();
        Ota(Rc::new(RefCell::new(FakeOta {
            entries: o.entries,
            running: o.running,
        })))
    }
}

impl Otadata for Ota {
    fn running(&self) -> u8 {
        self.0.borrow().running()
    }
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError> {
        self.0.borrow().entries()
    }
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError> {
        self.0.borrow_mut().set_next(slot)
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        self.0.borrow_mut().confirm()
    }
    fn withdraw(&mut self, slot: u8) -> Result<(), StoreError> {
        self.0.borrow_mut().withdraw(slot)
    }
}

type Store = EspStore<Sparse, Ota, Feeds>;
type NodeAgent = Agent<Store, Chain, Ed25519Compact>;

#[derive(Clone, Copy, Debug)]
enum Act {
    /// The host writes a new image into the slot (XFER_BEGIN..XFER_END).
    Transfer(u8),
    /// SET_PENDING of the slot.
    SetPending(u8),
    /// CONFIRM of the app on trial.
    Confirm,
    /// A reset: the 2nd stage starts the selected image, or factory.
    Boot,
    /// The pending image never confirms: resets until it is aborted.
    Revert,
}

const ACTS: [Act; 7] = [
    Act::Transfer(0),
    Act::Transfer(1),
    Act::SetPending(0),
    Act::SetPending(1),
    Act::Confirm,
    Act::Boot,
    Act::Revert,
];

fn as_machine(s: SlotState) -> Image {
    match s {
        SlotState::Empty => Image::Empty,
        SlotState::Written => Image::Written,
        SlotState::Pending => Image::Pending,
        SlotState::Trying => Image::Trial,
        SlotState::Confirmed => Image::Confirmed,
        SlotState::Reverted => Image::Reverted,
    }
}

fn store(f: &Sparse, ota: &Ota) -> Store {
    EspStore::new(
        f.clone(),
        ota.clone(),
        Feeds::default(),
        &crate::ERASE_REGIONS,
    )
}

/// The agent of what runs: the recovery agent in factory, the app's in an
/// OTA slot.
fn agent(f: &Sparse, ota: &Ota) -> NodeAgent {
    let m = if ota.running() == SLOT_FACTORY {
        mode::AGENT
    } else {
        mode::APP
    };
    Agent::new(
        crate::config(m, crate::target_tag(TAG)),
        Ed25519Compact::new(*keypair().pk),
        store(f, ota),
        chain(),
    )
}

/// One node: its flash and otadata, and the agent of what runs (kept
/// across the host's requests, started again at each reset).
struct Model {
    f: Sparse,
    ota: Ota,
    agent: NodeAgent,
    seq: u8,
    /// Minor version of the next image written (each image distinct).
    next: u8,
}

impl Model {
    fn new() -> Self {
        let f = Sparse::default();
        let ota = Ota(Rc::new(RefCell::new(FakeOta::blank())));
        Model {
            agent: agent(&f, &ota),
            f,
            ota,
            seq: 0,
            next: 1,
        }
    }

    /// The same node on a flash of its own; its agent starts from it.
    fn copy(&self) -> Self {
        let (f, ota) = (self.f.copy(), self.ota.copy());
        Model {
            agent: agent(&f, &ota),
            f,
            ota,
            seq: self.seq,
            next: self.next,
        }
    }

    /// STATUS as the node's store maps it from flash and otadata.
    fn status(&self) -> StatusResp {
        store(&self.f, &self.ota).status()
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> Status {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = vec![0u8; MAX + 16];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("reply");
        let (f, _) = Frame::decode(reply.frame, MAX).unwrap();
        split_status(f.payload).unwrap().0
    }

    fn transfer(&mut self, s: u8) -> Status {
        let body: Vec<u8> = (0..BODY).map(|i| (i as u8) ^ self.next).collect();
        let img = image(self.next, &body, &keypair());
        self.next += 1;
        let begin = XferBegin {
            target: target::APP,
            slot: s,
            total_len: img.len() as u32,
            hdr_len: IMAGE_HDR_LEN,
        };
        let st = self.call(Cmd::XferBegin, &wire(|b| begin.encode(b)));
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        let size = MAX - XferChunk::OVERHEAD;
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, data) in part.chunks(size).enumerate() {
                let c = XferChunk {
                    offset: (i * size) as u32,
                    data,
                };
                let st = self.call(cmd, &wire(|b| c.encode(b)));
                if st != Status::Ok {
                    return st;
                }
            }
        }
        self.call(Cmd::XferEnd, &[])
    }

    /// A reset: the 2nd stage chooses, what it starts runs its agent from
    /// the store read again.
    fn boot(&mut self) {
        self.ota.0.borrow_mut().boot();
        self.agent = agent(&self.f, &self.ota);
    }

    fn trying(&self) -> bool {
        let st = self.status();
        st.slots
            .get(usize::from(st.active))
            .is_some_and(|s| s.state == SlotState::Trying)
    }

    /// Runs `op`; `false` when the host's own checks keep it from
    /// happening. What the host sends is accepted.
    fn step(&mut self, op: Act) -> bool {
        let st = self.status();
        match op {
            Act::Transfer(s) => {
                if s == st.active || s == st.confirmed {
                    return false;
                }
                assert_eq!(self.transfer(s), Status::Ok, "transfer {s}");
            }
            Act::SetPending(s) => {
                // An app on trial takes no SET_PENDING (it confirms or is
                // rolled back first).
                if s == st.active
                    || st.slots[usize::from(s)].state == SlotState::Empty
                    || self.trying()
                {
                    return false;
                }
                let p = SetPending {
                    target: target::APP,
                    slot: s,
                };
                assert_eq!(
                    self.call(Cmd::SetPending, &wire(|b| p.encode(b))),
                    Status::Ok,
                    "set pending {s}"
                );
            }
            Act::Confirm => {
                if !self.trying() {
                    return false;
                }
                assert_eq!(
                    self.call(Cmd::Confirm, &wire(|b| encode_confirm(target::APP, b))),
                    Status::Ok
                );
            }
            Act::Boot => self.boot(),
            Act::Revert => {
                let pending = st.slots[..2]
                    .iter()
                    .any(|s| matches!(s.state, SlotState::Pending | SlotState::Trying));
                if !pending {
                    return false;
                }
                for _ in 0..=crate::TRIAL_ATTEMPTS + 1 {
                    self.boot();
                    if !self.trying() {
                        break;
                    }
                }
                assert!(!self.trying(), "a trial never rolled back");
            }
        }
        true
    }

    fn agrees(&self, path: &[Act]) {
        let st = self.status();
        for (s, info) in (0u8..).zip(&st.slots[..2]) {
            assert_eq!(
                self.agent.images().state(s),
                Some(as_machine(info.state)),
                "slot {s} after {path:?}: {st:?}"
            );
        }
        // A slot shown rolled back comes with why: the agent's event of
        // it at the next start carries this reason.
        if st.slots[..2].iter().any(|s| s.state == SlotState::Reverted) {
            assert_eq!(
                st.last_reason,
                dagon_format::statelog::reason::TRIAL_EXHAUSTED,
                "after {path:?}: {st:?}"
            );
        }
        if let Some(active) = st.slots.get(usize::from(st.active)) {
            assert_ne!(
                active.state,
                SlotState::Reverted,
                "active slot {} after {path:?}",
                st.active
            );
        }
    }
}

fn explore(model: &mut Model, path: &mut Vec<Act>, confirms: &mut usize) -> usize {
    model.agrees(path);
    let mut checked = 1;
    if path.len() == DEPTH {
        return checked;
    }
    for op in ACTS {
        let mut next = model.copy();
        if next.step(op) {
            *confirms += matches!(op, Act::Confirm) as usize;
            path.push(op);
            checked += explore(&mut next, path, confirms);
            path.pop();
        }
    }
    checked
}

#[test]
fn the_store_agrees_with_the_agents_machines() {
    let mut confirms = 0;
    let checked = explore(&mut Model::new(), &mut Vec::new(), &mut confirms);
    eprintln!("{checked} sequences, {confirms} confirms");
    assert!(checked > 700, "{checked} sequences");
    assert!(confirms > 0, "no CONFIRM explored");
}

/// The ways a rolled-back marker could outlive its image: a new image
/// written into the slot whose trial was aborted, from factory and with a
/// confirmed app in the other slot. Neither slot shows Reverted then.
#[test]
fn a_rolled_back_marker_names_one_image() {
    use Act::*;
    let cases: [&[Act]; 2] = [
        &[Transfer(0), SetPending(0), Revert, Transfer(0)],
        &[
            Transfer(0),
            SetPending(0),
            Boot,
            Confirm,
            Transfer(1),
            SetPending(1),
            Revert,
            Transfer(1),
        ],
    ];
    for path in cases {
        let mut m = Model::new();
        for (i, op) in path.iter().enumerate() {
            assert!(m.step(*op), "{op:?} refused after {:?}", &path[..i]);
            m.agrees(&path[..=i]);
        }
        let st = m.status();
        assert!(
            st.slots[..2].iter().all(|s| s.state != SlotState::Reverted),
            "{path:?}: {st:?}"
        );
    }
}

/// A selection the host replaced before any reset is never tried: not when
/// the one that replaced it is withdrawn by a new transfer, nor when it is
/// aborted. The 2nd stage starts factory.
#[test]
fn a_replaced_selection_is_never_tried() {
    use Act::*;
    let start = [Transfer(0), Transfer(1), SetPending(0), SetPending(1)];
    for end in [&[Transfer(1), Boot][..], &[Boot, Boot]] {
        let mut m = Model::new();
        for op in start.iter().chain(end) {
            assert!(m.step(*op), "{op:?} refused");
            assert_ne!(m.ota.running(), 0, "slot 0 started after {op:?}");
        }
        assert_eq!(m.ota.running(), SLOT_FACTORY);
        assert_eq!(m.status().slots[0].state, SlotState::Written);
    }
}
