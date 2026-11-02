//! Text for a 3-digit 7-segment display. One byte per digit, bit 0..6 =
//! segments a..g, bit 7 = the digit's point.

use core::fmt::{self, Write};

use dagon_format::Version;

pub const DIGITS: usize = 3;
pub const POINT: u8 = 0x80;
pub const BLANK: [u8; DIGITS] = [0; DIGITS];
/// Shown when the text does not fit.
pub const DASHES: [u8; DIGITS] = [0x40; DIGITS];

const NUMERALS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];

fn glyph(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(NUMERALS[c as usize - '0' as usize]),
        ' ' => Some(0),
        '-' => Some(0x40),
        't' => Some(0x78),
        'S' => Some(0x6D),
        'E' => Some(0x79),
        _ => None,
    }
}

/// Right-aligned digits for `text`; a `.` lights the point of the digit
/// before it. `None` if a character has no glyph or it needs more digits.
pub fn render(text: &str) -> Option<[u8; DIGITS]> {
    let mut out = [0u8; DIGITS];
    let mut n = 0;
    for c in text.chars() {
        if c == '.' && n > 0 && out[n - 1] & POINT == 0 {
            out[n - 1] |= POINT;
            continue;
        }
        if n == DIGITS {
            return None;
        }
        out[n] = if c == '.' { POINT } else { glyph(c)? };
        n += 1;
    }
    let mut right = BLANK;
    right[DIGITS - n..].copy_from_slice(&out[..n]);
    Some(right)
}

/// The version as `major.minor.` (e.g. `1.1.`), dashes if it does not fit.
pub fn version(v: Version) -> [u8; DIGITS] {
    let mut buf = Buf::default();
    match write!(buf, "{}", v.short()) {
        Ok(()) => buf.as_str().and_then(render).unwrap_or(DASHES),
        Err(_) => DASHES,
    }
}

/// "TEST KEY" marker: `tSt`.
pub const TEST_KEY: [u8; DIGITS] = [0x78, 0x6D, 0x78];

#[derive(Default)]
struct Buf {
    b: [u8; 8],
    n: usize,
}

impl Buf {
    fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.b[..self.n]).ok()
    }
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.n + s.len();
        self.b
            .get_mut(self.n..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.n = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        let one = NUMERALS[1];
        assert_eq!(
            version(Version::new(1, 1, 0)),
            [0, one | POINT, one | POINT]
        );
        assert_eq!(
            version(Version::new(1, 10, 7)),
            [one | POINT, one, NUMERALS[0] | POINT]
        );
        assert_eq!(version(Version::new(10, 10, 0)), DASHES);
        assert_eq!(version(Version::new(255, 255, 0)), DASHES);
    }

    #[test]
    fn render_points_and_glyphs() {
        assert_eq!(render("tSt"), Some(TEST_KEY));
        assert_eq!(render(".."), Some([0, POINT, POINT]));
        assert_eq!(render("1234"), None);
        assert_eq!(render("x"), None);
    }
}
