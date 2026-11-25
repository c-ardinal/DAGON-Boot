//! The factory recovery agent end to end on the host: the dagon-agent over
//! `EspStore`, running factory with blank otadata, receiving signed images
//! into the OTA slots, like `dagon deploy watch` through the coordinator.

use std::cell::{RefCell, RefMut};

use crate::layout::{OTA_0, SLOT_A, SLOT_B, SLOT_FACTORY};
use crate::tests::{store, Store};
use dagon_agent::{image::State as Image, Agent};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

const MAX: usize = 1024;
const TAG: &str = "node-s3x";

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new([0x42; 32]))
}

fn image(minor: u8, body: &[u8], kp: &KeyPair) -> Vec<u8> {
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::VENDOR_STAGE2,
        mcu_id: McuId::of_name("esp32s3"),
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
    /// The slot store, shared with the bench (it plays the 2nd stage).
    store: &'static RefCell<Store>,
    agent: Agent<&'static RefCell<Store>, Chain, Ed25519Compact>,
    seq: u8,
}

impl Pc {
    fn config() -> dagon_agent::Config {
        crate::config(mode::AGENT, crate::target_tag(TAG))
    }

    fn verifier() -> Ed25519Compact {
        Ed25519Compact::new(*keypair().pk)
    }

    fn factory() -> Self {
        Self::over(store())
    }

    /// The factory agent starting on the flash `store` holds.
    fn over(store: Store) -> Self {
        let store: &'static RefCell<Store> = Box::leak(Box::new(RefCell::new(store)));
        Pc {
            store,
            agent: Agent::new(Self::config(), Self::verifier(), store, chain()),
            seq: 0,
        }
    }

    /// A reset: the 2nd stage boots, the app starts its agent again.
    fn reboot(&mut self) {
        self.store.borrow_mut().ota.boot();
        self.agent = Agent::new(Self::config(), Self::verifier(), self.store, chain());
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
        let body = bulk_of(f.cmd, st, body);
        (st, body.to_vec())
    }

    fn hello(&mut self) -> HelloResp {
        let p = wire(|b| {
            HelloReq {
                proto_ver: 1,
                max_payload: MAX as u16,
            }
            .encode(b)
        });
        let (st, body) = self.call(Cmd::Hello, &p);
        assert_eq!(st, Status::Ok);
        HelloResp::parse(&body).unwrap()
    }

    fn status(&mut self) -> StatusResp {
        let (st, body) = self.call(Cmd::Status, &[]);
        assert_eq!(st, Status::Ok);
        StatusResp::parse(&body).unwrap()
    }

    /// XFER_BEGIN, header in 128-byte pieces, body in full-size pieces,
    /// XFER_END: the status of the first step that is not OK.
    fn transfer(&mut self, slot: u8, img: &[u8]) -> Status {
        self.transfer_to(target::APP, slot, img, usize::MAX)
    }

    /// [`Pc::transfer`] to a slot of `t`, stopping after `requests`
    /// requests (a power cut between two of them): `Status::Timeout` then.
    fn transfer_to(&mut self, t: u8, slot: u8, img: &[u8], requests: usize) -> Status {
        let mut left = requests;
        let mut p = [0u8; 32];
        let n = XferBegin {
            target: t,
            slot,
            total_len: img.len() as u32,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .unwrap();
        let mut steps: Vec<(Cmd, Vec<u8>)> = vec![(Cmd::XferBegin, p[..n].to_vec())];
        for (cmd, part, size) in [
            (Cmd::XferHdr, &img[..HEADER_SIZE], 128),
            (
                Cmd::XferData,
                &img[HEADER_SIZE..],
                MAX - XferChunk::OVERHEAD,
            ),
        ] {
            for (i, c) in part.chunks(size).enumerate() {
                let b = wire(|b| {
                    XferChunk {
                        offset: (i * size) as u32,
                        data: c,
                    }
                    .encode(b)
                });
                steps.push((cmd, b));
            }
        }
        steps.push((Cmd::XferEnd, Vec::new()));
        for (cmd, b) in steps {
            if left == 0 {
                return Status::Timeout;
            }
            left -= 1;
            let (st, _) = self.call(cmd, &b);
            if st != Status::Ok {
                return st;
            }
        }
        Status::Ok
    }

    fn set_pending(&mut self, slot: u8) -> Status {
        self.call(
            Cmd::SetPending,
            &wire(|b| {
                dagon_proto::payload::SetPending {
                    target: target::APP,
                    slot,
                }
                .encode(b)
            }),
        )
        .0
    }

    fn store(&mut self) -> RefMut<'static, Store> {
        self.store.borrow_mut()
    }
}

