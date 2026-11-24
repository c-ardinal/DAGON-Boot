//! The agent's image machines over the F401's own store (`KindSStore`)
//! agree with the STATUS that store reports from flash, for every sequence
//! of what the host writes (a transfer into either slot, SET_PENDING of
//! either slot, CONFIRM) and what the boot core writes (its app decision,
//! `boot::decide`: a start, or a trial never confirmed and rolled back),
//! from the first install. `dagon-agent`'s own fold test checks the
//! machines against the log's fold over a model store and
//! `dagon-sim`'s against the simulator; this one checks the board's store,
//! its headers read from flash and its log loaded from flash.
//!
//! A rolled-back marker that outlives its image (a new image written into
//! the rolled-back slot, a confirmed image demoted after its re-trial was
//! rolled back) fails here: STATUS pairs the marker with the header digest.

mod cut;

use cut::*;
use dagon_agent::image::State as Image;
use dagon_agent::{Agent, Config, SlotStore};
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::slot;
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::{KindSStore, ERASE_REGIONS, TRIAL_ATTEMPTS};
use dagon_proto::payload::*;
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

/// Operations per sequence; every sequence up to this length is checked.
const DEPTH: usize = 6;
/// Body bytes of the images the host writes: two chunks.
const BODY: usize = 400;
const CHUNK: usize = 250;
const TAG: &str = "node-f401";

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type NodeAgent = Agent<KindSStore<Cut>, Chain, Ed25519Compact>;

fn chain() -> Chain {
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        // Leaked: a few bytes per agent the search starts.
        Box::leak(Box::new(Published::new())),
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 8000)),
    )
}

fn verifier() -> Ed25519Compact {
    Ed25519Compact::new(*KeyPair::from_seed(Seed::new(SEED)).pk)
}

fn config() -> Config {
    Config {
        mcu_id: MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag(TAG),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    }
}

#[derive(Clone, Copy, Debug)]
enum Op {
    /// The host writes a new image into the slot (XFER_BEGIN..XFER_END).
    Transfer(u8),
    /// SET_PENDING of the slot.
    SetPending(u8),
    /// CONFIRM of the app on trial.
    Confirm,
    /// A reset: the boot core starts the pending image or the confirmed.
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

fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 300];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// One node: its flash, the slot its app runs from, and the agent of that
/// app (kept across the host's requests, started again at each boot).
struct Model {
    f: Cut,
    running: u8,
    agent: NodeAgent,
    seq: u8,
    /// Minor version of the next image written (each image distinct).
    next: u8,
}

impl Model {
    fn new() -> Self {
        let f = installed();
        let mut m = Model {
            agent: agent(&f, 0),
            f,
            running: 0,
            seq: 0,
            next: 1,
        };
        m.boot();
        assert_eq!(m.running, 0);
        m
    }

    /// The same node on a flash of its own; its agent starts from it.
    fn copy(&self) -> Self {
        let f = self.f.copy();
        Model {
            agent: agent(&f, self.running),
            f,
            running: self.running,
            seq: self.seq,
            next: self.next,
        }
    }

    /// STATUS as the node's store reports it from flash.
    fn status(&self) -> StatusResp {
        store(&self.f, self.running).status()
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
        let (f, _) = Frame::decode(reply.frame, 300).unwrap();
        split_status(f.payload).unwrap().0
    }

    fn transfer(&mut self, s: u8) -> Status {
        let img = signed(
            image_kind::APP,
            s,
            SLOTS.slots[usize::from(s)].header,
            self.next,
            BODY,
        );
        self.next += 1;
        let begin = XferBegin {
            target: target::APP,
            slot: s,
            total_len: img.len() as u32,
            hdr_len: HEADER_SIZE as u16,
        };
        let st = self.call(Cmd::XferBegin, &wire(|b| begin.encode(b)));
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, data) in part.chunks(CHUNK).enumerate() {
                let c = XferChunk {
                    offset: (i * CHUNK) as u32,
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

    /// A reset and the boot core's app decision; the app it starts runs
    /// its agent from the store loaded again.
    fn boot(&mut self) {
        let cfg = BootConfig {
            mcu_id: MCU_ID,
            target_tag: target_tag(TAG),
            max_attempts: TRIAL_ATTEMPTS,
        };
        let mut log = Log::load(&mut self.f, &LOG).unwrap();
        self.running = match decide(&mut self.f, &mut log, &SLOTS, &cfg, &verifier()) {
            Decision::Boot { slot, .. } => slot,
            Decision::NoImage => panic!("nothing to start"),
        };
        self.agent = agent(&self.f, self.running);
    }

    fn trying(&self) -> bool {
        let st = self.status();
        st.slots
            .get(usize::from(st.active))
            .is_some_and(|s| s.state == SlotState::Trying)
    }

    /// Runs `op`; `false` when the host's own checks keep it from
    /// happening. What the host sends is accepted.
    fn step(&mut self, op: Op) -> bool {
        let st = self.status();
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
            Op::Confirm => {
                if !self.trying() {
                    return false;
                }
                assert_eq!(
                    self.call(Cmd::Confirm, &wire(|b| encode_confirm(target::APP, b))),
                    Status::Ok
                );
            }
            Op::Boot => self.boot(),
            Op::Revert => {
                let pending = st.slots[..2]
                    .iter()
                    .any(|s| matches!(s.state, SlotState::Pending | SlotState::Trying));
                if !pending {
                    return false;
                }
                for _ in 0..=TRIAL_ATTEMPTS + 1 {
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

    fn agrees(&self, path: &[Op]) {
        let st = self.status();
        for (s, info) in (0u8..).zip(&st.slots[..2]) {
            assert_eq!(
                self.agent.images().state(s),
                Some(as_machine(info.state)),
                "slot {s} after {path:?}: {st:?}"
            );
        }
        assert_ne!(
            st.slots[usize::from(st.active)].state,
            SlotState::Reverted,
            "active slot {} after {path:?}",
            st.active
        );
    }
}

fn store(f: &Cut, running: u8) -> KindSStore<Cut> {
    KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS, running).unwrap()
}

fn agent(f: &Cut, running: u8) -> NodeAgent {
    assert_ne!(running, slot::NONE);
    Agent::new(config(), verifier(), store(f, running), chain())
}

fn explore(model: &mut Model, path: &mut Vec<Op>, confirms: &mut usize) -> usize {
    model.agrees(path);
    let mut checked = 1;
    if path.len() == DEPTH {
        return checked;
    }
    for op in OPS {
        let mut next = model.copy();
        if next.step(op) {
            *confirms += matches!(op, Op::Confirm) as usize;
            path.push(op);
            checked += explore(&mut next, path, confirms);
            path.pop();
        }
    }
    checked
}

#[test]
fn the_boards_store_agrees_with_the_agents_machines() {
    let mut confirms = 0;
    let checked = explore(&mut Model::new(), &mut Vec::new(), &mut confirms);
    eprintln!("{checked} sequences, {confirms} confirms");
    assert!(checked > 700, "{checked} sequences");
    assert!(confirms > 0, "no CONFIRM explored");
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
        let st = m.status();
        assert!(
            st.slots[..2].iter().all(|s| s.state != SlotState::Reverted),
            "{path:?}: {st:?}"
        );
    }
}
