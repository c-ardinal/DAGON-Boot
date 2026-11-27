//! What boot code reads off the flash: an image header and the state
//! log's face header and record. Nothing panics; what parses writes back
//! to bytes that parse to the same value.
#![no_main]

use dagon_format::header::{ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(raw) = data.first_chunk::<HEADER_SIZE>() {
        if let Ok(h) = ImageHeader::parse(raw) {
            assert_eq!(ImageHeader::parse(&h.to_bytes()), Ok(h));
        }
    }
    if let Some(raw) = data.first_chunk::<FACE_HEADER_SIZE>() {
        if let Ok(h) = FaceHeader::parse(raw) {
            assert_eq!(FaceHeader::parse(&h.to_bytes()), Ok(h));
        }
    }
    if let Some(raw) = data.first_chunk::<RECORD_SIZE>() {
        if let Ok(r) = Record::parse(raw) {
            assert_eq!(Record::parse(&r.to_bytes()), Ok(r));
        }
    }
});
