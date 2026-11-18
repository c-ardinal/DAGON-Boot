//! An app agent with link diagnostics on fake ports: a store whose
//! commits can be cut before or after they land, seed sources over fake
//! drivers, and the project's test key.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dagon_agent::snapshot::AppSnapshot;
use dagon_agent::{writer, Agent, Config, SlotStore, StoreError, Transition};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::{mcu_id, port_kind, target_tag, HEADER_SIZE};
use dagon_format::{Version, DIAG_CLEAR_DOMAIN};
use dagon_port::diag::{
    CounterSeed, DiagStore, FactoryId, NoDiag, NoSeed, RandomBytes, RandomSeed, Sample, SeedSource,
    ANSWER_FRAME,
};
use dagon_proto::diag::*;
use dagon_proto::payload::{mode, role, SlotState, StatusResp};
use dagon_proto::table::link_diag::store;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed as KeySeed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

pub fn keypair() -> KeyPair {
    KeyPair::from_seed(KeySeed::new(TEST_SEED))
}

/// A key the node does not hold.
pub fn foreign() -> KeyPair {
    KeyPair::from_seed(KeySeed::new([7; 32]))
}

/// The signature `kp` makes of a clear of `scope` under `seed`.
pub fn sign(kp: &KeyPair, seed: &Seed, scope: &ClearScope) -> [u8; 64] {
    let msg = ClearMessage::new(seed, scope);
    *kp.sk
        .sign([DIAG_CLEAR_DOMAIN, msg.as_bytes()].concat(), None)
}

/// The app slots: A confirmed; nothing is written.
pub struct Slots;

impl SlotStore for Slots {
    fn status(&mut self) -> StatusResp {
        let mut st = StatusResp {
            active: 0,
            confirmed: 0,
            ..Default::default()
        };
        st.slots[0].state = SlotState::Confirmed;
        st.slots[0].version = Version::new(1, 0, 0);
        st
    }
    fn capacity(&self, _: u8, _: u8) -> u32 {
        0
    }
    fn erase(&mut self, _: u8, _: u8, _: u32, _: &Transition) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn write(&mut self, _: u8, _: u8, _: u32, _: &[u8], _: &Transition) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn read(&mut self, _: u8, _: u8, _: u32, _: &mut [u8]) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn write_header(
        &mut self,
        _: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn read_header(&mut self, _: u8, _: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn set_pending(
        &mut self,
        _: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
    fn confirm(&mut self, _: u8, _: &[u8; HEADER_SIZE], _: &Transition) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
}

pub type Chain = Composed<&'static Published<AppSnapshot, 8>, Nop, Nop, fn() -> u64>;

fn now() -> u64 {
    0
}

pub fn chain() -> Chain {
    let events: &'static mut Published<AppSnapshot, 8> = Box::leak(Box::new(Published::new()));
    compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now as fn() -> u64, Watchdog::new(4000, 8000)),
    )
}

pub const MCU: u16 = mcu_id::STM32F401XE;

pub fn config() -> Config {
    Config {
        mcu_id: MCU,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag("seg-f401-7seg"),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: 3,
    }
}

/// Where a write of the fake store is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// Nothing lands; the store answers `Status::Flash`.
    Before,
    /// The write lands, then the store answers `Status::Flash` (the node
    /// is cut before it answers).
    After,
}

/// What the fake store holds and what it was asked.
pub struct State {
    pub info: DiagInfo,
    pub entries: BTreeMap<Key, Entry>,
    pub peers: BTreeMap<Key, Peer>,
    pub counter: u32,
    pub cut_counter: Option<Cut>,
    pub cut_clear: Option<Cut>,
    pub clears: Vec<ClearScope>,
}

impl Default for State {
    fn default() -> Self {
        State {
            info: DiagInfo {
                availability: 0x7F,
                store: store::RETENTION_POWER_CUT,
                policy: dagon_proto::payload::PolicySet::EMPTY
                    .with(1)
                    .with(2)
                    .with(3),
                params: Params {
                    fdc_step: 32,
                    confirmation: 1,
                    aging: 40,
                },
            },
            entries: BTreeMap::new(),
            peers: BTreeMap::new(),
            counter: 0,
            cut_counter: None,
            cut_clear: None,
            clears: Vec::new(),
        }
    }
}