/// A signed factory recovery agent (kind AGENT, detached header, any slot).
fn agent_image(minor: u8, body: &[u8], kp: &KeyPair) -> Vec<u8> {
    let app = image(minor, body, kp);
    let mut h = ImageHeader::parse(app[..HEADER_SIZE].try_into().unwrap()).unwrap();
    h.image_kind = image_kind::AGENT;
    h.load_addr = crate::layout::FACTORY.offset;
    h.build_flags = build_flags::VERIFY_ENFORCED;
    h.capabilities = capabilities::SIG_VERIFIER;
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
}

/// From the factory agent: app 1.0 in A, confirmed and running.
fn app_confirmed_in_a(kp: &KeyPair) -> Pc {
    confirm_app_in_a(Pc::factory(), kp)
}

/// [`app_confirmed_in_a`] on the flash `pc` holds.
fn confirm_app_in_a(mut pc: Pc, kp: &KeyPair) -> Pc {
    assert_eq!(
        pc.transfer(SLOT_A, &image(0, &body(3000, 1), kp)),
        Status::Ok
    );
    assert_eq!(pc.set_pending(SLOT_A), Status::Ok);
    pc.reboot();
    assert_eq!(
        pc.call(
            Cmd::Confirm,
            &wire(|b| dagon_proto::payload::encode_confirm(target::APP, b))
        )
        .0,
        Status::Ok
    );
    pc.reboot();
    let st = pc.status();
    assert_eq!((st.active, st.confirmed), (SLOT_A, SLOT_A));
    pc
}

/// The recovery agent in the factory partition is written through the
/// release like the app, by the running app's agent: body first, the
/// detached header last; it is not selected or confirmed (the 2nd stage
/// starts factory only when no OTA slot is valid).
#[test]
fn the_running_app_writes_a_new_factory_agent() {
    let kp = keypair();
    let mut pc = app_confirmed_in_a(&kp);
    let ag = agent_image(3, &body(5000, 9), &kp);
    assert_eq!(
        pc.transfer_to(target::AGENT, SLOT_FACTORY, &ag, usize::MAX),
        Status::Ok
    );
    let st = pc.status();
    assert_eq!(st.slots[2].state, SlotState::Written);
    assert_eq!(st.slots[2].version, Version::new(1, 3, 0));
    assert_eq!((st.active, st.confirmed), (SLOT_A, SLOT_A));
    let at = crate::layout::FACTORY.offset as usize;
    assert_eq!(&pc.store().flash.0[at..at + 5000], &ag[HEADER_SIZE..]);
    let h = crate::store::header_addr(dagon_proto::payload::target::AGENT, SLOT_FACTORY).unwrap()
        as usize;
    assert_eq!(&pc.store().flash.0[h..h + HEADER_SIZE], &ag[..HEADER_SIZE]);
    assert_eq!(
        pc.call(
            Cmd::SetPending,
            &wire(|b| dagon_proto::payload::SetPending {
                target: target::AGENT,
                slot: SLOT_FACTORY
            }
            .encode(b))
        )
        .0,
        Status::Arg,
        "the factory is never selected through DAGON"
    );
    // An app image is not taken as the agent.
    let app = image(4, &body(5000, 9), &kp);
    assert_eq!(
        pc.transfer_to(target::AGENT, SLOT_FACTORY, &app, usize::MAX),
        Status::Compat
    );
}

