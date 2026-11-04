//! The agent's image machines over a SimStore agree with the STATUS the
//! store reports, for every sequence of what the agent writes (transfers,
//! switches, confirms) and what the simulated boot core writes (a start, a
//! trial that never confirms and is rolled back), from a provisioned node.
//! The store folds its log with dagon-core, as every kind S store does, so
//! this checks the simulator against the same contract as the boards.

use dagon_agent::image::State as Image;
use dagon_agent::{Agent, Config, SlotStore};
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::store::Boot;
use dagon_sim::*;
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
/// Operations per sequence; every sequence up to this length is checked.
const DEPTH: usize = 7;

fn kp() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn cfg() -> Config {
    f401_config(*kp().pk)
}

/// A signed image for `slot` whose version (and so header) is `minor`.
fn image(slot: u8, minor: u16) -> Vec<u8> {
    let kp = kp();
    let body = [minor as u8; 64];
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: Layout::STM32F401XE.slots[slot as usize].0 + 0x200,
        payload_size: body.len() as u32,
        version: Version::new(1, 0, minor),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: cfg().target_tag,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], &body].concat()
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

fn chain() -> Chain {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 8000)),
    )
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Transfer(u8),
    SetPending(u8),
    Confirm,
    /// A reset: the boot core starts the pending image (or the confirmed).
    Boot,
    /// The pending image never confirms: resets until it is rolled back.
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

struct Model {
    /// The board's handle (the boot core); the agent holds another.
    store: SimStore,
    agent: Agent<SimStore, Chain>,
    seq: u8,
    next: u16,
}

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

impl Model {
    fn new() -> Self {
        let mut store = SimStore::new(SimFlash::stm32f401xe(1), Layout::STM32F401XE, *kp().pk);
        store.provision(0, &image(0, 0)).unwrap();
        assert_eq!(store.boot(), Boot::Run(0));
        Model {
            agent: Agent::new(cfg(), store.clone(), chain()),
            store,
            seq: 0,
            next: 1,
        }
    }

    /// The same device in an independent copy; its agent starts from it.
    fn copy(&self) -> Self {
        let store = self.store.snapshot();
        Model {
            agent: Agent::new(cfg(), store.clone(), chain()),
            store,
            seq: self.seq,
            next: self.next,
        }
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> Status {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = vec![0u8; 300];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("reply");
        let (f, _) = Frame::decode(reply.frame, max_payload::USB).unwrap();
        assert_eq!(f.flags, flags::RESP);
        split_status(f.payload).unwrap().0
    }

    fn transfer(&mut self, slot: u8) -> Status {
        let img = image(slot, self.next);
        self.next += 1;
        let mut p = [0u8; 300];
        let n = XferBegin {
            target: target::APP,
            slot,
            total_len: img.len() as u32,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .unwrap();
        let st = self.call(Cmd::XferBegin, &p[..n]);
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, c) in part.chunks(200).enumerate() {
                let n = XferChunk {
                    offset: (i * 200) as u32,
                    data: c,
                }
                .encode(&mut p)
                .unwrap();
                let st = self.call(cmd, &p[..n]);
                if st != Status::Ok {
                    return st;
                }
            }
        }
        self.call(Cmd::XferEnd, &[])
    }

    /// Runs `op`; `false` when the agent's guards keep it from happening.
    fn step(&mut self, op: Op) -> bool {
        let st = self.store.status();
        let trying = |st: &StatusResp| {
            st.slots
                .get(usize::from(st.active))
                .is_some_and(|s| s.state == SlotState::Trying)
        };
        match op {
            Op::Transfer(s) => {
                if s == st.active || s == st.confirmed {
                    return false;
                }
                assert_eq!(self.transfer(s), Status::Ok, "transfer {s}");
            }
            Op::SetPending(s) => {
                if s == st.active || st.slots[usize::from(s)].state == SlotState::Empty {
                    return false;
                }
                let st = self.call(Cmd::SetPending, &[target::APP, s]);
                assert_eq!(st, Status::Ok, "set pending {s}");
            }
            Op::Confirm => {
                if !trying(&st) {
                    return false;
                }
                assert_eq!(self.call(Cmd::Confirm, &[target::APP]), Status::Ok);
            }
            Op::Boot => {
                self.store.boot();
                self.agent.restart();
            }
            Op::Revert => {
                let pending = st.slots[..2]
                    .iter()
                    .any(|s| matches!(s.state, SlotState::Pending | SlotState::Trying));
                if !pending {
                    return false;
                }
                for _ in 0..=self.store.layout.trial_attempts() + 1 {
                    self.store.boot();
                    if !trying(&self.store.status()) {
                        break;
                    }
                }
                self.agent.restart();
            }
        }
        true
    }

    fn agrees(&mut self, path: &[Op]) {
        let st = self.store.status();
        for (s, info) in (0u8..).zip(&st.slots[..2]) {
            assert_eq!(
                self.agent.images().state(s),
                Some(as_machine(info.state)),
                "slot {s} after {path:?}: {st:?}"
            );
        }
        // The running slot is never one rolled back, and a node with a
        // confirmed image always has something to start.
        if let Some(a) = st
            .slots
            .get(usize::from(st.active))
            .filter(|_| st.active < 2)
        {
            assert_ne!(
                a.state,
                SlotState::Reverted,
                "active slot {} after {path:?}",
                st.active
            );
        }
        if st.confirmed < 2 {
            assert_ne!(
                self.store.snapshot().boot(),
                Boot::Nothing,
                "after {path:?}"
            );
        }
    }
}

fn explore(model: &mut Model, path: &mut Vec<Op>) -> usize {
    model.agrees(path);
    let mut checked = 1;
    if path.len() == DEPTH {
        return checked;
    }
    for op in OPS {
        let mut next = model.copy();
        if next.step(op) {
            path.push(op);
            checked += explore(&mut next, path);
            path.pop();
        }
    }
    checked
}

#[test]
fn the_simulators_store_agrees_with_the_agents_machines() {
    let checked = explore(&mut Model::new(), &mut Vec::new());
    assert!(checked > 1_500, "{checked} sequences");
}

/// The case the shared contract fixes: a confirmed image re-tried while
/// another trial ran starts its own trial with no attempts carried over,
/// and a rollback returns to the confirmed slot.
#[test]
fn a_retried_confirmed_image_starts_a_fresh_trial() {
    let mut m = Model::new();
    for op in [
        Op::Transfer(1),
        Op::SetPending(1),
        Op::Boot,
        Op::Boot,
        Op::SetPending(0),
        Op::Boot,
    ] {
        assert!(m.step(op), "{op:?}");
    }
    let st = m.store.status();
    assert_eq!((st.active, st.attempts), (0, 1));
    assert_eq!(st.slots[0].state, SlotState::Trying);
    assert!(m.step(Op::Revert));
    let st = m.store.status();
    assert_eq!((st.active, st.confirmed), (0, 0));
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
}
