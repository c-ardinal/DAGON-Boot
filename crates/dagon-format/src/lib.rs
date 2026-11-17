//! Byte layouts shared by every DAGON component: image header, manifest,
//! release bundle (`.dgb`), state log, topology canonical binary, plus
//! CRC-32, SHA-256 and the signature verifier interface ([`SigVerifier`]).
//! All multi-byte fields are little-endian.
#![no_std]
#![forbid(unsafe_code)]

pub mod bundle;
pub mod header;
pub mod le;
pub mod link;
pub mod manifest;
pub mod stage0;
pub mod statelog;
pub mod topology;
pub mod version;

pub use bundle::{BundleItem, BundleView};
pub use header::ImageHeader;
pub use manifest::{Constraint, ManifestEntry, ManifestFields, ManifestView};
pub use statelog::{FaceHeader, Record};
pub use topology::{TopoEnd, TopoLink, TopoNode, TopologyView};
pub use version::Version;

/// Errors from parsing or verifying a structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// Input shorter than the structure (or than a length field claims).
    Truncated,
    BadMagic,
    /// Known magic, unknown format version.
    UnsupportedVersion,
    /// A field holds a value outside its allowed set or range.
    BadField,
    BadCrc,
    /// `key_id` does not match the verifying key.
    UnknownKey,
    BadSignature,
    /// A SHA-256 binding (payload, header or topology) does not match.
    DigestMismatch,
}

/// CRC-32 (IEEE 802.3: reflected polynomial 0xEDB88320, init and final XOR 0xFFFFFFFF).
///
/// Four bits per table step: a 64-byte table instead of 1 KiB in every
/// image. On a node it checks state log records and frames, a few hundred
/// bytes at most, where the byte-wide table's speed is not needed.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        crc = CRC_TABLE[(crc & 0xF) as usize] ^ (crc >> 4);
        crc = CRC_TABLE[(crc & 0xF) as usize] ^ (crc >> 4);
    }
    crc ^ 0xFFFF_FFFF
}

/// The CRC of each 4-bit value, shifted through 4 bits.
const CRC_TABLE: [u32; 16] = {
    let mut table = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 4 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Incremental SHA-256.
#[derive(Clone, Default)]
pub struct Sha256(sha2::Sha256);

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        sha2::Digest::update(&mut self.0, data);
    }

    pub fn finalize(self) -> [u8; 32] {
        sha2::Digest::finalize(self.0).into()
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

/// Key ID: first 8 bytes of SHA-256 of the 32-byte Ed25519 public key.
pub fn key_id(pubkey: &[u8; 32]) -> [u8; 8] {
    le::arr(&sha256(pubkey), 0)
}

/// Checks the signatures of DAGON's signed structures. The algorithm and
/// the key it checks under belong to the implementation; the image header
/// format fixes a 64-byte signature and an 8-byte key id.
pub trait SigVerifier {
    /// The `key_id` a structure signed under this verifier's key carries.
    fn key_id(&self) -> [u8; 8];

    /// Whether `sig` signs `domain` followed by every part of `msg`, in order.
    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool;
}

/// Checks that `signature` signs `domain` || `message` under `verifier`.
pub fn verify_signed<V: SigVerifier + ?Sized>(
    verifier: &V,
    domain: &[u8],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), FormatError> {
    if verifier.verify(domain, &[message], signature) {
        Ok(())
    } else {
        Err(FormatError::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// The same CRC computed one bit at a time, the definition.
    fn crc32_bitwise(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    0xEDB8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
        }
        crc ^ 0xFFFF_FFFF
    }

    #[test]
    fn crc32_matches_the_bitwise_definition_and_uses_a_16_entry_table() {
        // Known answers beyond the check value (IEEE 802.3 CRC-32).
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
        // Every byte value at every position of a run, and every length.
        let data: [u8; 512] = core::array::from_fn(|i| if i < 256 { i as u8 } else { !(i as u8) });
        for n in 0..data.len() {
            assert_eq!(crc32(&data[..n]), crc32_bitwise(&data[..n]), "{n} bytes");
        }
        assert_eq!(core::mem::size_of_val(&CRC_TABLE), 16 * 4);
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256(b"abc")[..4],
            [0xba, 0x78, 0x16, 0xbf],
            "FIPS 180-2 'abc' vector"
        );
    }
}
