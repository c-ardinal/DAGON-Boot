//! Text for the 4-digit 7-segment display of the X-NUCLEO-53L0A1. One byte
//! per digit (left to right), bit 0..6 = segments a..g, bit 7 = the digit's
//! point. The shield wires no point: [`POINT`] is kept in the text but not
//! shown (see [`crate::shield`]).

use core::fmt::{self, Write};

use dagon_format::Version;

pub const DIGITS: usize = 4;
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
        // The update words (crate::update).
        'A' => Some(0x77),
        'b' => Some(0x7C),
        'C' => Some(0x39),
        'c' => Some(0x58),
        'd' => Some(0x5E),
        'F' => Some(0x71),
        'h' => Some(0x74),
        // `I` on the left verticals, `i` the lower left one: neither looks like 1.
        'I' => Some(0x30),
        'i' => Some(0x10),
        'L' => Some(0x38),
        'n' => Some(0x54),
        'o' => Some(0x5C),
        'P' => Some(0x73),
        'r' => Some(0x50),
        _ => None,
    }
}

/// Right-aligned digits for `text`; a `.` sets the point of the digit
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

/// The version as `major.minor.patch` (`1.2.0` shows `120`, the shield has
/// no points), `major.minor.` if that does not fit, else dashes.
pub fn version(v: Version) -> [u8; DIGITS] {
    fn fit(args: fmt::Arguments) -> Option<[u8; DIGITS]> {
        let mut buf = Buf::default();
        buf.write_fmt(args).ok()?;
        buf.as_str().and_then(render)
    }
    fit(format_args!("{v}"))
        .or_else(|| fit(format_args!("{}", v.short())))
        .unwrap_or(DASHES)
}

/// "TEST KEY" marker: `tESt`.
pub const TEST_KEY: [u8; DIGITS] = [0x78, 0x79, 0x6D, 0x78];

#[derive(Default)]
pub(crate) struct Buf {
    b: [u8; 12],
    n: usize,
}

impl Buf {
    pub(crate) fn as_str(&self) -> Option<&str> {
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
    fn numerals_light_the_usual_segments() {
        // a..g = bit 0..6.
        let expected = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
        for (d, code) in expected.into_iter().enumerate() {
            let text = [b'0' + d as u8];
            let text = core::str::from_utf8(&text).unwrap();
            assert_eq!(render(text), Some([0, 0, 0, code]), "{text}");
        }
    }

    #[test]
    fn points_dashes_and_blanks() {
        assert_eq!(render(""), Some(BLANK));
        assert_eq!(render("    "), Some(BLANK));
        assert_eq!(render("-"), Some([0, 0, 0, 0x40]));
        assert_eq!(render("1."), Some([0, 0, 0, 0x06 | POINT]));
        assert_eq!(render("."), Some([0, 0, 0, POINT]));
        assert_eq!(render(".."), Some([0, 0, POINT, POINT]));
        assert_eq!(render("1 -"), Some([0, 0x06, 0, 0x40]));
        assert_eq!(render("tESt"), Some(TEST_KEY));
        assert_eq!(render("12345"), None);
        assert_eq!(render("x"), None);
    }

    #[test]
    fn versions() {
        let [zero, one, two] = [0x3F, 0x06, 0x5B];
        assert_eq!(
            version(Version::new(1, 2, 0)),
            [0, one | POINT, two | POINT, zero]
        );
        assert_eq!(
            version(Version::new(1, 10, 7)),
            [one | POINT, one, zero | POINT, 0x07]
        );
        // 10.10.0 needs five digits: major.minor. instead.
        assert_eq!(
            version(Version::new(10, 10, 0)),
            [one, zero | POINT, one, zero | POINT]
        );
        assert_eq!(version(Version::new(255, 255, 0)), DASHES);
    }
}