impl State {
    pub fn add_peer(&mut self, link: u8, addr: u8) {
        let p = Peer {
            link,
            addr,
            instance: NO_INSTANCE,
        };
        self.peers.insert(p.key(), p);
    }

    pub fn add_entry(&mut self, e: Entry) {
        self.entries.insert(e.key(), e);
    }

    fn apply(&mut self, scope: ClearScope) {
        let at = scope.endpoint();
        let hit = |e: Endpoint| at.is_none_or(|a| a == e);
        match scope {
            ClearScope::OneDtc { dtc, at } => self
                .entries
                .retain(|_, e| !(e.dtc == dtc && e.endpoint() == at)),
            ClearScope::EndpointDtcs(_) | ClearScope::AllDtcs => {
                self.entries.retain(|_, e| !hit(e.endpoint()))
            }
            ClearScope::EndpointAll(_) | ClearScope::All => {
                self.entries.retain(|_, e| !hit(e.endpoint()));
                self.peers.retain(|_, p| !hit(p.endpoint()));
            }
        }
    }
}

/// The fake store: a handle on [`State`] the test keeps too.
#[derive(Clone, Default)]
pub struct Fake(pub Rc<RefCell<State>>);

impl DiagStore for Fake {
    type Room = [u8; ANSWER_FRAME];
    const ROOM: Self::Room = [0; ANSWER_FRAME];
    fn info(&mut self) -> DiagInfo {
        self.0.borrow().info
    }
    fn entry_after(&mut self, after: Option<Key>) -> Result<Option<Entry>, Status> {
        let s = self.0.borrow();
        Ok(s.entries
            .values()
            .find(|e| after.is_none_or(|a| e.key() > a))
            .copied())
    }
    fn peer_after(&mut self, after: Option<Key>) -> Result<Option<Peer>, Status> {
        let s = self.0.borrow();
        Ok(s.peers
            .values()
            .find(|p| after.is_none_or(|a| p.key() > a))
            .copied())
    }
    fn clear(&mut self, scope: ClearScope) -> Result<(), Status> {
        let mut s = self.0.borrow_mut();
        s.clears.push(scope);
        match s.cut_clear {
            Some(Cut::Before) => Err(Status::Flash),
            Some(Cut::After) => {
                s.apply(scope);
                Err(Status::Flash)
            }
            None => {
                s.apply(scope);
                Ok(())
            }
        }
    }
    fn next_seed_counter(&mut self) -> Result<u32, Status> {
        let mut s = self.0.borrow_mut();
        if s.counter == u32::MAX {
            return Err(Status::Unsupported);
        }
        match s.cut_counter {
            Some(Cut::Before) => Err(Status::Flash),
            Some(Cut::After) => {
                s.counter += 1;
                Err(Status::Flash)
            }
            None => {
                s.counter += 1;
                Ok(s.counter)
            }
        }
    }
    fn record(&mut self, sample: Sample) -> Result<(), Status> {
        if sample.pass {
            self.0.borrow_mut().add_peer(sample.link, sample.addr);
        }
        Ok(())
    }
}

/// A factory ID of 4 bytes; `None`: unreadable.
#[derive(Clone)]
pub struct Id(pub Rc<RefCell<Option<[u8; 4]>>>);

impl FactoryId for Id {
    const LEN: usize = 4;
    fn read(&mut self, out: &mut [u8; 16]) -> Result<(), Status> {
        let id = self.0.borrow().ok_or(Status::Unsupported)?;
        out[..4].copy_from_slice(&id);
        Ok(())
    }
}

/// Random bytes from a list the test gives; an empty list: the driver
/// fails.
#[derive(Clone, Default)]
pub struct Random(pub Rc<RefCell<Vec<[u8; 16]>>>);

impl RandomBytes for Random {
    fn fill(&mut self, out: &mut [u8; 16]) -> Result<(), Status> {
        let mut q = self.0.borrow_mut();
        if q.is_empty() {
            return Err(Status::Unsupported);
        }
        *out = q.remove(0);
        Ok(())
    }
}

