//! The agent over the DAGON wire against an in-memory slot store: what it
//! answers, what it writes, and the events and decisions on the way.

use std::cell::RefCell;
use std::rc::Rc;

use dagon_agent::snapshot::{AppSnapshot, Phase};
use dagon_agent::{image, requests, session, Agent, Config, SlotStore, StoreError, Transition};
use dagon_events::{
    compose, Accept, Bounds, Composed, Ctx, Delay, Event, Extension, Health, Layer, Nop, Published,
    Reader, RebootRequest, TrialInfo, UpdateOffer, Watchdog,
};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const TAG: [u8; 16] = target_tag("seg-f401-7seg");
const CAP: usize = 4096;
/// Watchdog periods of the test layer: a reboot delay is capped at 2000 ms.
const WATCHDOG: Watchdog = Watchdog::new(4000, 8000);

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn image(slot: u8, body: &[u8]) -> Vec<u8> {
    let kp = keypair();
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: 0x0802_0200,
        payload_size: body.len() as u32,
        version: Version::new(1, 1, 0),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
}

struct Mem {
    status: StatusResp,
    bodies: [Vec<u8>; 2],
    headers: [[u8; HEADER_SIZE]; 2],
    erases: usize,
    writes: usize,
    pending: Option<u8>,
    confirms: usize,
    /// Header bytes flipped on read-back (a flash fault).
    bad_readback: bool,
}

impl Mem {
    /// Running confirmed from A; B empty.
    fn new() -> Self {
        let mut status = StatusResp {
            active: 0,
            confirmed: 0,
            ..Default::default()
        };
        status.slots[0].state = SlotState::Confirmed;
        status.slots[0].version = Version::new(1, 0, 0);
        status.slots[2].state = SlotState::Empty;
        Mem {
            status,
            bodies: [vec![0xFF; CAP], vec![0xFF; CAP]],
            headers: [[0xFF; HEADER_SIZE]; 2],
            erases: 0,
            writes: 0,
            pending: None,
            confirms: 0,
            bad_readback: false,
        }
    }
}

impl SlotStore for Mem {
    fn status(&mut self) -> StatusResp {
        self.status
    }
    fn capacity(&self, target: u8, _slot: u8) -> u32 {
        if target == target::APP {
            CAP as u32
        } else {
            0
        }
    }
    fn erase(&mut self, _t: u8, slot: u8, len: u32, _: &Transition) -> Result<(), StoreError> {
        self.erases += 1;
        self.bodies[slot as usize][..len as usize].fill(0xFF);
        self.headers[slot as usize] = [0xFF; HEADER_SIZE];
        self.status.slots[slot as usize].state = SlotState::Empty;
        Ok(())
    }
    fn write(
        &mut self,
        _t: u8,
        slot: u8,
        off: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.writes += 1;
        self.bodies[slot as usize][off as usize..][..data.len()].copy_from_slice(data);
        Ok(())
    }
    fn read(&mut self, _t: u8, slot: u8, off: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let src = self.bodies[slot as usize].get(off as usize..off as usize + buf.len());
        buf.copy_from_slice(src.ok_or(StoreError::Range)?);
        Ok(())
    }
    fn write_header(
        &mut self,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.headers[slot as usize] = *h;
        let info = &mut self.status.slots[slot as usize];
        info.state = SlotState::Written;
        info.version = ImageHeader::parse(h).map_or(Version::default(), |h| h.version);
        Ok(())
    }
    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        *out = self.headers[slot as usize];
        if self.bad_readback {
            out[100] ^= 1;
        }
        Ok(())
    }
    fn set_pending(
        &mut self,
        slot: u8,
        _h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        // The log names one pending slot: the one before is an image again,
        // or the confirmed one again.
        let st = &mut self.status;
        for (i, info) in st.slots[..2].iter_mut().enumerate() {
            if i != slot as usize && matches!(info.state, SlotState::Pending | SlotState::Trying) {
                info.state = if i == st.confirmed as usize {
                    SlotState::Confirmed
                } else {
                    SlotState::Written
                };
                if i == st.active as usize {
                    st.active = st.confirmed;
                }
            }
        }
        self.pending = Some(slot);
        st.slots[slot as usize].state = SlotState::Pending;
        Ok(())
    }
    fn confirm(
        &mut self,
        slot: u8,
        _h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.confirms += 1;
        let old = self.status.confirmed;
        if old != slot && (old as usize) < 2 {
            self.status.slots[old as usize].state = SlotState::Written;
        }
        self.status.slots[slot as usize].state = SlotState::Confirmed;
        self.status.confirmed = slot;
        Ok(())
    }
}

