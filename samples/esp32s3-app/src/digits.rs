//! The running version as large seven-segment digits on the round 466×466
//! AMOLED, as solid rectangles (the panel draws only even-aligned areas).
//!
//! Legibility: light strokes on true black (unlit pixels), strokes one fifth
//! of the digit width or more, no thin features, and the whole line kept
//! inside the circle with a margin, as large as fits.

use dagon_mcu_esp32s3::Version;

/// Panel width and height in pixels.
pub const SIZE: u16 = 466;
/// Background: pixels off.
pub const BLACK: u16 = 0x0000;
/// Strokes: near white (RGB565 #F7F7F7), slightly off full white so the
/// AMOLED does not bloom.
pub const INK: u16 = 0xF7BE;
/// Clearance between the text and the screen edge.
const MARGIN: u32 = 24;
/// Most characters in a version line: "255.255.65535".
pub const MAX_CHARS: usize = 13;
/// Most rectangles one line can need (seven segments per character).
pub const MAX_RECTS: usize = MAX_CHARS * 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// The line to show: `major.minor.patch`, or three dashes when the version
/// is unknown. Returns the characters used in `buf`.
pub fn text(version: Option<Version>, buf: &mut [u8; MAX_CHARS]) -> usize {
    let Some(v) = version else {
        buf[..3].copy_from_slice(b"---");
        return 3;
    };
    let mut n = 0;
    for (i, part) in [
        u32::from(v.major()),
        u32::from(v.minor()),
        u32::from(v.patch()),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            buf[n] = b'.';
            n += 1;
        }
        let mut digits = [0u8; 5];
        let mut k = 0;
        let mut x = part;
        loop {
            digits[k] = b'0' + (x % 10) as u8;
            k += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        for &d in digits[..k].iter().rev() {
            buf[n] = d;
            n += 1;
        }
    }
    n
}

/// Segments a..g as bits 0..6; `None` for characters that are not drawn as
/// a digit cell.
fn segments(c: u8) -> Option<u8> {
    const DIGITS: [u8; 10] = [
        0b011_1111, 0b000_0110, 0b101_1011, 0b100_1111, 0b110_0110, 0b110_1101, 0b111_1101,
        0b000_0111, 0b111_1111, 0b110_1111,
    ];
    match c {
        b'0'..=b'9' => Some(DIGITS[usize::from(c - b'0')]),
        b'-' => Some(0b100_0000),
        _ => None,
    }
}

/// Unit `u`: stroke `u`, digit cell `6u` × `11u`, a dot is `u` × `u`.
fn width(chars: &[u8], u: u32) -> u32 {
    let items: u32 = chars
        .iter()
        .map(|&c| if segments(c).is_some() { 6 * u } else { u })
        .sum();
    let gaps = chars.windows(2).map(|w| gap(w[0], w[1], u)).sum::<u32>();
    items + gaps
}

/// Space between two neighbours: two strokes between digits, one next to a dot.
fn gap(a: u8, b: u8, u: u32) -> u32 {
    if segments(a).is_some() && segments(b).is_some() {
        2 * u
    } else {
        u
    }
}

/// Largest even unit whose line fits inside the circle with [`MARGIN`]:
/// the line's corners must lie within radius `SIZE / 2`.
fn unit(chars: &[u8]) -> u32 {
    let r = u32::from(SIZE) / 2;
    (2..=40u32)
        .rev()
        .step_by(2)
        .find(|&u| {
            let half_w = width(chars, u) / 2 + MARGIN;
            let half_h = 11 * u / 2 + MARGIN;
            half_w * half_w + half_h * half_h <= r * r
        })
        .unwrap_or(2)
}