pub type Counter = CounterSeed<Id>;

/// An app agent with link diagnostics over [`Fake`] and seeds from `Q`.
pub struct Node<Q: SeedSource, D: DiagStore = Fake> {
    pub agent: Agent<Slots, Chain, Ed25519Compact, writer::App, 1, Q, D>,
    seq: u8,
}

pub fn verifier() -> Ed25519Compact {
    Ed25519Compact::new(*keypair().pk)
}

impl<Q: SeedSource, D: DiagStore> Node<Q, D> {
    pub fn new(seeds: Q, store: D) -> Self {
        let agent = Agent::new(config(), verifier(), Slots, chain()).with_diag(seeds, store);
        Node { agent, seq: 0 }
    }

    /// One request with the next seq: its status and body.
    pub fn call(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        self.seq = self.seq.wrapping_add(1);
        self.send(self.seq, 0, cmd, payload)
    }

    /// One request frame as given.
    pub fn send(&mut self, seq: u8, fl: u8, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        let mut buf = vec![0u8; dagon_proto::frame_len(payload.len())];
        let n = Frame {
            flags: fl,
            seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("an answer");
        let (f, _) = Frame::decode(reply.frame, max_payload::USB).unwrap();
        assert_eq!(
            (f.flags & flags::RESP, f.seq, f.cmd),
            (flags::RESP, seq, cmd as u8)
        );
        let (st, body) = split_status(f.payload).unwrap();
        (st, body.to_vec())
    }

    /// DIAG_SEED: the seed, or the refusal.
    pub fn seed(&mut self) -> Result<Seed, Status> {
        let ask = enc(|b| DiagSeedReq { level: 1 }.encode(b));
        match self.call(Cmd::DiagSeed, &ask) {
            (Status::Ok, body) => Ok(Seed::parse(&body).unwrap()),
            (st, _) => Err(st),
        }
    }

    /// LINK_DIAG_CLEAR of `scope` with `key`.
    pub fn clear(&mut self, scope: ClearScope, key: Option<[u8; 64]>) -> Status {
        self.call(Cmd::LinkDiagClear, &clear_req(scope, key)).0
    }
}

pub fn clear_req(scope: ClearScope, key: Option<[u8; 64]>) -> Vec<u8> {
    enc(|b| LinkDiagClear { scope, key }.encode(b))
}

pub fn enc(f: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; max_payload::USB];
    let n = f(&mut b).unwrap();
    b.truncate(n);
    b
}

/// A counter-seed node over a fresh fake store, with factory ID `id`.
pub fn counter_node(id: [u8; 4]) -> (Node<Counter>, Fake) {
    let fake = Fake::default();
    let seeds = CounterSeed {
        mcu_id: MCU,
        id: Id(Rc::new(RefCell::new(Some(id)))),
    };
    (Node::new(seeds, fake.clone()), fake)
}

/// A node with the default ports.
pub type Plain = Agent<Slots, Chain, Ed25519Compact>;

pub fn plain() -> Plain {
    Agent::new(config(), verifier(), Slots, chain())
}

pub const EP: Endpoint = Endpoint {
    link: 1,
    addr: 0x42,
};

/// A random-seed node over a fresh fake store, its driver giving `seeds`.
pub fn random_node(seeds: Vec<[u8; 16]>) -> (Node<RandomSeed<Random>>, Fake) {
    let fake = Fake::default();
    let random = RandomSeed(Random(Rc::new(RefCell::new(seeds))));
    (Node::new(random, fake.clone()), fake)
}

/// A node that keeps link diagnostics and issues no seeds.
pub fn seedless() -> (Node<NoSeed>, Fake) {
    let fake = Fake::default();
    (Node::new(NoSeed, fake.clone()), fake)
}

/// A node whose store is [`NoDiag`] behind a real seed source.
pub fn storeless() -> Node<RandomSeed<Random>, NoDiag> {
    Node::new(
        RandomSeed(Random(Rc::new(RefCell::new(vec![[1; 16]])))),
        NoDiag,
    )
}
