//! Signed images: slot geometry per MCU, ELF to payload, header signing and
//! host-side verification.

use std::fmt;

use dagon_format::header::{
    self, build_flags, capabilities, image_kind, placement, slot_binding, ImageHeader, HEADER_SIZE,
    SIGNED_LEN,
};
use dagon_format::{sha256, Version};
use ed25519_compact::KeyPair;

pub use dagon_topology::{mcu_id_of, parse_version, port_kind_of};

/// RFC 8032 section 7.1 "TEST 1" public key. Images signed with it always
/// carry the TEST_KEY build flag, and only those do.
pub const TEST_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// Where an image lives in flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Absolute address of the 512-byte header (inline, trailer) or of the
    /// detached header sector.
    pub header_addr: u32,
    pub load_addr: u32,
    /// Largest payload that fits.
    pub max_payload: u32,
    pub placement: u8,
    pub slot_binding: u8,
}

const KIB: u32 = 1024;
const MIB: u32 = 1024 * KIB;

const fn inline(base: u32, size: u32, binding: u8) -> Slot {
    Slot {
        header_addr: base,
        load_addr: base + HEADER_SIZE as u32,
        max_payload: size - HEADER_SIZE as u32,
        placement: placement::INLINE,
        slot_binding: binding,
    }
}

const RP2040_XIP: u32 = 0x1000_0000;
const F401_FLASH: u32 = 0x0800_0000;
/// ESP32-S3 `dagon` partition holding the detached headers.
const ESP_DAGON_PART: u32 = 0x71_0000;

/// Slot for an image of `kind` in slot `slot` (`slot_binding::A` / `B`) on
/// `mcu` (topology MCU id). Single-slot images (F401 boot core, ESP images
/// that run from either partition) answer for slot A only.
pub fn slot(mcu: &str, kind: u8, slot: u8) -> Option<Slot> {
    use slot_binding::{A, B};
    let s = match (mcu, kind, slot) {
        ("rp2040", image_kind::BOOTCORE, A) => inline(RP2040_XIP + 0x8000, 128 * KIB, A),
        ("rp2040", image_kind::BOOTCORE, B) => inline(RP2040_XIP + 0x2_8000, 128 * KIB, B),
        ("rp2040", image_kind::APP, A) => inline(RP2040_XIP + 0x4_8000, MIB, A),
        ("rp2040", image_kind::APP, B) => inline(RP2040_XIP + 0x14_8000, MIB, B),
        ("stm32f401re", image_kind::BOOTCORE, A) => Slot {
            header_addr: F401_FLASH + 0x7E00,
            load_addr: F401_FLASH,
            max_payload: 0x7E00,
            placement: placement::TRAILER,
            slot_binding: A,
        },
        ("stm32f401re", image_kind::APP, A) => inline(F401_FLASH + 0x2_0000, 128 * KIB, A),
        ("stm32f401re", image_kind::APP, B) => inline(F401_FLASH + 0x4_0000, 128 * KIB, B),
        // ponytail: ESP images are MMU-mapped and run from either OTA
        // partition, so load_addr names ota_0; per-partition headers are
        // placed by the receiver.
        ("esp32s3", image_kind::APP, A) => Slot {
            header_addr: ESP_DAGON_PART,
            load_addr: 0x11_0000,
            max_payload: 3 * MIB,
            placement: placement::DETACHED,
            slot_binding: slot_binding::ANY,
        },
        ("esp32s3", image_kind::AGENT, A) => Slot {
            header_addr: ESP_DAGON_PART + 0x2000,
            load_addr: 0x1_0000,
            max_payload: MIB,
            placement: placement::DETACHED,
            slot_binding: slot_binding::ANY,
        },
        _ => return None,
    };
    Some(s)
}

/// Slot of an already-signed header (`slot_binding::ANY` maps to slot A).
pub fn slot_of(mcu: &str, h: &ImageHeader) -> Option<Slot> {
    let s = if h.slot_binding == slot_binding::ANY {
        slot_binding::A
    } else {
        h.slot_binding
    };
    slot(mcu, h.image_kind, s)
}

