//! The round display's home screen (240x240), drawn onto any
//! `embedded-graphics` target so the layout runs in host tests: the 親機 in
//! the centre, up to three 子機 below it, each with its name, version and
//! a state word in Japanese under a ring in the state colour. While the PC
//! sends SHOW, the named node's ring becomes an arc that grows with the
//! percentage in the phase colour, with a line of text on top. A build
//! with the public test key shows "TEST KEY" at the bottom, clear of every
//! node ring.

use core::fmt::Write as _;

use dagon_mcu_rp2040::view::{Home, Node, Progress, Shade};
use dagon_proto::payload::phase;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Arc, Circle, PrimitiveStyle};
use embedded_graphics::text::{Alignment, Text};
use heapless::String;

use crate::glyphs::{CELL, GLYPHS};

pub const HUB_AT: Point = Point::new(120, 80);
pub const HUB_D: u32 = 90;
pub const LEFT: Point = Point::new(55, 150);
pub const BOTTOM: Point = Point::new(120, 172);
pub const RIGHT: Point = Point::new(185, 150);
pub const SUB_D: u32 = 60;

/// Sub node positions by how many there are.
pub fn sub_at(count: usize) -> &'static [Point] {
    match count {
        0 => &[],
        1 => &[BOTTOM],
        2 => &[LEFT, RIGHT],
        _ => &[LEFT, BOTTOM, RIGHT],
    }
}

pub const RING: u32 = 5;
const ARC: u32 = 9;
/// Top text line; the round panel is narrower up there.
const TOP_AT: Point = Point::new(120, 22);
const TOP_CHARS: usize = 22;
/// "TEST KEY" baseline: below the lowest node ring (the bottom 子機 ends
/// at y 205) and inside the round panel.
pub const TEST_KEY_AT: Point = Point::new(120, 226);

/// Draws the home screen; `test_key`: the firmware carries the public
/// test key.
pub fn draw<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    home: &Home,
    test_key: bool,
) -> Result<(), D::Error> {
    d.clear(Rgb565::BLACK)?;
    let p = home.progress.as_ref();
    node(d, HUB_AT, HUB_D, &home.hub, p, &FONT_10X20)?;
    for (&at, n) in sub_at(home.subs.len()).iter().zip(&home.subs) {
        node(d, at, SUB_D, n, p, &FONT_6X10)?;
    }
    if let Some(p) = p {
        let name = core::iter::once(&home.hub)
            .chain(&home.subs)
            .find(|n| n.node.index == p.node)
            .map_or("?", |n| n.node.name());
        let mut line: String<64> = String::new();
        if p.text.is_empty() {
            let _ = write!(line, "{name}: {} {}%", label(p.phase), p.percent);
        } else {
            let _ = write!(line, "{name}: {}", p.text);
        }
        let shown = &line[..line.len().min(TOP_CHARS)];
        let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
        Text::with_alignment(shown, TOP_AT, style, Alignment::Center).draw(d)?;
    }
    if test_key {
        // Built with the public test key: anyone can sign images for it.
        Text::with_alignment("TEST KEY", TEST_KEY_AT, test_key_style(), Alignment::Center)
            .draw(d)?;
    }
    Ok(())
}

fn test_key_style() -> MonoTextStyle<'static, Rgb565> {
    MonoTextStyle::new(&FONT_10X20, RED)
}

fn label(ph: u8) -> &'static str {
    match ph {
        phase::TRANSFER => "transfer",
        phase::VERIFY => "verify",
        phase::REBOOT => "reboot",
        phase::CONFIRM => "confirmed",
        phase::REJECTED => "rejected",
        phase::FAILED => "failed",
        phase::ROM => "ROM",
        _ => "",
    }
}

fn phase_colour(ph: u8) -> Rgb565 {
    match ph {
        phase::CONFIRM => GREEN,
        phase::REJECTED => RED,
        phase::FAILED => YELLOW,
        phase::ROM => PURPLE,
        _ => BLUE,
    }
}

