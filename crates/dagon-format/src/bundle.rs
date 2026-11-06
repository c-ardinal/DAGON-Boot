//! Release bundle file (`.dgb`): 16-byte head, K 16-byte items, then the
//! manifest and item bytes at their offsets. The file itself is unsigned; it
//! is trusted through the manifest signature and the SHA-256 chain.

use crate::header::{self, HEADER_SIZE};
use crate::le::{arr, put_u16, put_u32, u16_at, u32_at};
use crate::manifest::ManifestView;
use crate::{crc32, sha256, FormatError};

pub const MAGIC: [u8; 4] = *b"DGBN";
pub const BN_VERSION: u16 = 1;
pub const HEAD_SIZE: usize = 16;
pub const ITEM_SIZE: usize = 16;
/// Every manifest entry and the topology.
pub const MAX_ITEMS: usize = crate::manifest::MAX_ENTRIES + 1;

pub mod dest {
    /// Image for the hub itself.
    pub const HUB_DIRECT: u8 = 1;
    /// Image relayed to a sub node.
    pub const SUB_STORE: u8 = 2;
    /// Topology canonical binary. `entry_index` is [`super::NO_ENTRY`]; its
    /// SHA-256 must equal the manifest `topology_sha256`.
    pub const TOPOLOGY: u8 = 3;
}

/// `entry_index` of items that are not manifest entries (topology).
pub const NO_ENTRY: u8 = 0xFF;

pub const fn head_size(items: usize) -> usize {
    HEAD_SIZE + ITEM_SIZE * items
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleItem {
    pub entry_index: u8,
    pub dest: u8,
    /// From file start, 4-byte aligned.
    pub offset: u32,
    /// Image items: 512-byte header + payload. Topology: canonical binary length.
    pub length: u32,
    /// CRC-32 of the item bytes.
    pub crc32: u32,
}

impl BundleItem {
    pub fn parse(b: &[u8]) -> Self {
        BundleItem {
            entry_index: b[0],
            dest: b[1],
            offset: u32_at(b, 4),
            length: u32_at(b, 8),
            crc32: u32_at(b, 12),
        }
    }

    pub fn to_bytes(&self) -> [u8; ITEM_SIZE] {
        let mut b = [0u8; ITEM_SIZE];
        b[0] = self.entry_index;
        b[1] = self.dest;
        put_u32(&mut b, 4, self.offset);
        put_u32(&mut b, 8, self.length);
        put_u32(&mut b, 12, self.crc32);
        b
    }
}

/// Writes the bundle head and item table into `out[..head_size(items.len())]`.
pub fn encode_head(
    items: &[BundleItem],
    manifest_off: u32,
    manifest_len: u32,
    out: &mut [u8],
) -> Result<usize, FormatError> {
    if !(1..=MAX_ITEMS).contains(&items.len()) {
        return Err(FormatError::BadField);
    }
    let size = head_size(items.len());
    let out = out.get_mut(..size).ok_or(FormatError::Truncated)?;
    out.fill(0);
    out[0..4].copy_from_slice(&MAGIC);
    put_u16(out, 4, BN_VERSION);
    out[6] = items.len() as u8;
    put_u32(out, 8, manifest_off);
    put_u32(out, 12, manifest_len);
    for (i, it) in items.iter().enumerate() {
        let off = HEAD_SIZE + ITEM_SIZE * i;
        out[off..off + ITEM_SIZE].copy_from_slice(&it.to_bytes());
    }
    Ok(size)
}

/// Structurally validated view over a whole `.dgb` file.
#[derive(Clone, Copy, Debug)]
pub struct BundleView<'a> {
    raw: &'a [u8],
}