pub fn image_kind_of(name: &str) -> Option<u8> {
    match name {
        "app" => Some(image_kind::APP),
        "bootcore" => Some(image_kind::BOOTCORE),
        "agent" => Some(image_kind::AGENT),
        _ => None,
    }
}

/// Comma-separated flag names into a bit set.
pub fn parse_bits(list: &str, names: &[(&str, u32)]) -> Option<u32> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .try_fold(0u32, |acc, n| {
            names
                .iter()
                .find(|(name, _)| *name == n)
                .map(|(_, b)| acc | b)
        })
}

pub const BUILD_FLAG_NAMES: &[(&str, u32)] = &[
    ("verify-enforced", build_flags::VERIFY_ENFORCED),
    ("debug", build_flags::DEBUG),
    ("sim-only", build_flags::SIM_ONLY),
];

pub const CAPABILITY_NAMES: &[(&str, u32)] = &[
    ("confirm-by-hub", capabilities::CONFIRM_BY_HUB),
    ("reboot-to-update", capabilities::REBOOT_TO_UPDATE),
    ("rom-entry-coop", capabilities::ROM_ENTRY_COOP),
    ("sig-verifier", capabilities::SIG_VERIFIER),
];

/// Default `(build_flags, capabilities)` for an image kind.
pub fn default_bits(kind: u8) -> (u32, u32) {
    match kind {
        image_kind::BOOTCORE | image_kind::AGENT => {
            (build_flags::VERIFY_ENFORCED, capabilities::SIG_VERIFIER)
        }
        _ => (0, capabilities::CONFIRM_BY_HUB),
    }
}

/// A verifying image (boot core, agent) must enforce verification, carry a
/// signature verifier and not be a simulator-only build.
pub fn capabilities_ok(kind: u8, flags: u32, caps: u32) -> bool {
    match kind {
        image_kind::BOOTCORE | image_kind::AGENT => {
            flags & build_flags::VERIFY_ENFORCED != 0
                && caps & capabilities::SIG_VERIFIER != 0
                && flags & build_flags::SIM_ONLY == 0
        }
        _ => true,
    }
}