fn node<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    at: Point,
    diameter: u32,
    n: &Node,
    progress: Option<&Progress>,
    font: &MonoFont<'_>,
) -> Result<(), D::Error> {
    let shown = progress.filter(|p| p.node == n.node.index);
    match shown {
        Some(p) => {
            Circle::with_center(at, diameter)
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::CSS_DIM_GRAY, RING))
                .draw(d)?;
            let sweep = p.percent as f32 * 3.6;
            Arc::with_center(at, diameter, (-90.0f32).deg(), sweep.deg())
                .into_styled(PrimitiveStyle::with_stroke(phase_colour(p.phase), ARC))
                .draw(d)?;
        }
        None => {
            Circle::with_center(at, diameter)
                .into_styled(PrimitiveStyle::with_stroke(colour(n.shade), RING))
                .draw(d)?;
        }
    }
    let style = MonoTextStyle::new(font, Rgb565::WHITE);
    let line = font.character_size.height as i32;
    // Name and version above the centre, the state in words below it, so
    // colour is never the only cue.
    let name_at = at - Point::new(0, line / 2 + 2);
    Text::with_alignment(n.node.name(), name_at, style, Alignment::Center).draw(d)?;
    let mut v: String<16> = String::new();
    match shown.map(|p| p.version).or(n.version) {
        Some(ver) => {
            let _ = write!(v, "{ver}");
        }
        None => {
            let _ = v.push_str("--");
        }
    }
    let (word, c) = word_of(n, shown);
    let (top, over_version) = word_place(at, diameter, font, &word, shown.is_some());
    if !over_version {
        let version_at = at + Point::new(0, line / 2 - 2);
        Text::with_alignment(&v, version_at, style, Alignment::Center).draw(d)?;
    }
    word_draw(d, &word, top, c)
}

/// Where the state word goes in a node ring of `diameter` at `at`: its top
/// row, and whether it takes the version's row. Normally it sits below the
/// version, inside the ring. While a progress arc (wider than the ring) is
/// shown, a word that would touch the arc there moves up to the version's
/// row instead (the top line names the version being installed).
pub fn word_place(
    at: Point,
    diameter: u32,
    font: &MonoFont<'_>,
    word: &str,
    progress: bool,
) -> (Point, bool) {
    let line = font.character_size.height as i32;
    let below = at + Point::new(0, line / 2 + 1);
    if !progress || word.is_ascii() || fits(word, below, at, clear_of_arc(diameter)) {
        return (below, false);
    }
    (at + Point::new(0, line / 2 - 9), true)
}

/// Radius a word stays within under a progress arc: inside the arc's
/// stroke with a visible gap.
pub fn clear_of_arc(diameter: u32) -> i32 {
    inside(diameter, ARC) - ARC_GAP
}

/// Pixels between a word and the progress arc.
const ARC_GAP: i32 = 2;

/// Radius inside a stroke of `width` on a circle of `diameter`, with a
/// pixel to spare.
pub fn inside(diameter: u32, width: u32) -> i32 {
    (diameter / 2 - width / 2 - 1) as i32
}

/// Whether every pixel of `word` at `top` lies within `radius` of `at`.
pub fn fits(word: &str, top: Point, at: Point, radius: i32) -> bool {
    word_pixels(word, top).all(|p| {
        let (dx, dy) = (p.x - at.x, p.y - at.y);
        dx * dx + dy * dy <= radius * radius
    })
}

/// The state word under a node and its colour.
fn word_of(n: &Node, shown: Option<&Progress>) -> (String<16>, Rgb565) {
    let mut word: String<16> = String::new();
    let c = match shown {
        Some(p) => {
            match p.phase {
                phase::TRANSFER => {
                    let _ = write!(word, "{}%", p.percent);
                }
                _ => {
                    let _ = word.push_str(phase_word(p.phase, &p.text));
                }
            }
            phase_colour(p.phase)
        }
        None => {
            let _ = word.push_str(state_word(n.shade));
            colour(n.shade)
        }
    };
    (word, c)
}