/// A first write as the host lays it out: the agent's body from the start
/// of factory and its header in the partition's last erase unit, the dagon
/// partition (the detached headers) erased.
fn first_written(ag: &[u8]) -> Store {
    let mut st = store();
    let f = crate::layout::FACTORY;
    st.flash.0[f.offset as usize..][..ag.len() - HEADER_SIZE].copy_from_slice(&ag[HEADER_SIZE..]);
    st.flash.0[copy_at()..][..HEADER_SIZE].copy_from_slice(&ag[..HEADER_SIZE]);
    st
}

/// Where the first write leaves the agent's header copy.
fn copy_at() -> usize {
    let f = crate::layout::FACTORY;
    header_copy_at(f.offset, f.size, crate::layout::ERASE_UNIT, 0).unwrap() as usize
}

/// With the dagon partition erased, STATUS reports the factory agent from
/// the header copy a first write left in the factory partition.
#[test]
fn status_reports_the_factory_agent_from_its_header_copy() {
    let kp = keypair();
    let ag = agent_image(7, &body(5000, 9), &kp);
    let mut pc = Pc::over(first_written(&ag));
    let st = pc.status();
    assert_eq!(st.slots[2].state, SlotState::Confirmed, "factory runs");
    assert_eq!(st.slots[2].version, Version::new(1, 7, 0));
    assert_eq!(
        st.slots[2].digest16,
        header_digest16(ag[..HEADER_SIZE].try_into().unwrap())
    );
}

/// A copy that does not name the body in factory is not reported: another
/// body, an app's header, a length reaching the copy.
#[test]
fn a_header_copy_that_is_not_the_bodys_is_not_reported() {
    let kp = keypair();
    let ag = agent_image(7, &body(5000, 9), &kp);
    let none = |st: Store, why: &str| {
        let version = Pc::over(st).status().slots[2].version;
        assert_eq!(version, Version::default(), "{why}");
    };

    let mut st = first_written(&ag);
    st.flash.0[crate::layout::FACTORY.offset as usize + 4999] ^= 1;
    none(st, "another body");

    none(
        first_written(&image(7, &body(5000, 9), &kp)),
        "an app's header",
    );

    let mut st = first_written(&ag);
    let mut h = ImageHeader::parse(ag[..HEADER_SIZE].try_into().unwrap()).unwrap();
    h.payload_size = crate::layout::FACTORY.size;
    st.flash.0[copy_at()..][..HEADER_SIZE].copy_from_slice(&h.to_bytes());
    none(st, "a length reaching the copy");
}

/// An agent received later is reported from its detached header; the first
/// write's copy, which names the old body, is not reported once that
/// header is gone again.
#[test]
fn a_received_agent_replaces_the_header_copy() {
    let kp = keypair();
    let pc = Pc::over(first_written(&agent_image(7, &body(5000, 9), &kp)));
    let mut pc = confirm_app_in_a(pc, &kp);
    assert_eq!(pc.status().slots[2].version, Version::new(1, 7, 0));
    let new = agent_image(8, &body(6000, 3), &kp);
    assert_eq!(
        pc.transfer_to(target::AGENT, SLOT_FACTORY, &new, usize::MAX),
        Status::Ok
    );
    assert_eq!(pc.status().slots[2].version, Version::new(1, 8, 0));
    let h = crate::store::header_addr(target::AGENT, SLOT_FACTORY).unwrap() as usize;
    pc.store().flash.0[h..h + HEADER_SIZE].fill(0xFF);
    assert_eq!(pc.status().slots[2].version, Version::default());
}

