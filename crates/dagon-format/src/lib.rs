//! Byte layouts shared by every DAGON component: image header, manifest,
//! release bundle (`.dgb`), state log, topology canonical binary, plus
//! CRC-32, SHA-256 and Ed25519 verification. All multi-byte fields are
//! little-endian.
#![no_std]
#![forbid(unsafe_code)]

pub mod bundle;
pub mod header;
pub mod le;
pub mod manifest;
pub mod statelog;
pub mod topology;
pub mod version;

pub use bundle::{BundleItem, BundleView};
pub use header::ImageHeader;
pub use manifest::{Constraint, ManifestEntry, ManifestFields, ManifestView};
pub use statelog::{FaceHeader, Record};
pub use topology::{TopoLink, TopoNode, TopologyView};
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
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
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

/// Verifies `signature` = Ed25519(`domain` || `message`) under `pubkey`.
pub fn verify_signed(
    pubkey: &[u8; 32],
    domain: &[u8],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), FormatError> {
    let pk = ed25519_compact::PublicKey::new(*pubkey);
    let sig = ed25519_compact::Signature::new(*signature);
    let mut st = pk
        .verify_incremental(&sig)
        .map_err(|_| FormatError::BadSignature)?;
    st.absorb(domain);
    st.absorb(message);
    st.verify().map_err(|_| FormatError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
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
