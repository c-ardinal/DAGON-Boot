//! Inline images in a slot: header at the slot start, body right after.

use dagon_core::log::SlotRef;
use dagon_format::header::{self, ImageHeader, HEADER_SIZE};
use dagon_port::Flash;

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