/// Never without a bootable recovery image: the factory is written only
/// while a confirmed app runs from an OTA slot (that app's agent is the
/// way back if the write is cut), never while factory itself runs or is
/// the trial's fallback.
#[test]
fn the_factory_is_written_only_under_a_confirmed_app() {
    let kp = keypair();
    let ag = agent_image(3, &body(2000, 9), &kp);
    let mut pc = Pc::factory();
    assert_eq!(
        pc.transfer_to(target::AGENT, SLOT_FACTORY, &ag, usize::MAX),
        Status::State,
        "factory runs"
    );
    assert_eq!(
        pc.transfer(SLOT_A, &image(0, &body(3000, 1), &kp)),
        Status::Ok
    );
    assert_eq!(pc.set_pending(SLOT_A), Status::Ok);
    pc.reboot();
    assert_eq!(pc.status().confirmed, SLOT_FACTORY);
    assert_eq!(
        pc.transfer_to(target::AGENT, SLOT_FACTORY, &ag, usize::MAX),
        Status::State,
        "on trial, factory is the fallback"
    );
    assert_eq!(pc.status().slots[2].state, SlotState::Empty);
}

/// A power cut between any two requests of the factory write: the app
/// still starts (its agent answers), the factory holds no image a reader
/// takes as complete, and the write goes through when repeated.
#[test]
fn a_cut_factory_write_keeps_the_app_and_can_be_repeated() {
    let kp = keypair();
    let ag = agent_image(3, &body(5000, 9), &kp);
    let steps = 1 + HEADER_SIZE.div_ceil(128) + 5000usize.div_ceil(MAX - XferChunk::OVERHEAD) + 1;
    for cut in 0..steps {
        let mut pc = app_confirmed_in_a(&kp);
        let first = pc.transfer_to(target::AGENT, SLOT_FACTORY, &ag, cut);
        assert_eq!(first, Status::Timeout, "cut {cut}");
        pc.reboot();
        let st = pc.status();
        assert_eq!((st.active, st.confirmed), (SLOT_A, SLOT_A), "cut {cut}");
        assert_eq!(st.slots[2].state, SlotState::Empty, "cut {cut}");
        assert_eq!(
            pc.transfer_to(target::AGENT, SLOT_FACTORY, &ag, usize::MAX),
            Status::Ok,
            "cut {cut}: repeat"
        );
        assert_eq!(pc.status().slots[2].version, Version::new(1, 3, 0));
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
    assert_eq!(
        pc.call(
            Cmd::Confirm,
            &wire(|b| dagon_proto::payload::encode_confirm(target::APP, b))
        )
        .0,
        Status::Ok
    );
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

/// A re-transfer into the pending slot leaves it written, in STATUS and in
/// the agent's image machine alike, and the next reset does not try it.
#[test]
fn a_re_transfer_into_the_pending_slot_is_only_written() {
    let kp = keypair();
    let mut pc = Pc::factory();
    assert_eq!(
        pc.transfer(SLOT_A, &image(0, &body(3000, 1), &kp)),
        Status::Ok
    );
    assert_eq!(pc.set_pending(SLOT_A), Status::Ok);
    assert_eq!(pc.agent.images().state(SLOT_A), Some(Image::Pending));

    assert_eq!(
        pc.transfer(SLOT_A, &image(1, &body(2000, 2), &kp)),
        Status::Ok
    );
    let st = pc.status();
    assert_eq!(st.slots[0].state, SlotState::Written);
    assert_eq!(st.slots[0].version, Version::new(1, 1, 0));
    assert_eq!(pc.agent.images().state(SLOT_A), Some(Image::Written));
    pc.reboot();
    assert_eq!(
        pc.status().active,
        SLOT_FACTORY,
        "no trial without SET_PENDING"
    );
    assert_eq!(pc.agent.images().state(SLOT_A), Some(Image::Written));
}

/// The request payload `enc` writes.
#[allow(dead_code)]
fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The bulk of an OK RELAY or I2C_RAW response; any other body as it is.
#[allow(dead_code)]
fn bulk_of(cmd: u8, status: dagon_proto::Status, body: &[u8]) -> &[u8] {
    match dagon_proto::Cmd::from_u8(cmd) {
        Some(c @ (dagon_proto::Cmd::Relay | dagon_proto::Cmd::I2cRaw))
            if status == dagon_proto::Status::Ok =>
        {
            dagon_proto::payload::parse_bulk_resp(c, body).unwrap()
        }
        _ => body,
    }
}
