//! Receive state machine against an in-memory slot store.

use dagon_agent::{Agent, Config, DefaultHooks, Hooks, Progress, SlotStore, Stage, StoreError};
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
    fn erase(&mut self, _t: u8, slot: u8, len: u32) -> Result<(), StoreError> {
        self.erases += 1;
        self.bodies[slot as usize][..len as usize].fill(0xFF);
        self.headers[slot as usize] = [0xFF; HEADER_SIZE];
        Ok(())
    }
    fn write(&mut self, _t: u8, slot: u8, off: u32, data: &[u8]) -> Result<(), StoreError> {
        self.writes += 1;
        self.bodies[slot as usize][off as usize..][..data.len()].copy_from_slice(data);
        Ok(())
    }
    fn read(&mut self, _t: u8, slot: u8, off: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let src = self.bodies[slot as usize].get(off as usize..off as usize + buf.len());
        buf.copy_from_slice(src.ok_or(StoreError::Range)?);
        Ok(())
    }
    fn write_header(&mut self, slot: u8, h: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.headers[slot as usize] = *h;
        Ok(())
    }
    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        *out = self.headers[slot as usize];
        Ok(())
    }
    fn set_pending(&mut self, slot: u8, _h: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.pending = Some(slot);
        self.status.slots[slot as usize].state = SlotState::Pending;
        Ok(())
    }
    fn confirm(&mut self, slot: u8, _h: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.confirms += 1;
        self.status.slots[slot as usize].state = SlotState::Confirmed;
        self.status.confirmed = slot;
        Ok(())
    }
}

#[derive(Default)]
struct TestHooks {
    refuse_update: bool,
    fail_self_check: bool,
    stages: Vec<Stage>,
}

impl Hooks for TestHooks {
    fn self_check(&mut self) -> bool {
        !self.fail_self_check
    }
    fn accept_update(&mut self, _r: &XferBegin) -> bool {
        !self.refuse_update
    }
    fn on_progress(&mut self, p: Progress) {
        if self.stages.last() != Some(&p.stage) {
            self.stages.push(p.stage);
        }
    }
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
    }
}

struct Node<H: Hooks> {
    agent: Agent<Mem, H>,
    seq: u8,
}

impl<H: Hooks> Node<H> {
    fn new(hooks: H) -> Self {
        Node {
            agent: Agent::new(config(), Mem::new(), hooks),
            seq: 0,
        }
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

    fn store(&mut self) -> &mut Mem {
        self.agent.store()
    }
}

fn body() -> Vec<u8> {
    (0..1000u32).map(|i| (i * 13) as u8).collect()
}

#[test]
fn update_set_pending_reboot_confirm() {
    let mut n = Node::new(TestHooks::default());
    let img = image(1, &body());
    assert_eq!(n.transfer(1, &img), Status::Ok);
    assert_eq!(
        n.store().headers[1][..],
        img[..HEADER_SIZE],
        "header written last"
    );
    assert_eq!(
        n.agent.hooks().stages,
        [Stage::Erase, Stage::Body, Stage::Verify]
    );

    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::Ok);
    assert_eq!(n.store().pending, Some(1));
    let (st, _, reboot) = n.call(Cmd::Reboot, &[reboot_mode::NORMAL]);
    assert_eq!((st, reboot), (Status::Ok, true));

    // After the boot core starts B on trial.
    let s = &mut n.store().status;
    s.active = 1;
    s.slots[1] = SlotInfo {
        state: SlotState::Trying,
        version: Version::new(1, 1, 0),
        digest16: [0; 16],
    };
    let hello = Node::<TestHooks>::enc(
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
    let (_, body, _) = n.call(Cmd::Status, &[]);
    assert_eq!(StatusResp::parse(&body).unwrap().confirmed, 1);
}

#[test]
fn tampered_header_is_rejected_without_erase() {
    let mut n = Node::new(DefaultHooks);
    let mut img = image(1, &body());
    img[0x018] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Signature);
    assert_eq!(n.store().erases, 0);
}

#[test]
fn tampered_body_is_rejected_without_header() {
    let mut n = Node::new(DefaultHooks);
    let mut img = image(1, &body());
    img[HEADER_SIZE + 10] ^= 1;
    assert_eq!(n.transfer(1, &img), Status::Digest);
    assert_eq!(
        n.store().headers[1],
        [0xFF; HEADER_SIZE],
        "slot stays imageless"
    );
    assert_eq!(n.call(Cmd::SetPending, &[target::APP, 1]).0, Status::State);
}

#[test]
fn wrong_board_or_key_is_rejected() {
    let mut n = Node::new(DefaultHooks);
    n.agent = Agent::new(
        Config {
            target_tag: target_tag("other"),
            ..config()
        },
        Mem::new(),
        DefaultHooks,
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
    let mut n = Node::new(DefaultHooks);
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
}

#[test]
fn hooks_can_refuse() {
    let mut n = Node::new(TestHooks {
        refuse_update: true,
        ..Default::default()
    });
    assert_eq!(n.transfer(1, &image(1, &body())), Status::State);
    assert_eq!(n.store().erases, 0);

    let mut n = Node::new(TestHooks {
        fail_self_check: true,
        ..Default::default()
    });
    n.store().status.active = 1;
    n.store().status.slots[1].state = SlotState::Trying;
    assert_eq!(n.call(Cmd::Confirm, &[target::APP]).0, Status::State);
    assert_eq!(n.store().confirms, 0);
}

#[test]
fn retried_request_is_not_executed_twice() {
    let mut n = Node::new(DefaultHooks);
    let img = image(1, &body());
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: img.len() as u32,
        hdr_len: 512,
    };
    n.call(
        Cmd::XferBegin,
        &Node::<DefaultHooks>::enc(begin, XferBegin::encode),
    );
    for (i, c) in img[..HEADER_SIZE].chunks(252).enumerate() {
        let p = Node::<DefaultHooks>::enc(
            XferChunk {
                offset: (i * 252) as u32,
                data: c,
            },
            XferChunk::encode,
        );
        assert_eq!(n.call(Cmd::XferHdr, &p).0, Status::Ok);
    }
    let p = Node::<DefaultHooks>::enc(
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
    let mut n = Node::new(DefaultHooks);
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
        n.call(
            Cmd::XferBegin,
            &Node::<DefaultHooks>::enc(begin, XferBegin::encode)
        )
        .0,
        Status::Unsupported
    );
    assert_eq!(
        n.call(Cmd::XferData, &[0, 0, 0, 0, 1]).0,
        Status::State,
        "no session"
    );
}
