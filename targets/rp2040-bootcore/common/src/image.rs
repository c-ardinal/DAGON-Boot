//! Inline images in a slot: header at the slot start, body right after.

use dagon_core::log::SlotRef;
use dagon_format::header::{self, mcu_id, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::reason;
use dagon_format::Sha256;
use dagon_port::Flash;

use crate::layout::HEADER_LEN;

/// The header in the slot at `base` as a reference for slot `slot`, `None`
/// when there is no parsable header (erased or partly written).
pub fn head_ref<F: Flash>(flash: &mut F, base: u32, slot: u8) -> Option<SlotRef> {
    let mut raw = [0u8; HEADER_SIZE];
    flash.read(base, &mut raw).ok()?;
    ref_of(&raw, slot)
}

/// A raw header as a reference for slot `slot`; `None` if it does not parse.
pub fn ref_of(raw: &[u8; HEADER_SIZE], slot: u8) -> Option<SlotRef> {
    let h = ImageHeader::parse(raw).ok()?;
    Some(SlotRef {
        slot,
        version: h.version,
        digest16: header::header_digest16(raw),
    })
}

/// stage0's check of the boot core in the slot at `base` (capacity `cap`
/// incl. header) as slot `slot`: the header is the one `expect` names (when
/// the log names one), fits, is of `image_kind` for this MCU, is linked for
/// this slot and the body hashes to `payload_sha256`. Errors are state log
/// reason codes. Returns the body address.
pub fn check<F: Flash>(
    flash: &mut F,
    base: u32,
    cap: u32,
    slot: u8,
    expect: Option<&SlotRef>,
    image_kind: u8,
) -> Result<u32, u8> {
    let mut raw = [0u8; HEADER_SIZE];
    flash
        .read(base, &mut raw)
        .map_err(|_| reason::HASH_MISMATCH)?;
    let h = ImageHeader::parse(&raw).map_err(|_| reason::HASH_MISMATCH)?;
    let body = base + HEADER_LEN;
    let fits = h.payload_size <= cap - HEADER_LEN && h.load_addr == body;
    let ours = h.image_kind == image_kind
        && h.mcu_id == mcu_id::RP2040
        && (h.slot_binding == slot || h.slot_binding == header::slot_binding::ANY);
    let named = expect.is_none_or(|m| m.digest16 == header::header_digest16(&raw));
    if !named || !fits || !ours {
        return Err(reason::HASH_MISMATCH);
    }
    if hash(flash, body, h.payload_size) != Some(h.payload_sha256) {
        return Err(reason::HASH_MISMATCH);
    }
    Ok(body)
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
