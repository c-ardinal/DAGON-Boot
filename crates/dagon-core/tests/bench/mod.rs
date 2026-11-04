//! The in-memory flash with the F401 layout and the signed test images the
//! boot tests run on (shared by dagon-core's and dagon-boot's tests).
#![allow(dead_code, unused_imports)]

pub use dagon_core::boot::{decide, BootConfig, Decision, SlotLayout};
pub use dagon_core::log::{Log, LogError, LogLayout, SlotRef};
pub use dagon_format::header::*;
pub use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
pub use dagon_format::{key_id, sha256, Version};
pub use dagon_port::{Flash, FlashError, FlashRegion};
pub use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
pub const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
pub const BASE: u32 = 0x0800_0000;
pub const TAG: [u8; 16] = target_tag("seg-f401-7seg");
pub const LOG: LogLayout = LogLayout {
    faces: [0x0800_8000, 0x0800_C000],
    face_len: 0x4000,
};
pub const SLOTS: SlotLayout = SlotLayout {
    slots: [0x0802_0000, 0x0804_0000],
    slot_len: 0x2_0000,
};
pub const A: u8 = 0;
pub const B: u8 = 1;

pub struct Mem {
    pub bytes: Vec<u8>,
    pub writes: usize,
}

impl Mem {
    pub fn new() -> Self {
        Mem {
            bytes: vec![0xFF; 512 * 1024],
            writes: 0,
        }
    }
    pub fn range(&self, addr: u32, len: usize) -> Result<std::ops::Range<usize>, FlashError> {
        let start = addr.checked_sub(BASE).ok_or(FlashError::Range)? as usize;
        (start + len <= self.bytes.len())
            .then_some(start..start + len)
            .ok_or(FlashError::Range)
    }
    pub fn put(&mut self, addr: u32, data: &[u8]) {
        let r = self.range(addr, data.len()).unwrap();
        self.bytes[r].copy_from_slice(data);
    }
}

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let r = self.range(addr, buf.len())?;
        buf.copy_from_slice(&self.bytes[r]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let r = self.range(addr, len as usize)?;
        self.bytes[r].fill(0xFF);
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
        self.writes += 1;
        Ok(())
    }
}

pub fn cfg() -> BootConfig {
    BootConfig {
        pubkey: *KeyPair::from_seed(Seed::new(SEED)).pk,
        mcu_id: mcu_id::STM32F401XE,
        target_tag: TAG,
        max_attempts: 3,
    }
}

/// Signed image for `slot`; returns (header, body).
pub fn image(slot: u8, minor: u8) -> ([u8; HEADER_SIZE], Vec<u8>) {
    image_at(slot, minor, SLOTS.slots[slot as usize] + HEADER_SIZE as u32)
}

pub fn image_at(slot: u8, minor: u8, load_addr: u32) -> ([u8; HEADER_SIZE], Vec<u8>) {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let body: Vec<u8> = (0..2048u32).map(|i| (i ^ minor as u32) as u8).collect();
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr,
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
    (h.to_bytes(), body)
}

pub fn slot_ref(slot: u8, hdr: &[u8; HEADER_SIZE]) -> SlotRef {
    SlotRef {
        slot,
        version: ImageHeader::parse(hdr).unwrap().version,
        digest16: header_digest16(hdr),
    }
}

pub fn install(m: &mut Mem, slot: u8, (hdr, body): &([u8; HEADER_SIZE], Vec<u8>)) -> SlotRef {
    let base = SLOTS.slots[slot as usize];
    m.erase(base, SLOTS.slot_len).unwrap();
    m.put(base, hdr);
    m.put(base + HEADER_SIZE as u32, body);
    slot_ref(slot, hdr)
}

pub fn write_log(m: &mut Mem, face: usize, generation: u32, recs: &[(u8, SlotRef)]) {
    write_face(m, &LOG, face, generation, recs);
}

pub fn write_face(
    m: &mut Mem,
    at: &LogLayout,
    face: usize,
    generation: u32,
    recs: &[(u8, SlotRef)],
) {
    let mut out = FaceHeader { generation }.to_bytes().to_vec();
    for (i, (kind, r)) in recs.iter().enumerate() {
        let rec = Record {
            kind: *kind,
            target: target::APP,
            slot: r.slot,
            reason: reason::NONE,
            seq: i as u32 + 1,
            version: r.version,
            digest16: r.digest16,
        };
        out.extend_from_slice(&rec.to_bytes());
    }
    m.erase(at.faces[face], at.face_len).unwrap();
    m.put(at.faces[face], &out);
}

/// Device with A confirmed (provisioned) and B pending.
pub fn with_pending_b() -> Mem {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image(B, 1));
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, b),
        ],
    );
    m
}

pub fn boot(m: &mut Mem) -> Decision {
    let mut log = Log::load(m, &LOG).unwrap();
    decide(m, &mut log, &SLOTS, &cfg())
}

pub fn entry(slot: u8) -> u32 {
    SLOTS.slots[slot as usize] + HEADER_SIZE as u32
}

pub fn state(m: &mut Mem) -> dagon_core::log::LogState {
    Log::load(m, &LOG).unwrap().state
}