/// At most 4 Japanese characters: fits a small node ring. A rejection
/// or failure names its cause when the PC's SHOW text gives it.
pub fn phase_word(ph: u8, text: &str) -> &'static str {
    match ph {
        phase::VERIFY => "検証",
        phase::REBOOT => "再起動",
        phase::CONFIRM => "確定",
        phase::REJECTED => match text {
            "E_SIGNATURE" => "署名不正",
            "E_KEY" => "鍵不一致",
            "E_COMPAT" => "非互換",
            "E_DIGEST" => "破損",
            _ => "拒否",
        },
        phase::FAILED => match text {
            "ROLLED BACK" => "巻き戻し",
            "NO ANSWER" => "無応答",
            _ => "失敗",
        },
        phase::ROM => "ROM",
        _ => "",
    }
}

/// As [`phase_word`].
pub fn state_word(s: Shade) -> &'static str {
    match s {
        Shade::Unknown => "...",
        Shade::Confirmed => "確定",
        Shade::Trial => "試行中",
        Shade::NoAnswer => "無応答",
        Shade::RolledBack => "巻き戻し",
    }
}

/// Width in pixels of `word`: Japanese characters take an [`CELL`] cell,
/// others the small ASCII font's 6.
fn word_width(word: &str) -> i32 {
    word.chars()
        .map(|c| if c.is_ascii() { 6 } else { CELL as i32 })
        .sum()
}

/// The lit pixels of `word` centred on `top` (its first row).
pub fn word_pixels(word: &str, top: Point) -> impl Iterator<Item = Point> + '_ {
    let mut x = top.x - word_width(word) / 2;
    word.chars().flat_map(move |c| {
        let left = x;
        x += if c.is_ascii() { 6 } else { CELL as i32 };
        let rows = GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, r)| *r);
        (0..CELL as i32).flat_map(move |dy| {
            (0..CELL as i32).filter_map(move |dx| {
                let row = rows?[dy as usize];
                (row & (1 << (CELL as i32 - 1 - dx)) != 0)
                    .then_some(Point::new(left + dx, top.y + dy))
            })
        })
    })
}

fn word_draw<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    word: &str,
    top: Point,
    c: Rgb565,
) -> Result<(), D::Error> {
    if word.is_ascii() {
        let small = MonoTextStyle::new(&FONT_6X10, c);
        let baseline = top + Point::new(0, 8);
        Text::with_alignment(word, baseline, small, Alignment::Center).draw(d)?;
        return Ok(());
    }
    d.draw_iter(word_pixels(word, top).map(|p| Pixel(p, c)))
}

/// The six LCD state colours (green confirmed, blue transfer/trial,
/// yellow rolled back, red rejected/no answer, purple ROM path, grey
/// unknown), lightened where needed so each keeps at least 4.5:1 against
/// the black background (pure blue and dark purple do not).
const GREEN: Rgb565 = Rgb565::new(0, 56, 10); // about #00E052
const BLUE: Rgb565 = Rgb565::new(8, 40, 31); // about #42A2FF
const YELLOW: Rgb565 = Rgb565::new(31, 56, 0); // about #FFE000
const RED: Rgb565 = Rgb565::new(31, 12, 8); // about #FF3142
const PURPLE: Rgb565 = Rgb565::new(24, 28, 31); // about #C672FF
const GREY: Rgb565 = Rgb565::new(18, 37, 18); // about #949594

pub fn colour(s: Shade) -> Rgb565 {
    match s {
        Shade::Unknown => GREY,
        Shade::Confirmed => GREEN,
        Shade::Trial => BLUE,
        Shade::NoAnswer => RED,
        Shade::RolledBack => YELLOW,
    }
}

#[cfg(test)]
mod tests;