/// What the test extension saw: every event kind (`replay:` when
/// replayed), and the decision arguments.
#[derive(Default)]
struct Seen {
    events: Vec<String>,
    offers: Vec<UpdateOffer>,
    trials: Vec<TrialInfo>,
}

/// A user extension recording what it sees and answering as told.
#[derive(Default, Clone)]
struct Probe {
    seen: Rc<RefCell<Seen>>,
    refuse_update: bool,
    health: Option<Health>,
    delay: Option<Delay>,
}

impl Extension for Probe {
    fn id(&self) -> &'static str {
        "probe"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        let k = if e.is_replayed() {
            format!("replay:{}", e.kind())
        } else {
            e.kind().to_string()
        };
        self.seen.borrow_mut().events.push(k);
    }
    fn accept_update(&mut self, o: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
        self.seen.borrow_mut().offers.push(*o);
        self.refuse_update.then_some(Accept::No)
    }
    fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
        self.delay
    }
    fn self_check(&mut self, t: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        self.seen.borrow_mut().trials.push(*t);
        self.health
    }
}

type Chain = Composed<&'static Published<AppSnapshot, 32>, Nop, Probe, fn() -> u64>;

fn now() -> u64 {
    0
}

fn chain(probe: Probe) -> Chain {
    let events: &'static mut Published<AppSnapshot, 32> = Box::leak(Box::new(Published::new()));
    compose(
        Layer::App,
        events,
        Nop,
        probe,
        Bounds::new(now as fn() -> u64, WATCHDOG),
    )
}

fn config() -> Config {
    Config {
        pubkey: *keypair().pk,
        mcu_id: mcu_id::STM32F401XE,
        port_kind: port_kind::SELF_BOOT,
        target_tag: TAG,
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: 3,
    }
}

struct Node {
    /// The slot store, shared with the bench (it plays the boot core).
    mem: &'static RefCell<Mem>,
    agent: Agent<&'static RefCell<Mem>, Chain>,
    /// The agent's published snapshot.
    events: Reader<AppSnapshot, 32>,
    probe: Probe,
    seq: u8,
}

impl Node {
    fn new(probe: Probe) -> Self {
        Self::with(Mem::new(), probe)
    }

    fn with(mem: Mem, probe: Probe) -> Self {
        let mem: &'static RefCell<Mem> = Box::leak(Box::new(RefCell::new(mem)));
        let c = chain(probe.clone());
        let events = c.reader();
        Node {
            mem,
            agent: Agent::new(config(), mem, c),
            events,
            probe,
            seq: 0,
        }
    }

    /// A reset: the store as `boot` leaves it, a fresh agent over it.
    fn reset(&mut self, boot: impl FnOnce(&mut Mem)) {
        boot(&mut self.mem.borrow_mut());
        self.probe.seen.borrow_mut().events.clear();
        let c = chain(self.probe.clone());
        self.events = c.reader();
        self.agent = Agent::new(config(), self.mem, c);
    }

    /// The published snapshot's phase.
    fn phase(&self) -> Phase {
        self.events.latest().expect("published").state.phase
    }

    fn events(&self) -> Vec<String> {
        std::mem::take(&mut self.probe.seen.borrow_mut().events)
    }

    /// Sends one request with a fresh seq; returns status, body, reboot flag.
    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>, bool) {
        self.seq = self.seq.wrapping_add(1);
        self.call_seq(self.seq, cmd, payload)
    }

    fn call_seq(&mut self, seq: u8, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>, bool) {
        let mut buf = vec![0u8; 300];
        let n = Frame {
            flags: 0,
            seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("reply");
        let (f, _) = Frame::decode(reply.frame, 300).unwrap();
        assert_eq!((f.seq, f.cmd, f.flags), (seq, cmd as u8, flags::RESP));
        let (st, body) = split_status(f.payload).unwrap();
        (st, body.to_vec(), reply.reboot)
    }

    fn enc<T>(
        v: T,
        f: impl Fn(&T, &mut [u8]) -> Result<usize, dagon_proto::FrameError>,
    ) -> Vec<u8> {
        let mut b = vec![0u8; 300];
        let n = f(&v, &mut b).unwrap();
        b.truncate(n);
        b
    }

    /// Full XFER of `img` to `slot`; returns the first non-OK status (or OK).
    fn transfer(&mut self, slot: u8, img: &[u8]) -> Status {
        let begin = XferBegin {
            target: target::APP,
            slot,
            total_len: img.len() as u32,
            hdr_len: 512,
        };
        let st = self
            .call(Cmd::XferBegin, &Self::enc(begin, XferBegin::encode))
            .0;
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, chunk) in part.chunks(252).enumerate() {
                let c = XferChunk {
                    offset: (i * 252) as u32,
                    data: chunk,
                };
                let (st, b, _) = self.call(cmd, &Self::enc(c, XferChunk::encode));
                if st != Status::Ok {
                    return st;
                }
                assert_eq!(
                    parse_next_offset(&b).unwrap() as usize,
                    i * 252 + chunk.len()
                );
            }
        }
        let (st, digest, _) = self.call(Cmd::XferEnd, &[]);
        if st == Status::Ok {
            assert_eq!(digest, sha256(body));
        }
        st
    }

    fn store(&mut self) -> std::cell::RefMut<'static, Mem> {
        self.mem.borrow_mut()
    }
}

