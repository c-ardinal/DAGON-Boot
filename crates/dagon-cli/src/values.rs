//! Values the commands take as text (versions, slot letters, plain names)
//! and what an MCU's slots are.

use dagon_format::header::{image_kind, slot_binding};
use dagon_format::Version;
use dagon_sign::image;
use dagon_topology::defs::Registry;

/// `s` as a version (`major.minor.patch`).
pub(crate) fn version(s: &str) -> Result<Version, String> {
    image::parse_version(s).ok_or_else(|| format!("bad version '{s}' (want major.minor.patch)"))
}

/// The image header binding of slot letter `c` (`a` slot A, `b` slot B):
/// the letter's place in the alphabet, as STATUS numbers the slots.
pub(crate) fn binding_of(c: char) -> u8 {
    (c.to_ascii_lowercase() as u8).wrapping_sub(b'a')
}

/// The letter of slot `binding`.
pub(crate) fn slot_letter(binding: u8) -> char {
    char::from(b'a' + binding)
}

/// The letters of every image slot the wire has room for (`a`, `b`).
pub(crate) fn slot_letters() -> impl Iterator<Item = char> {
    (0..dagon_proto::payload::StatusResp::IMAGE_SLOTS as u8).map(slot_letter)
}

/// Node ids, release names: what may become a file name.
pub(crate) fn plain_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !s.starts_with('.')
        && !crate::paths::device_name(s)
}

/// Whether apps for `mcu` are linked separately for slot B. Not for an MCU
/// whose one image runs from either slot (ESP32-S3): it builds once.
pub(crate) fn has_slot_b_link(reg: &Registry, mcu: &str) -> bool {
    dagon_sign::image::slot_with(reg, mcu, image_kind::APP, slot_binding::B).is_some()
}
