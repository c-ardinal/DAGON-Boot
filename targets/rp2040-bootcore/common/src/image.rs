//! Inline images in a slot: header at the slot start, body right after.

use dagon_format::header::{self, mcu_id, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::reason;
use dagon_format::Sha256;
use dagon_port::Flash;

use crate::layout::HEADER_LEN;
use crate::log::Mark;

/// The header in the slot at `base` as a mark for slot `slot`, `None` when
/// there is no parsable header (erased or partly written).
pub fn head_mark<F: Flash>(flash: &mut F, base: u32, slot: u8) -> Option<Mark> {
    let mut raw = [0u8; HEADER_SIZE];
    flash.read(base, &mut raw).ok()?;
    mark_of(&raw, slot)
}

/// A raw header as a mark for slot `slot`; `None` if it does not parse.
pub fn mark_of(raw: &[u8; HEADER_SIZE], slot: u8) -> Option<Mark> {
    let h = ImageHeader::parse(raw).ok()?;
    Some(Mark {
        slot,
        version: h.version,
        digest16: header::header_digest16(raw),
    })
}

/// Checks the image in the slot at `base` (capacity `cap` incl. header)
/// as slot `slot` before starting it: the header is the one `expect` names
/// (when the log names one), fits, is of
/// `image_kind` for this MCU and the body hashes to `payload_sha256`.
/// Errors are state log reason codes. Returns the flash offset of the body.
pub fn check<F: Flash>(
    flash: &mut F,
    base: u32,
    cap: u32,
    slot: u8,
    expect: Option<&Mark>,
    image_kind: u8,
) -> Result<u32, u8> {
    let mut raw = [0u8; HEADER_SIZE];
    flash
        .read(base, &mut raw)
        .map_err(|_| reason::HASH_MISMATCH)?;
    let h = ImageHeader::parse(&raw).map_err(|_| reason::HASH_MISMATCH)?;
    let fits = h.payload_size <= cap - HEADER_LEN;
    let ours = h.image_kind == image_kind
        && h.mcu_id == mcu_id::RP2040
        && (h.slot_binding == slot || h.slot_binding == header::slot_binding::ANY);
    let named = expect.is_none_or(|m| m.digest16 == header::header_digest16(&raw));
    if !named || !fits || !ours {
        return Err(reason::HASH_MISMATCH);
    }
    let body = base + HEADER_LEN;
    if hash(flash, body, h.payload_size) != Some(h.payload_sha256) {
        return Err(reason::HASH_MISMATCH);
    }
    Ok(body)
}

/// Key ID and Ed25519 signature of the header in the slot at `base`.
pub fn verify<F: Flash>(flash: &mut F, base: u32, pubkey: &[u8; 32]) -> Result<(), u8> {
    let mut raw = [0u8; HEADER_SIZE];
    flash
        .read(base, &mut raw)
        .map_err(|_| reason::SIG_INVALID)?;
    header::verify_header(&raw, pubkey)
        .map(|_| ())
        .map_err(|_| reason::SIG_INVALID)
}

/// SHA-256 of `len` bytes at `addr`.
pub fn hash<F: Flash>(flash: &mut F, addr: u32, len: u32) -> Option<[u8; 32]> {
    let mut h = Sha256::new();
    let mut buf = [0u8; 256];
    let mut off = 0;
    while off < len {
        let n = (len - off).min(buf.len() as u32) as usize;
        flash.read(addr + off, &mut buf[..n]).ok()?;
        h.update(&buf[..n]);
        off += n as u32;
    }
    Some(h.finalize())
}