fn body() -> Vec<u8> {
    (0..1000u32).map(|i| (i * 13) as u8).collect()
}

/// After the boot core starts B on trial.
fn b_on_trial(m: &mut Mem) {
    m.status.active = 1;
    m.status.attempts = 1;
    m.status.slots[1] = SlotInfo {
        state: SlotState::Trying,
        version: Version::new(1, 1, 0),
        digest16: [0; 16],
    };
}

fn dedup(events: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in events {
        if out.last() != Some(&e) {
            out.push(e);
        }
    }
    out
}

#[test]
fn update_set_pending_reboot_confirm() {
    let mut n = Node::new(Probe::default());
    assert_eq!(n.events(), ["starting", "store_opened"]);
    let img = image(1, &body());
    assert_eq!(n.transfer(1, &img), Status::Ok);
    assert_eq!(
        n.store().headers[1][..],
        img[..HEADER_SIZE],
        "header written last"
    );
    assert_eq!(
        dedup(n.events()),
        [
            "update_offered",
            "header_received",
            "header_verified",
            "transfer_started",
            "body_opened",
            "transfer_progress",
            "verify_started",
            "verify_ok",
            "written",
        ]
    );
    assert_eq!(n.agent.images().state(1), Some(image::State::Written));

    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::Ok);
    assert_eq!(n.store().pending, Some(1));
    let (st, _, reboot) = n.call(Cmd::Reboot, &[reboot_mode::NORMAL]);
    assert_eq!((st, reboot), (Status::Ok, true));
    assert_eq!(n.events(), ["pending_set", "reboot_requested"]);

    n.reset(b_on_trial);
    assert_eq!(
        n.events(),
        ["starting", "store_opened", "replay:trial_started"]
    );
    assert!(n.agent.images().on_trial());
    let hello = Node::enc(
        HelloReq {
            proto_ver: 1,
            max_payload: 1088,
        },
        HelloReq::encode,
    );
    let (st, body, _) = n.call(Cmd::Hello, &hello);
    let h = HelloResp::parse(&body).unwrap();
    assert_eq!(
        (st, h.active_version, h.trial),
        (Status::Ok, Version::new(1, 1, 0), true)
    );

    assert_eq!(n.call(Cmd::Confirm, &[target::APP]).0, Status::Ok);
    assert_eq!(
        n.call(Cmd::Confirm, &[target::APP]).0,
        Status::Ok,
        "repeat is OK"
    );
    assert_eq!(n.store().confirms, 1, "repeat writes nothing");
    assert_eq!(
        n.events(),
        ["hello", "self_check", "confirmed", "demoted"],
        "the old confirmed slot is demoted"
    );
    let t = n.probe.seen.borrow().trials[0];
    assert_eq!(
        (t.slot, t.version, t.attempt, t.max),
        (1, Version::new(1, 1, 0), 1, config().trial_attempts),
        "the boot core's attempt limit comes from the configuration"
    );
    let (_, body, _) = n.call(Cmd::Status, &[]);
    assert_eq!(StatusResp::parse(&body).unwrap().confirmed, 1);
    assert_eq!(n.agent.images().state(0), Some(image::State::Written));
    assert!(!n.agent.images().on_trial());
}

