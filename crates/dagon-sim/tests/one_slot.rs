//! A node whose MCU definition gives the app and the boot core one slot
//! each: the store receives into slot A only, STATUS sends slot B as
//! empty, stage0 and the boot core start from slot A, and the agent over
//! it knows no slot B.

use dagon_agent::image::State as Image;
use dagon_agent::{Agent, SlotStore};
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::store::{Boot, Cores};
use dagon_sim::*;
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

/// The F401 layout with app slot A and boot core slot A only.
const ONE: Layout = Layout {
    slots: &[(0x0802_0000, 0x2_0000)],
    cores: match Layout::STM32F401XE.cores {
        Some(c) => Some(Cores {
            slots: &[(0x0801_0000, 0x1_0000)],
            ..c
        }),
        None => None,
    },
    ..Layout::STM32F401XE
};

fn kp() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn image(slot: u8, minor: u16) -> Vec<u8> {
    let kp = kp();
    let body = [minor as u8; 64];
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: ONE.slots[usize::from(slot)].0 + 0x200,
        payload_size: body.len() as u32,
        version: Version::new(1, 0, minor),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: f401_config(*kp.pk).target_tag,
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

/// A provisioned one-slot node, booted.
fn node() -> SimStore {
    let mut store = SimStore::new(SimFlash::stm32f401xe(3), ONE, *kp().pk);
    store.provision(0, &image(0, 0)).unwrap();
    assert_eq!(store.boot(), Boot::Run(0));
    store
}

#[test]
fn the_store_receives_into_the_slots_its_layout_gives_only() {
    let mut store = node();
    assert!(store.capacity(target::APP, 0) > 0);
    assert_eq!(store.capacity(target::APP, 1), 0);
    assert!(store.capacity(target::BOOTCORE, 0) > 0);
    assert_eq!(store.capacity(target::BOOTCORE, 1), 0);
    let st = store.status();
    assert_eq!((st.active, st.confirmed), (0, 0));
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
    assert_eq!(
        st.slots[1].state,
        SlotState::Empty,
        "an absent slot is sent as empty"
    );
    let core = store.target_status(target::BOOTCORE).unwrap();
    assert_eq!(core.slots[1].state, SlotState::Empty);
    // Every later reset starts the same slot.
    assert_eq!(store.boot(), Boot::Run(0));
}

#[test]
fn the_agent_over_a_one_slot_store_knows_no_slot_b() {
    let store = node();
    let mut agent = Agent::new(f401_config(*kp().pk), store, chain());
    let view = agent.images();
    assert_eq!(view.state(0), Some(Image::Confirmed));
    assert_eq!(view.state(1), None);
    assert_eq!(agent.core_state(1), None);
    let img = image(0, 1);
    let mut p = [0u8; 32];
    let n = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: img.len() as u32,
        hdr_len: IMAGE_HDR_LEN,
    }
    .encode(&mut p)
    .unwrap();
    let mut buf = vec![0u8; 300];
    let n = Frame {
        flags: 0,
        seq: 1,
        cmd: Cmd::XferBegin as u8,
        payload: &p[..n],
    }
    .encode(&mut buf)
    .unwrap();
    let reply = agent.handle(&buf[..n]).expect("reply");
    let (f, _) = Frame::decode(reply.frame, max_payload::USB).unwrap();
    assert_eq!(f.flags, flags::RESP);
    assert_eq!(split_status(f.payload).unwrap().0, Status::Unsupported);
}
