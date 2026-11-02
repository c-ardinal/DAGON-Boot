//! State log: two ping-pong faces, each a 32-byte face header followed by
//! append-only 32-byte records. Erased bytes are 0xFF; an all-0xFF record ends
//! the scan.

use crate::le::{arr, put_u16, put_u32, u16_at, u32_at};
use crate::{crc32, FormatError, Version};

pub const FACE_HEADER_SIZE: usize = 32;
pub const RECORD_SIZE: usize = 32;
pub const MAGIC: [u8; 4] = *b"DGSL";
pub const FORMAT_VERSION: u16 = 1;

/// Record `kind` values written by this release.
pub mod kind {
    pub const EMPTY: u8 = 0xFF;
    pub const SLOT_PENDING: u8 = 0x01;
    pub const BOOT_ATTEMPT: u8 = 0x02;
    pub const CONFIRMED: u8 = 0x03;
    pub const ROLLBACK: u8 = 0x04;
    pub const PROVISION: u8 = 0x08;
    pub const SNAPSHOT: u8 = 0x0A;
}

/// Record `reason` values written by this release (0 = none).
pub mod reason {
    pub const NONE: u8 = 0x00;
    pub const HASH_MISMATCH: u8 = 0x01;
    pub const SIG_INVALID: u8 = 0x02;
    pub const TRIAL_EXHAUSTED: u8 = 0x03;
    pub const NO_VALID_IMAGE: u8 = 0x05;
}

/// Record `target` values.
pub mod target {
    pub const APP: u8 = 0;
    pub const BOOTCORE: u8 = 1;
    pub const AGENT: u8 = 2;
}

/// Slot numbers used by the state log, STATUS and SET_PENDING.
pub mod slot {
    pub const A: u8 = 0;
    pub const B: u8 = 1;
    pub const FACTORY: u8 = 2;
    /// "No slot" in STATUS `active` / `confirmed`.
    pub const NONE: u8 = 0xFF;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceHeader {
    /// Larger is newer.
    pub generation: u32,
}

impl FaceHeader {
    pub fn parse(b: &[u8; FACE_HEADER_SIZE]) -> Result<Self, FormatError> {
        if crc32(&b[..28]) != u32_at(b, 28) {
            return Err(FormatError::BadCrc);
        }
        if b[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if u16_at(b, 4) != FORMAT_VERSION || u16_at(b, 6) as usize != RECORD_SIZE {
            return Err(FormatError::UnsupportedVersion);
        }
        Ok(FaceHeader {
            generation: u32_at(b, 8),
        })
    }

    pub fn to_bytes(&self) -> [u8; FACE_HEADER_SIZE] {
        let mut b = [0xFFu8; FACE_HEADER_SIZE];
        b[0..4].copy_from_slice(&MAGIC);
        put_u16(&mut b, 4, FORMAT_VERSION);
        put_u16(&mut b, 6, RECORD_SIZE as u16);
        put_u32(&mut b, 8, self.generation);
        let crc = crc32(&b[..28]);
        put_u32(&mut b, 28, crc);
        b
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub target: u8,
    pub slot: u8,
    pub reason: u8,
    pub seq: u32,
    pub version: Version,
    /// First 16 bytes of the SHA-256 of the image header the record refers to.
    pub digest16: [u8; 16],
}

impl Record {
    /// True for an erased (all 0xFF) record slot: end of the log.
    pub fn is_erased(b: &[u8; RECORD_SIZE]) -> bool {
        b.iter().all(|&x| x == 0xFF)
    }

    /// Fails with `BadCrc` on a torn or corrupt record. Callers check
    /// [`Record::is_erased`] first.
    pub fn parse(b: &[u8; RECORD_SIZE]) -> Result<Self, FormatError> {
        if crc32(&b[..28]) != u32_at(b, 28) {
            return Err(FormatError::BadCrc);
        }
        Ok(Record {
            kind: b[0],
            target: b[1],
            slot: b[2],
            reason: b[3],
            seq: u32_at(b, 4),
            version: Version::from_raw(u32_at(b, 8)),
            digest16: arr(b, 12),
        })
    }

    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut b = [0u8; RECORD_SIZE];
        b[0] = self.kind;
        b[1] = self.target;
        b[2] = self.slot;
        b[3] = self.reason;
        put_u32(&mut b, 4, self.seq);
        put_u32(&mut b, 8, self.version.raw());
        b[12..28].copy_from_slice(&self.digest16);
        let crc = crc32(&b[..28]);
        put_u32(&mut b, 28, crc);
        b
    }
}