/// Rectangles that draw `chars` centred on the screen, in `out`; returns how
/// many. Every origin and size is even.
pub fn layout(chars: &[u8], out: &mut [Rect; MAX_RECTS]) -> usize {
    let chars = &chars[..chars.len().min(MAX_CHARS)];
    let u = unit(chars);
    let even = |v: u32| (v & !1) as u16;
    let mut x = even((u32::from(SIZE) - width(chars, u).min(u32::from(SIZE))) / 2);
    let y = even((u32::from(SIZE) - 11 * u) / 2);
    let (u16u, mut n) = (u as u16, 0);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            x += gap(chars[i - 1], c, u) as u16;
        }
        match segments(c) {
            Some(bits) => {
                for (s, r) in cell(x, y, u16u).into_iter().enumerate() {
                    if bits & (1 << s) != 0 {
                        out[n] = r;
                        n += 1;
                    }
                }
                x += 6 * u16u;
            }
            None => {
                // Dot on the baseline.
                out[n] = Rect {
                    x,
                    y: y + 10 * u16u,
                    w: u16u,
                    h: u16u,
                };
                n += 1;
                x += u16u;
            }
        }
    }
    n
}

/// Segments a..g of the digit cell at (`x`, `y`).
fn cell(x: u16, y: u16, u: u16) -> [Rect; 7] {
    let r = |dx: u16, dy: u16, w: u16, h: u16| Rect {
        x: x + dx * u,
        y: y + dy * u,
        w: w * u,
        h: h * u,
    };
    [
        r(1, 0, 4, 1),  // a: top
        r(5, 1, 1, 4),  // b: upper right
        r(5, 6, 1, 4),  // c: lower right
        r(1, 10, 4, 1), // d: bottom
        r(0, 6, 1, 4),  // e: lower left
        r(0, 1, 1, 4),  // f: upper left
        r(1, 5, 4, 1),  // g: middle
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(v: Option<Version>) -> Vec<Rect> {
        let mut buf = [0u8; MAX_CHARS];
        let n = text(v, &mut buf);
        let mut out = [Rect::default(); MAX_RECTS];
        let k = layout(&buf[..n], &mut out);
        out[..k].to_vec()
    }

    fn inside_circle(r: &Rect) -> bool {
        let c = i32::from(SIZE) / 2;
        let corners = [
            (r.x, r.y),
            (r.x + r.w, r.y),
            (r.x, r.y + r.h),
            (r.x + r.w, r.y + r.h),
        ];
        corners.iter().all(|&(x, y)| {
            let (dx, dy) = (i32::from(x) - c, i32::from(y) - c);
            dx * dx + dy * dy <= c * c
        })
    }

    #[test]
    fn text_of_versions() {
        let mut buf = [0u8; MAX_CHARS];
        let n = text(Some(Version::new(1, 10, 0)), &mut buf);
        assert_eq!(&buf[..n], b"1.10.0");
        let n = text(Some(Version::new(255, 255, 65535)), &mut buf);
        assert_eq!(&buf[..n], b"255.255.65535");
        let n = text(None, &mut buf);
        assert_eq!(&buf[..n], b"---");
    }

    #[test]
    fn rects_are_even_and_inside_the_round_screen() {
        for v in [
            Some(Version::new(1, 0, 0)),
            Some(Version::new(1, 1, 0)),
            Some(Version::new(255, 255, 65535)),
            None,
        ] {
            let rects = line(v);
            assert!(!rects.is_empty());
            for r in &rects {
                assert!(r.w > 0 && r.h > 0);
                assert_eq!((r.x | r.y | r.w | r.h) & 1, 0, "{r:?} not even");
                assert!(inside_circle(r), "{v:?}: {r:?} outside the circle");
            }
        }
    }

    #[test]
    fn short_versions_are_large_and_distinct() {
        let a = line(Some(Version::new(1, 0, 0)));
        let b = line(Some(Version::new(1, 1, 0)));
        assert_ne!(a, b);
        // "1.0.0": 2 + 6 + 6 segments and 2 dots; strokes of 12 px or more.
        assert_eq!(a.len(), 16);
        assert!(a.iter().map(|r| r.w.min(r.h)).min().unwrap() >= 12);
        // Digit height at least a fifth of the screen.
        let top = a.iter().map(|r| r.y).min().unwrap();
        let bottom = a.iter().map(|r| r.y + r.h).max().unwrap();
        assert!(bottom - top >= SIZE / 5);
    }
}
