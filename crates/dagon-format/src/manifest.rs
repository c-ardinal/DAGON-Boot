//! Release manifest: 0x50-byte head, N 64-byte entries, M 16-byte
//! constraints, 64-byte signature = Ed25519(`DAGON/MAN/1` || everything before it).

use crate::le::{arr, put_u16, put_u32, u16_at, u32_at};
use crate::{verify_signed, FormatError, Version};

pub const MAGIC: [u8; 4] = *b"DGMF";
pub const MF_VERSION: u16 = 1;
pub const SIGN_DOMAIN: &[u8] = b"DAGON/MAN/1";
pub const HEAD_SIZE: usize = 0x50;
pub const ENTRY_SIZE: usize = 64;
pub const CONSTRAINT_SIZE: usize = 16;
pub const SIG_SIZE: usize = 64;
pub const MAX_ENTRIES: usize = 16;
pub const MAX_CONSTRAINTS: usize = 16;

pub mod entry_flags {
    pub const REQUIRED: u8 = 1 << 0;
    pub const ALLOW_DOWNGRADE: u8 = 1 << 1;
}

pub mod constraint_kind {
    pub const LOCKSTEP_PROTO: u8 = 0x01;
    pub const REQUIRES: u8 = 0x02;
    pub const MIN_BOOTCORE: u8 = 0x03;
}

pub const fn total_size(entries: usize, constraints: usize) -> usize {
    HEAD_SIZE + ENTRY_SIZE * entries + CONSTRAINT_SIZE * constraints + SIG_SIZE
}

/// Head fields other than magic, version, counts and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestFields {
    pub release_seq: u32,
    /// SHA-256 of the topology canonical binary.
    pub topology_sha256: [u8; 32],
    /// ASCII, NUL padded.
    pub release_name: [u8; 16],
    pub key_id: [u8; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    pub node_index: u8,
    pub image_kind: u8,
    pub slot_binding: u8,
    pub flags: u8,
    pub version: Version,
    pub payload_size: u32,
    pub proto_version: u32,
    /// SHA-256 of the signed 512-byte image header.
    pub header_sha256: [u8; 32],
    pub order_rank: u8,
}

impl ManifestEntry {
    pub fn parse(b: &[u8]) -> Self {
        ManifestEntry {
            node_index: b[0],
            image_kind: b[1],
            slot_binding: b[2],
            flags: b[3],
            version: Version::from_raw(u32_at(b, 4)),
            payload_size: u32_at(b, 8),
            proto_version: u32_at(b, 12),
            header_sha256: arr(b, 16),
            order_rank: b[48],
        }
    }

