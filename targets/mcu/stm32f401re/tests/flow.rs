//! The F401 store behind dagon-agent over an in-memory flash with the board
//! geometry, with the boot core's decision between resets: update into the
//! inactive slot, trial, CONFIRM, then SET_PENDING back to the previously
//! confirmed slot.

use dagon_agent::{Agent, Config, SlotStore, Transition};
use dagon_core::boot::{decide, BootConfig, Decision, SlotLayout};
use dagon_core::log::{Log, LogLayout, SlotRef};
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::{kind, reason, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_stm32f401re::KindSStore;
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::*;
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const BASE: u32 = 0x0800_0000;
const TAG: [u8; 16] = target_tag("seg-f401-7seg");
const LOG: LogLayout = LogLayout {
    faces: [0x0800_8000, 0x0800_C000],
    face_len: 0x4000,
};
const SLOTS: SlotLayout = SlotLayout {
    slots: [0x0802_0000, 0x0804_0000],
    slot_len: 0x2_0000,
};
const GEOMETRY: [FlashRegion; 2] = [
    FlashRegion {
        start: 0x0800_8000,
        len: 0x8000,
        erase_size: 0x4000,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: 0x0802_0000,
        len: 0x4_0000,
        erase_size: 0x2_0000,
        write_size: 4,
        erased_value: 0xFF,
    },
];
const A: u8 = 0;
const B: u8 = 1;

#[derive(Default)]
struct Mem {
    bytes: Vec<u8>,
    erases: Vec<(u32, u32)>,
}

impl Mem {
    fn new() -> Self {
        Mem {
            bytes: vec![0xFF; 512 * 1024],
            erases: Vec::new(),
        }
    }
    fn range(&self, addr: u32, len: usize) -> Result<std::ops::Range<usize>, FlashError> {
        let start = addr.checked_sub(BASE).ok_or(FlashError::Range)? as usize;
        (start + len <= self.bytes.len())
            .then_some(start..start + len)
            .ok_or(FlashError::Range)
    }
}

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &GEOMETRY
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let r = self.range(addr, buf.len())?;
        buf.copy_from_slice(&self.bytes[r]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let r = self.range(addr, len as usize)?;
        self.bytes[r].fill(0xFF);
        self.erases.push((addr, len));
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let r = self.range(addr, data.len())?;
        if self.bytes[r.clone()].iter().any(|&b| b != 0xFF) {
            return Err(FlashError::Hw);
        }
        self.bytes[r].copy_from_slice(data);
        Ok(())
    }
}

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

/// Signed image for `slot`, body length not a multiple of the write unit.
fn image(slot: u8, minor: u8) -> Vec<u8> {
    let kp = keypair();
    let body: Vec<u8> = (0..1001u32).map(|i| (i * 7 + minor as u32) as u8).collect();
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: SLOTS.slots[slot as usize] + HEADER_SIZE as u32,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], &body].concat()
}

