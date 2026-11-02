//! Image version as one `u32`: `major << 24 | minor << 16 | patch`.
//!
//! Stored little-endian wherever a version appears (image header 0x018,
//! manifest entries, HELLO, STATUS, state log records). Because major sits in
//! the top byte, comparing the raw `u32` orders versions correctly. Display and
//! comparison of versions go through this type only; nothing else splits the
//! raw value.

use core::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(u32);

impl Version {
    pub const fn new(major: u8, minor: u8, patch: u16) -> Self {
        Version((major as u32) << 24 | (minor as u32) << 16 | patch as u32)
    }

    pub const fn from_raw(raw: u32) -> Self {
        Version(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn major(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub const fn minor(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub const fn patch(self) -> u16 {
        self.0 as u16
    }

    pub const fn to_le_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    pub const fn from_le_bytes(b: [u8; 4]) -> Self {
        Version(u32::from_le_bytes(b))
    }

    /// Short form for small displays: `"major.minor."` (e.g. `1.1.` on a
    /// 3-digit 7-segment display, where each `.` is the digit's point).
    pub const fn short(self) -> Short {
        Short(self)
    }
}

/// `"major.minor.patch"`, e.g. `1.1.0`.
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major(), self.minor(), self.patch())
    }
}

/// See [`Version::short`].
#[derive(Clone, Copy, Debug)]
pub struct Short(Version);

impl fmt::Display for Short {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.", self.0.major(), self.0.minor())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::format;

    #[test]
    fn layout_and_order() {
        let v = Version::new(1, 1, 0);
        assert_eq!(v.raw(), 0x0101_0000);
        assert_eq!(v.to_le_bytes(), [0x00, 0x00, 0x01, 0x01]);
        assert!(Version::new(1, 2, 0) > Version::new(1, 1, 0xFFFF));
        assert!(Version::new(2, 0, 0) > Version::new(1, 255, 0xFFFF));
        assert_eq!(format!("{}", Version::new(1, 10, 300)), "1.10.300");
        assert_eq!(format!("{}", v.short()), "1.1.");
    }
}