    pub fn to_bytes(&self) -> [u8; ENTRY_SIZE] {
        let mut b = [0u8; ENTRY_SIZE];
        b[0] = self.node_index;
        b[1] = self.image_kind;
        b[2] = self.slot_binding;
        b[3] = self.flags;
        put_u32(&mut b, 4, self.version.raw());
        put_u32(&mut b, 8, self.payload_size);
        put_u32(&mut b, 12, self.proto_version);
        b[16..48].copy_from_slice(&self.header_sha256);
        b[48] = self.order_rank;
        b
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Constraint {
    pub kind: u8,
    pub node_a: u8,
    pub node_b: u8,
    pub node_mask: u32,
    pub min_version: Version,
}

impl Constraint {
    pub fn parse(b: &[u8]) -> Self {
        Constraint {
            kind: b[0],
            node_a: b[1],
            node_b: b[2],
            node_mask: u32_at(b, 4),
            min_version: Version::from_raw(u32_at(b, 8)),
        }
    }

    pub fn to_bytes(&self) -> [u8; CONSTRAINT_SIZE] {
        let mut b = [0u8; CONSTRAINT_SIZE];
        b[0] = self.kind;
        b[1] = self.node_a;
        b[2] = self.node_b;
        put_u32(&mut b, 4, self.node_mask);
        put_u32(&mut b, 8, self.min_version.raw());
        b
    }
}

/// Validated view over a manifest's bytes.
#[derive(Clone, Copy, Debug)]
pub struct ManifestView<'a> {
    raw: &'a [u8],
}

impl<'a> ManifestView<'a> {
    /// Checks magic, version, counts and `total_size`; `raw` may be longer
    /// than the manifest. Does not verify the signature.
    pub fn parse(raw: &'a [u8]) -> Result<Self, FormatError> {
        if raw.len() < HEAD_SIZE {
            return Err(FormatError::Truncated);
        }
        if raw[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if u16_at(raw, 4) != MF_VERSION {
            return Err(FormatError::UnsupportedVersion);
        }
        let (n, m) = (raw[6] as usize, raw[7] as usize);
        if !(1..=MAX_ENTRIES).contains(&n) || m > MAX_CONSTRAINTS {
            return Err(FormatError::BadField);
        }
        let total = total_size(n, m);
        if u32_at(raw, 0x0C) as usize != total {
            return Err(FormatError::BadField);
        }
        if raw.len() < total {
            return Err(FormatError::Truncated);
        }
        Ok(ManifestView { raw: &raw[..total] })
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.raw
    }

    pub fn entry_count(&self) -> usize {
        self.raw[6] as usize
    }

    pub fn constraint_count(&self) -> usize {
        self.raw[7] as usize
    }

    pub fn fields(&self) -> ManifestFields {
        ManifestFields {
            release_seq: u32_at(self.raw, 0x08),
            topology_sha256: arr(self.raw, 0x10),
            release_name: arr(self.raw, 0x30),
            key_id: arr(self.raw, 0x40),
        }
    }

    /// Panics if `i >= entry_count()`.
    pub fn entry(&self, i: usize) -> ManifestEntry {
        assert!(i < self.entry_count());
        let off = HEAD_SIZE + ENTRY_SIZE * i;
        ManifestEntry::parse(&self.raw[off..off + ENTRY_SIZE])
    }

    /// Panics if `i >= constraint_count()`.
    pub fn constraint(&self, i: usize) -> Constraint {
        assert!(i < self.constraint_count());
        let off = HEAD_SIZE + ENTRY_SIZE * self.entry_count() + CONSTRAINT_SIZE * i;
        Constraint::parse(&self.raw[off..off + CONSTRAINT_SIZE])
    }

    pub fn verify(&self, pubkey: &[u8; 32]) -> Result<(), FormatError> {
        if self.raw[0x40..0x48] != crate::key_id(pubkey) {
            return Err(FormatError::UnknownKey);
        }
        let body = self.raw.len() - SIG_SIZE;
        let sig: [u8; 64] = arr(self.raw, body);
        verify_signed(pubkey, SIGN_DOMAIN, &self.raw[..body], &sig)
    }
}

/// Writes an unsigned manifest (signature bytes zero) into `out` and returns
/// its length. The signer fills the last 64 bytes.
pub fn encode(
    fields: &ManifestFields,
    entries: &[ManifestEntry],
    constraints: &[Constraint],
    out: &mut [u8],
) -> Result<usize, FormatError> {
    let (n, m) = (entries.len(), constraints.len());
    if !(1..=MAX_ENTRIES).contains(&n) || m > MAX_CONSTRAINTS {
        return Err(FormatError::BadField);
    }
    let total = total_size(n, m);
    let out = out.get_mut(..total).ok_or(FormatError::Truncated)?;
    out.fill(0);
    out[0..4].copy_from_slice(&MAGIC);
    put_u16(out, 4, MF_VERSION);
    out[6] = n as u8;
    out[7] = m as u8;
    put_u32(out, 0x08, fields.release_seq);
    put_u32(out, 0x0C, total as u32);
    out[0x10..0x30].copy_from_slice(&fields.topology_sha256);
    out[0x30..0x40].copy_from_slice(&fields.release_name);
    out[0x40..0x48].copy_from_slice(&fields.key_id);
    for (i, e) in entries.iter().enumerate() {
        let off = HEAD_SIZE + ENTRY_SIZE * i;
        out[off..off + ENTRY_SIZE].copy_from_slice(&e.to_bytes());
    }
    for (i, c) in constraints.iter().enumerate() {
        let off = HEAD_SIZE + ENTRY_SIZE * n + CONSTRAINT_SIZE * i;
        out[off..off + CONSTRAINT_SIZE].copy_from_slice(&c.to_bytes());
    }
    Ok(total)
}
