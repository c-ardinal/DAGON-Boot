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
    /// The app asks its boot core to stay in update mode at the next
    /// start (REBOOT mode 1). Node-wide: `target`, `reason`, `version`
    /// and `digest16` are 0 and `slot` is `slot::NONE`. Used once: the
    /// boot core consumes it with [`UPDATE_DONE`] as it enters update
    /// mode. A reader that predates it skips it, as any unknown kind.
    pub const UPDATE_STAY: u8 = 0x06;
    pub const PROVISION: u8 = 0x08;
    /// The boot core consumed the [`UPDATE_STAY`] before it (fields as
    /// UPDATE_STAY's).
    pub const UPDATE_DONE: u8 = 0x09;
    pub const SNAPSHOT: u8 = 0x0A;
    /// The log has kept every [`PEER_SEEN`] since the first write.
    /// Node-wide (fields as UPDATE_STAY's). Only the log a first write
    /// makes holds it: no node appends it, a compaction copies it, and a
    /// log rebuilt or made by an older release has none, so records that
    /// were lost are never taken for a peer that never answered.
    pub const PEERS_KEPT: u8 = 0x0B;
    /// A node this one relays to answered with a valid frame at least
    /// once. Node-wide; `digest16[0]` is the relay link's id and
    /// `digest16[1]` the node's address on it ([`super::peer`]). One for
    /// each peer.
    pub const PEER_SEEN: u8 = 0x0C;
}

/// The peer records of a node that relays: where a [`kind::PEER_SEEN`]
/// names its peer in `digest16`, and how many a log holds.
pub mod peer {
    pub const LINK: usize = 0;
    pub const ADDR: usize = 1;
    /// The most [`super::kind::PEER_SEEN`] records a log holds.
    pub const MAX: usize = 16;

    /// Records a face of a relaying node's app log must hold, so a
    /// compaction with every peer recorded still leaves the room of a
    /// trial (`attempts`: the boot attempts a trial gets): the carried
    /// state of both targets (four records and the attempts each), the
    /// room kept for the trials (`2 * (attempts + 3)`), the stay and its
    /// consumption, PEERS_KEPT, every PEER_SEEN and the record being
    /// appended (`dagon_core::log` checks the sum against its own parts).
    pub const fn face_records(attempts: u8) -> u32 {
        4 * attempts as u32 + 18 + MAX as u32
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind keeps its number; the update-mode records take the
    /// design's 0x06 and 0x09 and round trip like any record.
    #[test]
    fn record_kinds_are_distinct_and_round_trip() {
        let kinds = [
            kind::SLOT_PENDING,
            kind::BOOT_ATTEMPT,
            kind::CONFIRMED,
            kind::ROLLBACK,
            kind::UPDATE_STAY,
            kind::PROVISION,
            kind::UPDATE_DONE,
            kind::SNAPSHOT,
            kind::PEERS_KEPT,
            kind::PEER_SEEN,
        ];
        assert_eq!(
            kinds,
            [0x01, 0x02, 0x03, 0x04, 0x06, 0x08, 0x09, 0x0A, 0x0B, 0x0C]
        );
        for k in kinds {
            let r = Record {
                kind: k,
                target: target::APP,
                slot: slot::NONE,
                reason: reason::NONE,
                seq: 7,
                version: Version::default(),
                digest16: [0; 16],
            };
            assert_eq!(Record::parse(&r.to_bytes()), Ok(r));
        }
    }
}
