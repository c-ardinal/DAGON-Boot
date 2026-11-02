//! Image header: 512 bytes, signed over bytes 0x000..0x1C0 with domain
//! `DAGON/IMG/1`. The payload is bound by `payload_sha256`.

use crate::le::{arr, put_u16, put_u32, u16_at, u32_at};
use crate::{sha256, verify_signed, FormatError, Version};

pub const HEADER_SIZE: usize = 512;
pub const MAGIC: [u8; 4] = *b"DGIM";
pub const HDR_VERSION: u16 = 1;
/// Bytes covered by the signature.
pub const SIGNED_LEN: usize = 0x1C0;
pub const SIGN_DOMAIN: &[u8] = b"DAGON/IMG/1";
const RESERVED_TAIL: core::ops::Range<usize> = 0x068..0x1C0;

pub mod image_kind {
    pub const APP: u8 = 0x01;
    pub const BOOTCORE: u8 = 0x02;
    pub const AGENT: u8 = 0x03;
    pub const STAGE0: u8 = 0x04;
}

pub mod port_kind {
    pub const SELF_BOOT: u8 = 0x01;
    pub const VENDOR_STAGE2: u8 = 0x02;
}

pub mod mcu_id {
    pub const RP2040: u16 = 0x0001;
    pub const STM32F401XE: u16 = 0x0002;
    pub const ESP32S3: u16 = 0x0003;
}

pub mod slot_binding {
    pub const A: u8 = 0x00;
    pub const B: u8 = 0x01;
    /// Runs from either slot (MMU-mapped).
    pub const ANY: u8 = 0xFF;
}

pub mod placement {
    /// First 512 bytes of the slot; payload at slot + 0x200.
    pub const INLINE: u8 = 0x00;
    /// Separate header sector (ESP `dagon` partition).
    pub const DETACHED: u8 = 0x01;
    /// Last 512 bytes of the region.
    pub const TRAILER: u8 = 0x02;
}

pub mod build_flags {
    pub const VERIFY_ENFORCED: u32 = 1 << 0;
    pub const DEBUG: u32 = 1 << 1;
    pub const SIM_ONLY: u32 = 1 << 2;
    pub const TEST_KEY: u32 = 1 << 3;
}

pub mod capabilities {
    pub const CONFIRM_BY_HUB: u32 = 1 << 0;
    pub const REBOOT_TO_UPDATE: u32 = 1 << 1;
    pub const ROM_ENTRY_COOP: u32 = 1 << 2;
    pub const SIG_VERIFIER: u32 = 1 << 3;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub image_kind: u8,
    pub port_kind: u8,
    pub mcu_id: u16,
    pub slot_binding: u8,
    pub placement: u8,
    pub load_addr: u32,
    pub payload_size: u32,
    pub version: Version,
    pub build_flags: u32,
    pub release_seq: u32,
    pub min_bootcore_version: Version,
    pub payload_sha256: [u8; 32],
    pub key_id: [u8; 8],
    /// ASCII, NUL padded.
    pub target_tag: [u8; 16],
    pub proto_version: u32,
    pub capabilities: u32,
    pub signature: [u8; 64],
}

impl ImageHeader {
    /// Parses the fixed fields. Does not verify the signature.
    pub fn parse(raw: &[u8; HEADER_SIZE]) -> Result<Self, FormatError> {
        if raw[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if u16_at(raw, 0x004) != HDR_VERSION {
            return Err(FormatError::UnsupportedVersion);
        }
        if u16_at(raw, 0x006) as usize != HEADER_SIZE
            || u16_at(raw, 0x00E) != 0
            || raw[RESERVED_TAIL].iter().any(|&b| b != 0)
        {
            return Err(FormatError::BadField);
        }
        Ok(ImageHeader {
            image_kind: raw[0x008],
            port_kind: raw[0x009],
            mcu_id: u16_at(raw, 0x00A),
            slot_binding: raw[0x00C],
            placement: raw[0x00D],
            load_addr: u32_at(raw, 0x010),
            payload_size: u32_at(raw, 0x014),
            version: Version::from_raw(u32_at(raw, 0x018)),
            build_flags: u32_at(raw, 0x01C),
            release_seq: u32_at(raw, 0x020),
            min_bootcore_version: Version::from_raw(u32_at(raw, 0x024)),
            payload_sha256: arr(raw, 0x028),
            key_id: arr(raw, 0x048),
            target_tag: arr(raw, 0x050),
            proto_version: u32_at(raw, 0x060),
            capabilities: u32_at(raw, 0x064),
            signature: arr(raw, 0x1C0),
        })
    }

    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut b = [0u8; HEADER_SIZE];
        b[0..4].copy_from_slice(&MAGIC);
        put_u16(&mut b, 0x004, HDR_VERSION);
        put_u16(&mut b, 0x006, HEADER_SIZE as u16);
        b[0x008] = self.image_kind;
        b[0x009] = self.port_kind;
        put_u16(&mut b, 0x00A, self.mcu_id);
        b[0x00C] = self.slot_binding;
        b[0x00D] = self.placement;
        put_u32(&mut b, 0x010, self.load_addr);
        put_u32(&mut b, 0x014, self.payload_size);
        put_u32(&mut b, 0x018, self.version.raw());
        put_u32(&mut b, 0x01C, self.build_flags);
        put_u32(&mut b, 0x020, self.release_seq);
        put_u32(&mut b, 0x024, self.min_bootcore_version.raw());
        b[0x028..0x048].copy_from_slice(&self.payload_sha256);
        b[0x048..0x050].copy_from_slice(&self.key_id);
        b[0x050..0x060].copy_from_slice(&self.target_tag);
        put_u32(&mut b, 0x060, self.proto_version);
        put_u32(&mut b, 0x064, self.capabilities);
        b[0x1C0..0x200].copy_from_slice(&self.signature);
        b
    }
}

/// Checks `key_id` against `pubkey`, then the Ed25519 signature, then parses.
/// Errors: `UnknownKey`, `BadSignature`, or a parse error.
pub fn verify_header(
    raw: &[u8; HEADER_SIZE],
    pubkey: &[u8; 32],
) -> Result<ImageHeader, FormatError> {
    if raw[0x048..0x050] != crate::key_id(pubkey) {
        return Err(FormatError::UnknownKey);
    }
    let sig: [u8; 64] = arr(raw, SIGNED_LEN);
    verify_signed(pubkey, SIGN_DOMAIN, &raw[..SIGNED_LEN], &sig)?;
    ImageHeader::parse(raw)
}

/// SHA-256 of the full signed 512-byte header (manifest `header_sha256`).
pub fn header_digest(raw: &[u8; HEADER_SIZE]) -> [u8; 32] {
    sha256(raw)
}

/// First 16 bytes of [`header_digest`] (state log and STATUS `digest16`).
pub fn header_digest16(raw: &[u8; HEADER_SIZE]) -> [u8; 16] {
    arr(&header_digest(raw), 0)
}

/// `target_tag` from an ASCII string (truncated to 16 bytes, NUL padded).
pub const fn target_tag(s: &str) -> [u8; 16] {
    let src = s.as_bytes();
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < src.len() && i < 16 {
        out[i] = src[i];
        i += 1;
    }
    out
}
