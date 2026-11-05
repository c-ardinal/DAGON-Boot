//! Stage0's version mark: 12 bytes a stage0 image carries anywhere in its
//! region, 4-byte aligned, so the agent can name the stage0 it boots
//! behind (VERSION `stage0`). Stage0 is written once and never updated, so
//! a node keeps a stage0 built before the mark existed: no mark found
//! means "unknown", never an error.
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | magic `DGS0` |
//! | 4 | 1 | format ([`FORMAT`]) |
//! | 5 | 3 | zero |
//! | 8 | 4 | version (u32 LE, [`Version`]) |
//!
//! A reader takes only [`FORMAT`]; a mark of another format reads as no
//! mark.

use crate::Version;

pub const MAGIC: [u8; 4] = *b"DGS0";
pub const FORMAT: u8 = 1;
pub const LEN: usize = 12;
/// Marks start on this boundary.
pub const ALIGN: usize = 4;

/// The mark as stage0 links it: `static MARK: Mark = Mark::of(...)`, kept
/// in the image by a reference the code takes (`core::hint::black_box`).
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark(pub [u8; LEN]);

impl Mark {
    pub const fn of(v: Version) -> Mark {
        let b = v.to_le_bytes();
        let m = MAGIC;
        Mark([
            m[0], m[1], m[2], m[3], FORMAT, 0, 0, 0, b[0], b[1], b[2], b[3],
        ])
    }
}

/// The version of a mark at the start of `b`, if `b` starts with one.
pub fn parse(b: &[u8]) -> Option<Version> {
    let b = b.get(..LEN)?;
    if b[..4] != MAGIC || b[4] != FORMAT || b[5..8] != [0; 3] {
        return None;
    }
    Some(Version::from_le_bytes([b[8], b[9], b[10], b[11]]))
}

/// Bytes read per call while searching.
const CHUNK: usize = 128;

/// The first mark in the `size` bytes at `at`, read through `read(addr,
/// buf)` (`false`: unreadable, the search ends with `None`).
pub fn find(at: u32, size: u32, mut read: impl FnMut(u32, &mut [u8]) -> bool) -> Option<Version> {
    let mut buf = [0u8; CHUNK];
    let mut off = 0u32;
    while off as usize + LEN <= size as usize {
        let n = ((size - off) as usize).min(CHUNK);
        if !read(at + off, &mut buf[..n]) {
            return None;
        }
        let mut i = 0;
        while i + LEN <= n {
            if let Some(v) = parse(&buf[i..]) {
                return Some(v);
            }
            i += ALIGN;
        }
        if n < CHUNK {
            return None;
        }
        // `i` is the first aligned offset whose mark would cross the
        // chunk's end: the next chunk starts there.
        off += i as u32;
    }
    None
}

/// `"major.minor.patch"` (a Cargo package version without pre-release or
/// build parts) as a [`Version`], at compile time:
/// `Mark::of(stage0::version(env!("CARGO_PKG_VERSION")))`. Panics (a
/// build error in a const) on anything else.
pub const fn version(s: &str) -> Version {
    let b = s.as_bytes();
    let mut parts = [0u32; 3];
    let (mut p, mut i, mut digits) = (0, 0, 0);
    while i < b.len() {
        match b[i] {
            b'.' if p < 2 && digits > 0 => {
                p += 1;
                digits = 0;
            }
            c @ b'0'..=b'9' => {
                parts[p] = parts[p] * 10 + (c - b'0') as u32;
                digits += 1;
            }
            _ => panic!("not a major.minor.patch version"),
        }
        i += 1;
    }
    assert!(p == 2 && digits > 0, "not a major.minor.patch version");
    assert!(parts[0] <= 0xFF && parts[1] <= 0xFF && parts[2] <= 0xFFFF);
    Version::new(parts[0] as u8, parts[1] as u8, parts[2] as u16)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;

    #[test]
    fn mark_bytes_and_parse() {
        let m = Mark::of(Version::new(1, 2, 3));
        assert_eq!(
            m.0,
            [b'D', b'G', b'S', b'0', 1, 0, 0, 0, 3, 0, 2, 1],
            "magic, format, zero, version LE"
        );
        assert_eq!(parse(&m.0), Some(Version::new(1, 2, 3)));
        let mut other = m.0;
        other[4] = 2;
        assert_eq!(parse(&other), None, "another format reads as no mark");
        assert_eq!(parse(&m.0[..LEN - 1]), None);
        assert_eq!(core::mem::align_of::<Mark>(), ALIGN);
    }

    fn region(len: usize, mark_at: Option<usize>) -> std::vec::Vec<u8> {
        let mut r = vec![0xFFu8; len];
        if let Some(at) = mark_at {
            r[at..at + LEN].copy_from_slice(&Mark::of(Version::new(0, 1, 0)).0);
        }
        r
    }

    fn search(r: &[u8]) -> Option<Version> {
        find(0x1000, r.len() as u32, |a, b| {
            let o = (a - 0x1000) as usize;
            b.copy_from_slice(&r[o..o + b.len()]);
            true
        })
    }

    #[test]
    fn finds_the_mark_anywhere_aligned_including_across_chunks() {
        let len = 16 * 1024;
        for at in [0, 4, CHUNK - 8, CHUNK - 4, CHUNK, 3 * CHUNK - 8, len - LEN] {
            assert_eq!(
                search(&region(len, Some(at))),
                Some(Version::new(0, 1, 0)),
                "{at}"
            );
        }
        assert_eq!(
            search(&region(len, None)),
            None,
            "a stage0 without the mark"
        );
        assert_eq!(
            search(&region(len, Some(2))),
            None,
            "unaligned is not a mark"
        );
        assert_eq!(search(&region(LEN - 1, None)), None);
        assert_eq!(find(0, 4096, |_, _| false), None, "unreadable");
    }

    #[test]
    fn cargo_versions() {
        const V: Version = version("0.1.0");
        assert_eq!(V, Version::new(0, 1, 0));
        assert_eq!(version("12.34.5678"), Version::new(12, 34, 5678));
        for bad in ["1.0", "1.0.0-rc.1", "1..0", "", "1.0.0.0"] {
            assert!(std::panic::catch_unwind(|| version(bad)).is_err(), "{bad}");
        }
    }
}