impl<'a> BundleView<'a> {
    /// Checks head, alignment, bounds and non-overlap of all ranges, and that
    /// the manifest parses. Does not check CRCs, digests or signatures.
    pub fn parse(raw: &'a [u8]) -> Result<Self, FormatError> {
        if raw.len() < HEAD_SIZE {
            return Err(FormatError::Truncated);
        }
        if raw[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if u16_at(raw, 4) != BN_VERSION {
            return Err(FormatError::UnsupportedVersion);
        }
        let k = raw[6] as usize;
        if !(1..=MAX_ITEMS).contains(&k) || raw[7] != 0 {
            return Err(FormatError::BadField);
        }
        if raw.len() < head_size(k) {
            return Err(FormatError::Truncated);
        }
        let view = BundleView { raw };
        let (moff, mlen) = (u32_at(raw, 8), u32_at(raw, 12));
        let mut ranges = [(0u32, 0u32); MAX_ITEMS + 1];
        ranges[0] = view.check_range(moff, mlen)?;
        for i in 0..k {
            let it = view.item(i);
            ranges[i + 1] = view.check_range(it.offset, it.length)?;
        }
        let ranges = &ranges[..k + 1];
        for (i, a) in ranges.iter().enumerate() {
            if ranges[i + 1..].iter().any(|b| a.0 < b.1 && b.0 < a.1) {
                return Err(FormatError::BadField);
            }
        }
        let m = ManifestView::parse(view.manifest_bytes())?;
        if m.bytes().len() != mlen as usize {
            return Err(FormatError::BadField);
        }
        Ok(view)
    }

    fn check_range(&self, off: u32, len: u32) -> Result<(u32, u32), FormatError> {
        let end = off.checked_add(len).ok_or(FormatError::BadField)?;
        if !off.is_multiple_of(4) || (off as usize) < head_size(self.item_count()) {
            return Err(FormatError::BadField);
        }
        if end as usize > self.raw.len() {
            return Err(FormatError::Truncated);
        }
        Ok((off, end))
    }

    pub fn item_count(&self) -> usize {
        self.raw[6] as usize
    }

    /// Panics if `i >= item_count()`.
    pub fn item(&self, i: usize) -> BundleItem {
        assert!(i < self.item_count());
        let off = HEAD_SIZE + ITEM_SIZE * i;
        BundleItem::parse(&self.raw[off..off + ITEM_SIZE])
    }

    pub fn item_bytes(&self, i: usize) -> &'a [u8] {
        let it = self.item(i);
        &self.raw[it.offset as usize..(it.offset + it.length) as usize]
    }

    fn manifest_bytes(&self) -> &'a [u8] {
        let (off, len) = (u32_at(self.raw, 8) as usize, u32_at(self.raw, 12) as usize);
        &self.raw[off..off + len]
    }

    pub fn manifest(&self) -> ManifestView<'a> {
        // Validated in `parse`.
        ManifestView::parse(self.manifest_bytes()).expect("manifest checked in parse")
    }

    /// Index of the `dest = TOPOLOGY` item, if present.
    pub fn topology_item(&self) -> Option<usize> {
        (0..self.item_count()).find(|&i| self.item(i).dest == dest::TOPOLOGY)
    }

    /// Full host-side check: manifest signature, every item CRC, and per item
    /// either the image chain (entry `header_sha256`, header signature, header
    /// fields equal to the entry, payload SHA-256) or, for the topology item,
    /// SHA-256 equal to the manifest `topology_sha256`.
    pub fn verify(&self, pubkey: &[u8; 32]) -> Result<(), FormatError> {
        let m = self.manifest();
        m.verify(pubkey)?;
        let mut topologies = 0;
        for i in 0..self.item_count() {
            let it = self.item(i);
            let bytes = self.item_bytes(i);
            if crc32(bytes) != it.crc32 {
                return Err(FormatError::BadCrc);
            }
            match it.dest {
                dest::TOPOLOGY => {
                    topologies += 1;
                    if it.entry_index != NO_ENTRY || topologies > 1 {
                        return Err(FormatError::BadField);
                    }
                    if sha256(bytes) != m.fields().topology_sha256 {
                        return Err(FormatError::DigestMismatch);
                    }
                }
                dest::HUB_DIRECT | dest::SUB_STORE => verify_image_item(&m, &it, bytes, pubkey)?,
                _ => return Err(FormatError::BadField),
            }
        }
        Ok(())
    }
}

fn verify_image_item(
    m: &ManifestView<'_>,
    it: &BundleItem,
    bytes: &[u8],
    pubkey: &[u8; 32],
) -> Result<(), FormatError> {
    if it.entry_index as usize >= m.entry_count() || bytes.len() < HEADER_SIZE {
        return Err(FormatError::BadField);
    }
    let e = m.entry(it.entry_index as usize);
    let raw: [u8; HEADER_SIZE] = arr(bytes, 0);
    if header::header_digest(&raw) != e.header_sha256 {
        return Err(FormatError::DigestMismatch);
    }
    let h = header::verify_header(&raw, pubkey)?;
    let fields_match = h.image_kind == e.image_kind
        && h.slot_binding == e.slot_binding
        && h.version == e.version
        && h.payload_size == e.payload_size
        && h.proto_version == e.proto_version
        && bytes.len() == HEADER_SIZE + h.payload_size as usize;
    if !fields_match {
        return Err(FormatError::BadField);
    }
    if sha256(&bytes[HEADER_SIZE..]) != h.payload_sha256 {
        return Err(FormatError::DigestMismatch);
    }
    Ok(())
}