/// Factory state: v1.0 in slot A, provisioned and confirmed.
fn provisioned() -> Mem {
    let mut m = Mem::new();
    let img = image(A, 0);
    let base = SLOTS.slots[0];
    m.bytes[(base - BASE) as usize..][..img.len()].copy_from_slice(&img);
    m.write(LOG.faces[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let mut log = Log::load(&mut m, &LOG).unwrap();
    let hdr: [u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
    let at = SlotRef {
        slot: A,
        version: Version::new(1, 0, 0),
        digest16: header_digest16(&hdr),
    };
    for k in [kind::PROVISION, kind::CONFIRMED] {
        log.append(&mut m, k, at, reason::NONE).unwrap();
    }
    m
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

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

/// The app layer's chain of one start (no extensions).
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

struct Node {
    agent: Agent<KindSStore<Mem>, Chain>,
    seq: u8,
}

impl Node {
    /// Reset: the boot core decides, then the app starts its agent.
    fn boot(mut m: Mem) -> (Node, Decision) {
        let cfg = BootConfig {
            pubkey: *keypair().pk,
            mcu_id: mcu_id::STM32F401XE,
            target_tag: TAG,
            max_attempts: 3,
        };
        let mut log = Log::load(&mut m, &LOG).unwrap();
        let d = decide(&mut m, &mut log, &SLOTS, &cfg);
        let store = KindSStore::load(m, &LOG, SLOTS).unwrap();
        let node = Node {
            agent: Agent::new(config(), store, chain()),
            seq: 0,
        };
        (node, d)
    }

    fn reboot(mut self) -> (Node, Decision) {
        let (st, _, reboot) = self.call(Cmd::Reboot, &[reboot_mode::NORMAL]);
        assert_eq!((st, reboot), (Status::Ok, true));
        let m = std::mem::take(self.agent.store().flash());
        Node::boot(m)
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>, bool) {
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
        let (st, body) = split_status(f.payload).unwrap();
        (st, body.to_vec(), reply.reboot)
    }

    fn transfer(&mut self, slot: u8, img: &[u8]) -> Status {
        let mut p = [0u8; 300];
        let begin = XferBegin {
            target: target::APP,
            slot,
            total_len: img.len() as u32,
            hdr_len: HEADER_SIZE as u16,
        };
        let n = begin.encode(&mut p).unwrap();
        let st = self.call(Cmd::XferBegin, &p[..n]).0;
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        // 250-byte chunks: not a multiple of the write unit.
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, chunk) in part.chunks(250).enumerate() {
                let c = XferChunk {
                    offset: (i * 250) as u32,
                    data: chunk,
                };
                let n = c.encode(&mut p).unwrap();
                let st = self.call(cmd, &p[..n]).0;
                if st != Status::Ok {
                    return st;
                }
            }
        }
        self.call(Cmd::XferEnd, &[]).0
    }

    fn set_pending(&mut self, slot: u8) -> Status {
        let req = SetPending {
            target: target::APP,
            slot,
        };
        let mut p = [0u8; 8];
        let n = req.encode(&mut p).unwrap();
        self.call(Cmd::SetPending, &p[..n]).0
    }

    fn status(&mut self) -> StatusResp {
        self.agent.store().status()
    }

    fn hello(&mut self) -> HelloResp {
        let req = HelloReq {
            proto_ver: dagon_proto::PROTO_VERSION as u32,
            max_payload: max_payload::I2C as u16,
        };
        let mut p = [0u8; 8];
        let n = req.encode(&mut p).unwrap();
        let (st, body, _) = self.call(Cmd::Hello, &p[..n]);
        assert_eq!(st, Status::Ok);
        HelloResp::parse(&body).unwrap()
    }
}

fn booted(d: Decision) -> (u8, bool) {
    match d {
        Decision::Boot { slot, trial, .. } => (slot, trial),
        Decision::NoImage => panic!("no image"),
    }
}

#[test]
fn update_confirm_then_back_to_previous_version() {
    let (mut n, d) = Node::boot(provisioned());
    assert_eq!(booted(d), (A, false));
    let h = n.hello();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 0, 0), false));

    // The running, confirmed slot is never a receive target.
    assert_eq!(n.transfer(A, &image(A, 1)), Status::State);
    assert_eq!(n.transfer(B, &image(B, 1)), Status::Ok);
    let st = n.status();
    assert_eq!(st.slots[B as usize].state, SlotState::Written);
    assert_eq!(st.slots[B as usize].version, Version::new(1, 1, 0));
    assert_eq!(n.set_pending(B), Status::Ok);
    assert_eq!(n.status().slots[B as usize].state, SlotState::Pending);

    let (mut n, d) = n.reboot();
    assert_eq!(booted(d), (B, true));
    let h = n.hello();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 1, 0), true));
    assert_eq!((n.status().active, n.status().confirmed), (B, A));
    assert_eq!(n.call(Cmd::Confirm, &[target::APP]).0, Status::Ok);
    let st = n.status();
    assert_eq!(st.slots[B as usize].state, SlotState::Confirmed);
    assert_eq!(st.slots[A as usize].state, SlotState::Written);
    assert_eq!((st.active, st.confirmed), (B, B));

    let (mut n, d) = n.reboot();
    assert_eq!(booted(d), (B, false));
    // Back to the version confirmed before: A still holds v1.0.
    assert_eq!(n.set_pending(A), Status::Ok);
    assert_eq!(n.status().slots[A as usize].state, SlotState::Pending);
    let (mut n, d) = n.reboot();
    assert_eq!(booted(d), (A, true));
    assert_eq!(n.hello().active_version, Version::new(1, 0, 0));
    assert_eq!(n.call(Cmd::Confirm, &[target::APP]).0, Status::Ok);
    let (mut n, d) = n.reboot();
    assert_eq!(booted(d), (A, false));
    assert_eq!(n.status().confirmed, A);
}

#[test]
fn erase_covers_header_and_body_units_only() {
    let (mut n, _) = Node::boot(provisioned());
    assert_eq!(n.transfer(B, &image(B, 1)), Status::Ok);
    let m = n.agent.store().flash();
    assert_eq!(m.erases, [(SLOTS.slots[1], 0x2_0000)]);
}

#[test]
fn unconfirmed_trial_rolls_back_to_confirmed_slot() {
    let (mut n, _) = Node::boot(provisioned());
    assert_eq!(n.transfer(B, &image(B, 1)), Status::Ok);
    assert_eq!(n.set_pending(B), Status::Ok);
    let (mut n, d) = n.reboot();
    assert_eq!(booted(d), (B, true));
    // Trial watchdog resets without CONFIRM, three times.
    for _ in 0..3 {
        let m = std::mem::take(n.agent.store().flash());
        let (next, d) = Node::boot(m);
        n = next;
        if booted(d) == (A, false) {
            let st = n.status();
            assert_eq!(st.slots[B as usize].state, SlotState::Reverted);
            assert_eq!(st.last_reason, reason::TRIAL_EXHAUSTED);
            return;
        }
    }
    panic!("no rollback");
}

#[test]
fn topology_target_is_unsupported() {
    let (mut n, _) = Node::boot(provisioned());
    assert_eq!(n.agent.store().capacity(target::TOPOLOGY, 0), 0);
    assert_eq!(n.agent.store().capacity(target::APP, 2), 0);
}

#[test]
fn unaligned_chunks_are_buffered_and_flushed_padded() {
    let mut s = KindSStore::load(provisioned(), &LOG, SLOTS).unwrap();
    let t = Transition::for_store_tests();
    let data: Vec<u8> = (1..=10).collect();
    s.erase(target::APP, B, 10, &t).unwrap();
    let mut off = 0;
    for n in [3, 1, 5, 1] {
        s.write(target::APP, B, off, &data[off as usize..][..n], &t)
            .unwrap();
        off += n as u32;
    }
    let mut back = [0u8; 12];
    s.read(target::APP, B, 0, &mut back).unwrap();
    assert_eq!(&back[..10], &data[..]);
    assert_eq!(&back[10..], [0xFF, 0xFF]);
    // Offsets must continue where the last write ended.
    s.erase(target::APP, B, 10, &t).unwrap();
    assert!(s.write(target::APP, B, 4, &data[..4], &t).is_err());
}