#[test]
fn a_rollback_the_boot_core_made_is_replayed() {
    let mut n = Node::new(Probe::default());
    n.reset(|m| {
        m.status.slots[1].state = SlotState::Reverted;
        m.status.last_reason = 3;
    });
    assert_eq!(
        n.events(),
        ["starting", "store_opened", "replay:rolled_back"]
    );
    assert_eq!(n.agent.images().state(1), Some(image::State::Reverted));
}

#[test]
fn tampered_header_is_rejected_without_erase() {
    let mut n = Node::new(Probe::default());
    let mut img = image(1, &body());
    img[0x018] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Signature);
    assert_eq!(n.store().erases, 0);
    let ev = dedup(n.events());
    assert_eq!(
        ev[ev.len() - 2..],
        ["transfer_aborted", "header_rejected"],
        "{ev:?}"
    );
}

#[test]
fn tampered_body_is_rejected_without_header() {
    let mut n = Node::new(Probe::default());
    let mut img = image(1, &body());
    img[HEADER_SIZE + 10] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Digest);
    assert_eq!(
        n.store().headers[1],
        [0xFF; HEADER_SIZE],
        "slot stays imageless"
    );
    assert_eq!(n.agent.images().state(1), Some(image::State::Empty));
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::State);
    let ev = n.events();
    assert!(
        ev.ends_with(&["verify_failed".into(), "pending_refused".into()]),
        "{ev:?}"
    );
}

#[test]
fn wrong_board_or_key_is_rejected() {
    let mut n = Node::new(Probe::default());
    n.agent = Agent::new(
        Config {
            target_tag: target_tag("other"),
            ..config()
        },
        n.mem,
        chain(Probe::default()),
    );
    assert_eq!(n.transfer(1, &image(1, &body())), Status::Compat);
    assert_eq!(
        n.transfer(1, &image(0, &body())),
        Status::Compat,
        "slot binding A into B"
    );
    let mut img = image(1, &body());
    img[0x048] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Key);
    assert_eq!(n.store().erases, 0);
}

#[test]
fn running_and_confirmed_slots_are_protected() {
    let mut n = Node::new(Probe::default());
    assert_eq!(n.transfer(0, &image(0, &body())), Status::State);
    n.store().status.active = 1;
    assert_eq!(
        n.transfer(0, &image(0, &body())),
        Status::State,
        "confirmed slot"
    );
    assert_eq!(
        n.call(Cmd::SetPending, &[target::APP, 1]).0,
        Status::State,
        "running slot"
    );
    assert!(n.probe.seen.borrow().offers.is_empty(), "never offered");
    let ev = n.events();
    assert_eq!(
        ev,
        [
            "starting",
            "store_opened",
            "update_refused",
            "update_refused",
            "pending_refused"
        ]
    );
}

#[test]
fn extensions_answer_the_decision_points() {
    let mut n = Node::new(Probe {
        refuse_update: true,
        ..Default::default()
    });
    assert_eq!(n.transfer(1, &image(1, &body())), Status::State);
    assert_eq!(n.store().erases, 0);
    let o = n.probe.seen.borrow().offers[0];
    assert_eq!((o.target, o.slot, o.body_len), (target::APP, 1, 1000));
    assert_eq!(
        n.events(),
        [
            "starting",
            "store_opened",
            "update_offered",
            "transfer_aborted",
            "update_refused"
        ]
    );

    for answer in [Health::Unhealthy, Health::Pending] {
        let mut n = Node::with(
            {
                let mut m = Mem::new();
                b_on_trial(&mut m);
                m
            },
            Probe {
                health: Some(answer),
                ..Default::default()
            },
        );
        assert_eq!(n.call(Cmd::Confirm, &[target::APP]).0, Status::State);
        assert_eq!(n.store().confirms, 0);
        assert!(n.agent.images().on_trial());
    }

    let mut n = Node::new(Probe {
        delay: Some(Delay::After(60_000)),
        ..Default::default()
    });
    let reply = {
        let mut buf = [0u8; 16];
        let k = Frame {
            flags: 0,
            seq: 9,
            cmd: Cmd::Reboot as u8,
            payload: &[reboot_mode::NORMAL],
        }
        .encode(&mut buf)
        .unwrap();
        let r = n.agent.handle(&buf[..k]).unwrap();
        (r.reboot, r.reboot_delay_ms)
    };
    assert_eq!(reply, (true, WATCHDOG.reboot_delay_max_ms()), "capped");
}

