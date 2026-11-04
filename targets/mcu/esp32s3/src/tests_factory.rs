//! The factory recovery agent end to end on the host: the dagon-agent over
//! `EspStore`, running factory with blank otadata, receiving signed images
//! into the OTA slots, like `dagon recover watch` through the hub.

use crate::layout::{OTA_0, SLOT_A, SLOT_B, SLOT_FACTORY};
use crate::tests::{store, Store};
use dagon_agent::Agent;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

const MAX: usize = 1024;
const TAG: &str = "watch-s3";

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new([0x42; 32]))
}

fn image(minor: u8, body: &[u8], kp: &KeyPair) -> Vec<u8> {
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::VENDOR_STAGE2,
        mcu_id: mcu_id::ESP32S3,
        slot_binding: slot_binding::ANY,
        placement: placement::DETACHED,
        load_addr: OTA_0.offset,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: 0,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: crate::target_tag(TAG),
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
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
        Bounds::new(now, Watchdog::new(4000, 4000)),
    )
}

struct Pc {
    agent: Agent<Store, Chain>,
    seq: u8,
}

impl Pc {
    fn config() -> dagon_agent::Config {
        let mut cfg = crate::config(mode::AGENT, crate::target_tag(TAG));
        cfg.pubkey = *keypair().pk;
        cfg
    }

    fn factory() -> Self {
        Pc {
            agent: Agent::new(Self::config(), store(), chain()),
            seq: 0,
        }
    }

    /// A reset: the 2nd stage boots, the app starts its agent again.
    fn reboot(&mut self) {
        let mut s = std::mem::replace(self.agent.store(), store());
        s.ota.boot();
        self.agent = Agent::new(Self::config(), s, chain());
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
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
        assert_eq!((f.seq, f.flags), (self.seq, flags::RESP));
        let (st, body) = split_status(f.payload).unwrap();
        (st, body.to_vec())
    }

    fn hello(&mut self) -> HelloResp {
        let mut p = [0u8; 6];
        HelloReq {
            proto_ver: 1,
            max_payload: MAX as u16,
        }
        .encode(&mut p)
        .unwrap();
        let (st, body) = self.call(Cmd::Hello, &p);
        assert_eq!(st, Status::Ok);
        HelloResp::parse(&body).unwrap()
    }

    fn status(&mut self) -> StatusResp {
        let (st, body) = self.call(Cmd::Status, &[]);
        assert_eq!(st, Status::Ok);
        StatusResp::parse(&body).unwrap()
    }

    fn chunks(&mut self, cmd: Cmd, data: &[u8], size: usize) -> Status {
        let mut p = vec![0u8; size + 4];
        for (i, c) in data.chunks(size).enumerate() {
            let n = XferChunk {
                offset: (i * size) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            let (st, _) = self.call(cmd, &p[..n]);
            if st != Status::Ok {
                return st;
            }
        }
        Status::Ok
    }

    /// XFER_BEGIN, header in 128-byte pieces, body in full-size pieces,
    /// XFER_END: the status of the first step that is not OK.
    fn transfer(&mut self, slot: u8, img: &[u8]) -> Status {
        let mut p = [0u8; 8];
        let n = XferBegin {
            target: target::APP,
            slot,
            total_len: img.len() as u32,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .unwrap();
        let steps = [
            self.call(Cmd::XferBegin, &p[..n]).0,
            self.chunks(Cmd::XferHdr, &img[..HEADER_SIZE], 128),
            self.chunks(Cmd::XferData, &img[HEADER_SIZE..], MAX - 4),
        ];
        if let Some(s) = steps.into_iter().find(|&s| s != Status::Ok) {
            return s;
        }
        self.call(Cmd::XferEnd, &[]).0
    }

    fn set_pending(&mut self, slot: u8) -> Status {
        self.call(Cmd::SetPending, &[target::APP, slot]).0
    }

    fn store(&mut self) -> &mut Store {
        self.agent.store()
    }
}

fn body(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(13) ^ seed)
        .collect()
}

#[test]
fn factory_receives_a_signed_app_and_hands_over_to_it() {
    let kp = keypair();
    let mut pc = Pc::factory();
    let h = pc.hello();
    assert_eq!((h.mode, h.trial), (mode::AGENT, false));
    assert_eq!(pc.status().active, SLOT_FACTORY);

    let img = image(0, &body(3000, 1), &kp);
    assert_eq!(pc.transfer(SLOT_A, &img), Status::Ok);
    let st = pc.status();
    assert_eq!(st.slots[0].state, SlotState::Written);
    assert_eq!(st.slots[0].version, Version::new(1, 0, 0));
    assert_eq!(pc.set_pending(SLOT_A), Status::Ok);
    assert_eq!(pc.status().slots[0].state, SlotState::Pending);

    // Reboot: the 2nd stage tries ota_0, the app (same agent) confirms.
    pc.reboot();
    let st = pc.status();
    assert_eq!((st.active, st.confirmed), (SLOT_A, SLOT_FACTORY));
    assert!(pc.hello().trial);
    assert_eq!(pc.call(Cmd::Confirm, &[target::APP]).0, Status::Ok);
    pc.reboot();
    let st = pc.status();
    assert_eq!((st.active, st.confirmed), (SLOT_A, SLOT_A));
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
}

#[test]
fn factory_rejects_tampered_and_foreign_images_without_writing() {
    let kp = keypair();
    let mut pc = Pc::factory();
    let good = image(0, &body(2000, 2), &kp);

    // Signature bit flipped: refused at the last XFER_HDR, nothing erased.
    let mut bad_sig = good.clone();
    bad_sig[0x1C0] ^= 1;
    pc.store().flash.0[OTA_0.offset as usize] = 0x55;
    assert_eq!(pc.transfer(SLOT_A, &bad_sig), Status::Signature);
    assert_eq!(pc.store().flash.0[OTA_0.offset as usize], 0x55, "erased");
    pc.store().flash.0[OTA_0.offset as usize] = 0xFF;

    // Body byte flipped: E_DIGEST at XFER_END, the header is never written.
    let mut bad_body = good.clone();
    bad_body[HEADER_SIZE + 10] ^= 0x80;
    assert_eq!(pc.transfer(SLOT_B, &bad_body), Status::Digest);
    assert_eq!(pc.status().slots[1].state, SlotState::Empty);
    assert_eq!(pc.set_pending(SLOT_B), Status::State);

    // Signed by another key: E_KEY.
    let other = image(0, &body(2000, 2), &KeyPair::from_seed(Seed::new([7; 32])));
    assert_eq!(pc.transfer(SLOT_A, &other), Status::Key);
    assert_eq!(pc.status().slots[0].state, SlotState::Empty);

    // The good one still goes through.
    assert_eq!(pc.transfer(SLOT_B, &good), Status::Ok);
    assert_eq!(pc.set_pending(SLOT_B), Status::Ok);
}
