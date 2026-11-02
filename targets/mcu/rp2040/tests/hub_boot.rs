//! The hub layout through dagon-core: install, update to B, three
//! unconfirmed trials, rollback to A; STATUS as the hub app reports it.

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::{Log, SlotRef};
use dagon_format::header::*;
use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
use dagon_format::{key_id, sha256, Version};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::SlotState;
use ed25519_compact::{KeyPair, Seed};
use dagon_mcu_rp2040::layout::*;
use dagon_mcu_rp2040::{image, status};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

struct Mem(Vec<u8>);

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let a = (addr - XIP_BASE) as usize;
        buf.copy_from_slice(self.0.get(a..a + buf.len()).ok_or(FlashError::Range)?);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let a = (addr - XIP_BASE) as usize;
        self.0[a..a + len as usize].fill(0xFF);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let a = (addr - XIP_BASE) as usize;
        self.0[a..a + data.len()].copy_from_slice(data);
        Ok(())
    }
}

/// Writes a signed app image into `slot`; returns its reference.
fn put_app(m: &mut Mem, kp: &KeyPair, slot: u8, minor: u8) -> SlotRef {
    let base = APP_SLOT[slot as usize];
    let body = vec![minor; 1024];
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::RP2040,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: base + HEADER_LEN,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: 0,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: target_tag(HUB_TAG),
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    let raw = h.to_bytes();
    m.write(base, &raw).unwrap();
    m.write(base + HEADER_LEN, &body).unwrap();
    image::ref_of(&raw, slot).unwrap()
}

fn status_now(m: &mut Mem, active: u8) -> dagon_proto::payload::StatusResp {
    let log = Log::load(m, &APP_LOG_LAYOUT).unwrap();
    let heads = [0u8, 1].map(|s| image::head_ref(m, APP_SLOT[s as usize], s));
    status::status(&log.state, heads, active, log.generation)
}

/// Flash with app A installed and the install log as `dagon rescue hub`
/// writes it.
fn install(kp: &KeyPair) -> (Mem, SlotRef) {
    let mut m = Mem(vec![
        0xFF;
        (APP_SLOT[1] + APP_SLOT_SIZE - XIP_BASE) as usize
    ]);
    let a = put_app(&mut m, kp, 0, 0);
    let rec = |k, seq| Record {
        kind: k,
        target: target::APP,
        slot: 0,
        reason: 0,
        seq,
        version: a.version,
        digest16: a.digest16,
    };
    m.write(APP_LOG[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    m.write(APP_LOG[0] + 32, &rec(kind::PROVISION, 1).to_bytes())
        .unwrap();
    m.write(APP_LOG[0] + 64, &rec(kind::CONFIRMED, 2).to_bytes())
        .unwrap();
    (m, a)
}

fn config(kp: &KeyPair) -> BootConfig {
    BootConfig {
        pubkey: *kp.pk,
        mcu_id: mcu_id::RP2040,
        target_tag: target_tag(HUB_TAG),
        max_attempts: MAX_ATTEMPTS,
    }
}

/// Many updates back and forth: the 4 KiB faces (127 records each) compact
/// into each other and the state stays right.
#[test]
fn hundreds_of_updates_compact_the_log() {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let (mut m, a) = install(&kp);
    let b = put_app(&mut m, &kp, 1, 1);
    for i in 0..200 {
        let to = if i % 2 == 0 { b } else { a };
        let mut log = Log::load(&mut m, &APP_LOG_LAYOUT).unwrap();
        log.append(&mut m, kind::SLOT_PENDING, to, 0).unwrap();
        let d = boot::decide(&mut m, &mut log, &APP_SLOTS, &config(&kp));
        assert!(
            matches!(d, Decision::Boot { trial: true, .. }),
            "{i}: {d:?}"
        );
        log.append(&mut m, kind::CONFIRMED, to, 0).unwrap();
        let st = status_now(&mut m, to.slot);
        assert_eq!(st.confirmed, to.slot, "{i}");
        assert_eq!(
            st.slots[to.slot as usize].state,
            SlotState::Confirmed,
            "{i}"
        );
    }
    let mut log = Log::load(&mut m, &APP_LOG_LAYOUT).unwrap();
    assert!(log.generation > 1, "600 records must have compacted");
    let d = boot::decide(&mut m, &mut log, &APP_SLOTS, &config(&kp));
    assert!(
        matches!(
            d,
            Decision::Boot {
                slot: 0,
                trial: false,
                ..
            }
        ),
        "{d:?}"
    );
}

#[test]
fn trial_without_confirm_rolls_back_to_a() {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let cfg = config(&kp);
    let (mut m, _a) = install(&kp);

    let b = put_app(&mut m, &kp, 1, 1);
    let mut log = Log::load(&mut m, &APP_LOG_LAYOUT).unwrap();
    log.append(&mut m, kind::SLOT_PENDING, b, 0).unwrap();
    assert_eq!(status_now(&mut m, 0).slots[1].state, SlotState::Pending);

    for _ in 0..MAX_ATTEMPTS {
        let mut log = Log::load(&mut m, &APP_LOG_LAYOUT).unwrap();
        let d = boot::decide(&mut m, &mut log, &APP_SLOTS, &cfg);
        assert!(
            matches!(
                d,
                Decision::Boot {
                    slot: 1,
                    trial: true,
                    ..
                }
            ),
            "{d:?}"
        );
        assert_eq!(status_now(&mut m, 1).slots[1].state, SlotState::Trying);
    }
    let mut log = Log::load(&mut m, &APP_LOG_LAYOUT).unwrap();
    let d = boot::decide(&mut m, &mut log, &APP_SLOTS, &cfg);
    assert!(
        matches!(
            d,
            Decision::Boot {
                slot: 0,
                trial: false,
                ..
            }
        ),
        "{d:?}"
    );

    // The ROLLBACK record names the destination slot (A).
    let last = APP_LOG[0] + 32 * (1 + 2 + 1 + MAX_ATTEMPTS as u32);
    let mut raw = [0u8; 32];
    m.read(last, &mut raw).unwrap();
    let r = Record::parse(&raw).unwrap();
    assert_eq!(
        (r.kind, r.slot, r.reason),
        (kind::ROLLBACK, 0, reason::TRIAL_EXHAUSTED)
    );

    let st = status_now(&mut m, 0);
    assert_eq!(
        (st.slots[0].state, st.slots[1].state),
        (SlotState::Confirmed, SlotState::Reverted)
    );
    assert_eq!((st.confirmed, st.last_reason), (0, reason::TRIAL_EXHAUSTED));
}