#[test]
fn an_app_confirms_its_own_trial() {
    let mut on_trial = Mem::new();
    b_on_trial(&mut on_trial);
    let mut n = Node::with(on_trial, Probe::default());
    n.events();
    assert_eq!(n.agent.confirm_own(), Ok(()));
    assert_eq!(n.store().confirms, 1);
    assert_eq!(n.events(), ["self_check", "confirmed", "demoted"]);
    assert_eq!(n.agent.confirm_own(), Err(Status::State), "not on trial");
    assert_eq!(n.store().confirms, 1);
    assert_eq!(n.events(), ["confirm_refused"]);

    let mut on_trial = Mem::new();
    b_on_trial(&mut on_trial);
    let mut n = Node::with(
        on_trial,
        Probe {
            health: Some(Health::Unhealthy),
            ..Default::default()
        },
    );
    n.events();
    assert_eq!(n.agent.confirm_own(), Err(Status::State));
    assert_eq!(n.store().confirms, 0);
    assert_eq!(n.events(), ["self_check", "confirm_refused"]);
}

/// Every failure lands in a defined phase of the published snapshot: a
/// refused, rejected, failed or dropped transfer and a refused SET_PENDING
/// are `Failed`; a refused confirm leaves the trial running (`Trial`).
#[test]
fn every_failure_lands_in_a_defined_phase() {
    // Refused by DAGON: the running slot.
    let mut n = Node::new(Probe::default());
    assert_eq!(n.phase(), Phase::Idle);
    assert_eq!(n.transfer(0, &image(0, &body())), Status::State);
    assert_eq!(dedup(n.events()).last().unwrap(), "update_refused");
    assert_eq!(n.phase(), Phase::Failed);

    // Refused by an extension.
    let mut n = Node::new(Probe {
        refuse_update: true,
        ..Default::default()
    });
    assert_eq!(n.transfer(1, &image(1, &body())), Status::State);
    assert_eq!(n.events().last().unwrap(), "update_refused");
    assert_eq!(n.phase(), Phase::Failed);

    // A header that fails its signature.
    let mut n = Node::new(Probe::default());
    let mut img = image(1, &body());
    img[0x018] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Signature);
    assert_eq!(dedup(n.events()).last().unwrap(), "header_rejected");
    assert_eq!(n.phase(), Phase::Failed);

    // A body that fails its digest, then a refused SET_PENDING.
    let mut n = Node::new(Probe::default());
    let mut img = image(1, &body());
    img[HEADER_SIZE + 10] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Digest);
    assert_eq!(n.events().last().unwrap(), "verify_failed");
    assert_eq!(n.phase(), Phase::Failed);
    let mut n = Node::new(Probe::default());
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::State);
    assert_eq!(n.events().last().unwrap(), "pending_refused");
    assert_eq!(n.phase(), Phase::Failed);

    // A transfer dropped after silence.
    let mut n = Node::new(Probe::default());
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: 1512,
        hdr_len: 512,
    };
    n.call(Cmd::XferBegin, &Node::enc(begin, XferBegin::encode));
    n.agent.abort();
    assert_eq!(n.events().last().unwrap(), "transfer_aborted");
    assert_eq!(n.phase(), Phase::Failed);

    // A refused confirm: the trial goes on.
    let mut m = Mem::new();
    b_on_trial(&mut m);
    let mut n = Node::with(
        m,
        Probe {
            health: Some(Health::Unhealthy),
            ..Default::default()
        },
    );
    assert_eq!(n.phase(), Phase::Trial);
    assert_eq!(n.agent.confirm_own(), Err(Status::State));
    assert_eq!(n.events().last().unwrap(), "confirm_refused");
    assert_eq!(n.phase(), Phase::Trial);
}

#[test]
fn an_idle_transfer_is_aborted() {
    let mut n = Node::new(Probe::default());
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: 1512,
        hdr_len: 512,
    };
    n.call(Cmd::XferBegin, &Node::enc(begin, XferBegin::encode));
    n.agent.abort();
    n.agent.abort();
    assert_eq!(
        n.events(),
        [
            "starting",
            "store_opened",
            "update_offered",
            "transfer_aborted"
        ]
    );
    n.agent.before_reboot();
    assert_eq!(n.events(), ["before_reboot"]);
}