/// Everything in the header except what is derived from the payload and key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSpec {
    pub image_kind: u8,
    pub port_kind: u8,
    pub mcu_id: u16,
    pub slot: Slot,
    pub version: Version,
    /// TEST_KEY is managed by `sign_image` and ignored here.
    pub build_flags: u32,
    pub release_seq: u32,
    pub min_bootcore_version: Version,
    pub target_tag: [u8; 16],
    pub proto_version: u32,
    pub capabilities: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    EmptyPayload,
    TooLarge {
        size: usize,
        max: u32,
    },
    /// Boot core or agent without VERIFY_ENFORCED + SIG_VERIFIER, or SIM_ONLY.
    Capability,
    BadElf(&'static str),
    /// ELF load address differs from the slot's.
    LoadAddress {
        elf: u32,
        slot: u32,
    },
    Format(dagon_format::FormatError),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPayload => f.write_str("payload is empty"),
            Self::TooLarge { size, max } => {
                write!(f, "payload is {size} bytes, the slot holds {max}")
            }
            Self::Capability => f.write_str(
                "boot core and agent images need verify-enforced and sig-verifier and must not be sim-only",
            ),
            Self::BadElf(why) => write!(f, "not a usable ELF: {why}"),
            Self::LoadAddress { elf, slot } => write!(
                f,
                "ELF is linked at {elf:#010x} but the slot payload starts at {slot:#010x}"
            ),
            Self::Format(e) => write!(f, "image check failed: {e:?}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// Ed25519(`domain` || `message`) with `key`. Deterministic.
pub fn sign(key: &KeyPair, domain: &[u8], message: &[u8]) -> [u8; 64] {
    *key.sk.sign([domain, message].concat(), None)
}

/// Header + payload, header signed with `key`.
pub fn sign_image(spec: &ImageSpec, payload: &[u8], key: &KeyPair) -> Result<Vec<u8>, ImageError> {
    if payload.is_empty() {
        return Err(ImageError::EmptyPayload);
    }
    if payload.len() > spec.slot.max_payload as usize {
        return Err(ImageError::TooLarge {
            size: payload.len(),
            max: spec.slot.max_payload,
        });
    }
    if !capabilities_ok(spec.image_kind, spec.build_flags, spec.capabilities) {
        return Err(ImageError::Capability);
    }
    let pk: [u8; 32] = *key.pk;
    let test_key = if pk == TEST_PUBLIC_KEY {
        build_flags::TEST_KEY
    } else {
        0
    };
    let mut h = ImageHeader {
        image_kind: spec.image_kind,
        port_kind: spec.port_kind,
        mcu_id: spec.mcu_id,
        slot_binding: spec.slot.slot_binding,
        placement: spec.slot.placement,
        load_addr: spec.slot.load_addr,
        payload_size: payload.len() as u32,
        version: spec.version,
        build_flags: (spec.build_flags & !build_flags::TEST_KEY) | test_key,
        release_seq: spec.release_seq,
        min_bootcore_version: spec.min_bootcore_version,
        payload_sha256: sha256(payload),
        key_id: dagon_format::key_id(&pk),
        target_tag: spec.target_tag,
        proto_version: spec.proto_version,
        capabilities: spec.capabilities,
        signature: [0; 64],
    };
    h.signature = sign(key, header::SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]);
    Ok([&h.to_bytes()[..], payload].concat())
}

/// Header signature and key ID, payload length and SHA-256.
pub fn verify_image(image: &[u8], pubkey: &[u8; 32]) -> Result<ImageHeader, ImageError> {
    let raw: &[u8; HEADER_SIZE] = image
        .get(..HEADER_SIZE)
        .and_then(|h| h.try_into().ok())
        .ok_or(ImageError::Format(dagon_format::FormatError::Truncated))?;
    let h = header::verify_header(raw, pubkey).map_err(ImageError::Format)?;
    let payload = &image[HEADER_SIZE..];
    if payload.len() != h.payload_size as usize {
        return Err(ImageError::Format(dagon_format::FormatError::Truncated));
    }
    if sha256(payload) != h.payload_sha256 {
        return Err(ImageError::Format(
            dagon_format::FormatError::DigestMismatch,
        ));
    }
    Ok(h)
}

const ELF_MAGIC: &[u8] = b"\x7fELF";
const PT_LOAD: u32 = 1;

/// Payload bytes for a slot whose payload starts at `load_addr`. ELF input
/// (32-bit little-endian, as for Cortex-M and Xtensa) is flattened from its
/// loadable segments by load (physical) address, gaps filled with 0xFF; the
/// lowest address must equal `load_addr`. Any other input is taken as a raw
/// binary already laid out for `load_addr`.
pub fn payload_from_file(bytes: &[u8], load_addr: u32) -> Result<Vec<u8>, ImageError> {
    if !bytes.starts_with(ELF_MAGIC) {
        return Ok(bytes.to_vec());
    }
    let segs = elf_load_segments(bytes)?;
    let base = segs
        .iter()
        .map(|s| s.0)
        .min()
        .ok_or(ImageError::BadElf("no loadable data"))?;
    if base != load_addr {
        return Err(ImageError::LoadAddress {
            elf: base,
            slot: load_addr,
        });
    }
    let end = segs
        .iter()
        .map(|s| s.0 as u64 + s.1.len() as u64)
        .max()
        .unwrap_or(0);
    let mut out = vec![0xFF; (end - base as u64) as usize];
    for (addr, data) in segs {
        let off = (addr - base) as usize;
        out[off..off + data.len()].copy_from_slice(data);
    }
    Ok(out)
}

fn elf_load_segments(b: &[u8]) -> Result<Vec<(u32, &[u8])>, ImageError> {
    let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    let u32_at = |o: usize| {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    if b.get(4) != Some(&1) || b.get(5) != Some(&1) {
        return Err(ImageError::BadElf(
            "only 32-bit little-endian ELF is supported",
        ));
    }
    let truncated = ImageError::BadElf("truncated");
    let phoff = u32_at(0x1C).ok_or(truncated.clone())? as usize;
    let phentsize = u16_at(0x2A).ok_or(truncated.clone())? as usize;
    let phnum = u16_at(0x2C).ok_or(truncated.clone())? as usize;
    let mut segs = Vec::new();
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        let (Some(kind), Some(off), Some(paddr), Some(filesz)) =
            (u32_at(p), u32_at(p + 4), u32_at(p + 12), u32_at(p + 16))
        else {
            return Err(truncated);
        };
        if kind != PT_LOAD || filesz == 0 {
            continue;
        }
        let data = b
            .get(off as usize..off as usize + filesz as usize)
            .ok_or(truncated.clone())?;
        segs.push((paddr, data));
    }
    Ok(segs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal ELF32 LE with the given (paddr, data) PT_LOAD segments.
    pub(crate) fn elf(segs: &[(u32, &[u8])]) -> Vec<u8> {
        let phoff = 52usize;
        let data_off = phoff + 32 * segs.len();
        let mut b = vec![0u8; data_off];
        b[..4].copy_from_slice(ELF_MAGIC);
        b[4] = 1;
        b[5] = 1;
        b[0x1C..0x20].copy_from_slice(&(phoff as u32).to_le_bytes());
        b[0x2A..0x2C].copy_from_slice(&32u16.to_le_bytes());
        b[0x2C..0x2E].copy_from_slice(&(segs.len() as u16).to_le_bytes());
        for (i, (addr, data)) in segs.iter().enumerate() {
            let p = phoff + 32 * i;
            let off = b.len() as u32;
            b[p..p + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
            b[p + 4..p + 8].copy_from_slice(&off.to_le_bytes());
            b[p + 12..p + 16].copy_from_slice(&addr.to_le_bytes());
            b[p + 16..p + 20].copy_from_slice(&(data.len() as u32).to_le_bytes());
            b.extend_from_slice(data);
        }
        b
    }

    #[test]
    fn elf_segments_flatten_by_load_address() {
        let e = elf(&[(0x0802_0200, &[1, 2]), (0x0802_0204, &[3])]);
        assert_eq!(
            payload_from_file(&e, 0x0802_0200).unwrap(),
            [1, 2, 0xFF, 0xFF, 3]
        );
        assert_eq!(
            payload_from_file(&e, 0x0804_0200),
            Err(ImageError::LoadAddress {
                elf: 0x0802_0200,
                slot: 0x0804_0200
            })
        );
        assert_eq!(payload_from_file(b"raw", 0).unwrap(), b"raw");
    }

    #[test]
    fn versions_and_bits_parse() {
        assert_eq!(parse_version("1.10.300"), Some(Version::new(1, 10, 300)));
        assert_eq!(parse_version("1.1"), None);
        assert_eq!(parse_version("1.1.0.0"), None);
        assert_eq!(
            parse_bits("verify-enforced, debug", BUILD_FLAG_NAMES),
            Some(build_flags::VERIFY_ENFORCED | build_flags::DEBUG)
        );
        assert_eq!(parse_bits("", BUILD_FLAG_NAMES), Some(0));
        assert_eq!(parse_bits("nope", BUILD_FLAG_NAMES), None);
    }

    #[test]
    fn verifying_images_need_both_bits() {
        let (f, c) = default_bits(image_kind::BOOTCORE);
        assert!(capabilities_ok(image_kind::BOOTCORE, f, c));
        assert!(!capabilities_ok(image_kind::BOOTCORE, 0, c));
        assert!(!capabilities_ok(
            image_kind::AGENT,
            f | build_flags::SIM_ONLY,
            c
        ));
        assert!(capabilities_ok(image_kind::APP, 0, 0));
    }
}