#[test]
fn retried_request_is_not_executed_twice() {
    let mut n = Node::new(Probe::default());
    let img = image(1, &body());
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: img.len() as u32,
        hdr_len: 512,
    };
    n.call(Cmd::XferBegin, &Node::enc(begin, XferBegin::encode));
    for (i, c) in img[..HEADER_SIZE].chunks(252).enumerate() {
        let p = Node::enc(
            XferChunk {
                offset: (i * 252) as u32,
                data: c,
            },
            XferChunk::encode,
        );
        assert_eq!(n.call(Cmd::XferHdr, &p).0, Status::Ok);
    }
    let p = Node::enc(
        XferChunk {
            offset: 0,
            data: &img[HEADER_SIZE..][..252],
        },
        XferChunk::encode,
    );
    let first = n.call_seq(0x40, Cmd::XferData, &p);
    let again = n.call_seq(0x40, Cmd::XferData, &p);
    assert_eq!(first, again);
    assert_eq!(n.store().writes, 1);
}

#[test]
fn unknown_and_hub_commands_are_unsupported_on_nodes() {
    let mut n = Node::new(Probe::default());
    assert_eq!(
        n.call(Cmd::Show, &[1, 1, 0, 0, 0, 0, 0]).0,
        Status::Unsupported
    );
    // The app handles ENTER_ROM before the agent sees it.
    assert_eq!(n.call(Cmd::EnterRom, &[1]).0, Status::Unsupported);
    let begin = XferBegin {
        target: target::TOPOLOGY,
        slot: 0,
        total_len: 64,
        hdr_len: 0,
    };
    assert_eq!(
        n.call(Cmd::XferBegin, &Node::enc(begin, XferBegin::encode))
            .0,
        Status::Unsupported
    );
    assert_eq!(
        n.call(Cmd::XferData, &[0, 0, 0, 0, 1]).0,
        Status::State,
        "no session"
    );
}

#[test]
fn the_agent_declares_its_machines() {
    let names: Vec<_> = dagon_agent::MACHINES
        .catalogs()
        .iter()
        .map(|c| c.machine)
        .collect();
    assert_eq!(names, ["image", "session", "node", "requests"]);
    assert_eq!(session::CATALOG.initial, "Idle");
    assert_eq!(requests::CATALOG.states, ["Ready"]);
}

#[test]
fn switching_away_from_a_pending_or_trial_slot_moves_or_withdraws_it() {
    let mut n = Node::new(Probe::default());
    assert_eq!(n.transfer(1, &image(1, &body())), Status::Ok);
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::Ok);
    n.reset(|m| {
        // The confirmed image in A, then B started on trial.
        let a = image(0, &body());
        m.headers[0].copy_from_slice(&a[..HEADER_SIZE]);
        m.bodies[0][..a.len() - HEADER_SIZE].copy_from_slice(&a[HEADER_SIZE..]);
        b_on_trial(m);
    });
    n.events();

    // Back to the confirmed slot A while B is on trial: B is an image again.
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 0]).0, Status::Ok);
    assert_eq!(n.events(), ["pending_set", "pending_moved"]);
    assert_eq!(n.agent.images().state(0), Some(image::State::Pending));
    assert_eq!(n.agent.images().state(1), Some(image::State::Written));

    // Then B again: A, still the confirmed slot, is the confirmed one again.
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::Ok);
    assert_eq!(n.events(), ["pending_set", "pending_withdrawn"]);
    assert_eq!(n.agent.images().state(0), Some(image::State::Confirmed));
    assert_eq!(n.agent.images().state(1), Some(image::State::Pending));
}

#[test]
fn a_header_that_does_not_read_back_leaves_no_image() {
    let mut n = Node::new(Probe::default());
    n.store().bad_readback = true;
    assert_eq!(n.transfer(1, &image(1, &body())), Status::Flash);
    assert_eq!(n.store().headers[1], [0xFF; HEADER_SIZE], "erased again");
    assert_eq!(n.store().status.slots[1].state, SlotState::Empty);
    assert_eq!(n.agent.images().state(1), Some(image::State::Empty));
    let ev = n.events();
    assert!(!ev.contains(&"written".to_string()), "{ev:?}");
    assert_eq!(ev.last().map(String::as_str), Some("verify_failed"));

    n.store().bad_readback = false;
    assert_eq!(
        n.transfer(1, &image(1, &body())),
        Status::Ok,
        "the slot takes the next one"
    );
}

#[test]
fn a_restart_reads_the_slots_again() {
    let mut n = Node::new(Probe::default());
    assert_eq!(n.transfer(1, &image(1, &body())), Status::Ok);
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::Ok);
    n.events();
    b_on_trial(&mut n.store());
    n.agent.restart();
    assert_eq!(
        n.events(),
        ["starting", "store_opened", "replay:trial_started"]
    );
    assert!(n.agent.images().on_trial());
}
