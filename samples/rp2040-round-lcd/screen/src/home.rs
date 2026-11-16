//! The app's home screen (240x240), drawn onto any `embedded-graphics`
//! target so the layout runs in host tests. Everything on it comes from
//! what the app has: this node's name, version and state from the events
//! DAGON hands the app, and the nodes its links reach from the generated
//! wiring.rs.
//!
//! This node sits at the top in a ring in its state's colour, its name and
//! version inside, its state in Japanese below, so colour is never the only
//! cue. While a new image is being written to it, the ring becomes an arc
//! that grows with the write's progress, with the percentage above it. Up
//! to three of the nodes its links reach sit below, each in a grey ring
//! with its name and the port the link runs on. A build with the public
//! test key shows "TEST KEY" at the bottom, clear of every ring.

use core::fmt::Write as _;

use dagon_format::Version;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Arc, Circle, PrimitiveStyle};
use embedded_graphics::text::{Alignment, Text};
use heapless::String;

use crate::{glyph, texts, CELL};

/// This node's state, as the app learns it from DAGON's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not known yet (DAGON has not opened the slot store).
    Starting,
    Confirmed,
    Writing,
    Verifying,
    Written,
    Pending,
    Rebooting,
    Trial,
    RolledBack,
    Failed,
}

impl Status {
    pub const ALL: [Status; 10] = [
        Status::Starting,
        Status::Confirmed,
        Status::Writing,
        Status::Verifying,
        Status::Written,
        Status::Pending,
        Status::Rebooting,
        Status::Trial,
        Status::RolledBack,
        Status::Failed,
    ];

    /// Its word below the ring.
    pub fn word(self) -> &'static str {
        match self {
            Status::Starting => texts::HOME_STARTING,
            Status::Confirmed => texts::HOME_CONFIRMED,
            Status::Writing => texts::HOME_WRITING,
            Status::Verifying => texts::HOME_VERIFYING,
            Status::Written => texts::HOME_WRITTEN,
            Status::Pending => texts::HOME_PENDING,
            Status::Rebooting => texts::HOME_REBOOTING,
            Status::Trial => texts::HOME_TRIAL,
            Status::RolledBack => texts::HOME_ROLLED_BACK,
            Status::Failed => texts::HOME_FAILED,
        }
    }

    /// Its colour: green confirmed, blue while something is under way or on
    /// trial, yellow rolled back, red failed, grey not known yet.
    pub fn colour(self) -> Rgb565 {
        match self {
            Status::Starting => GREY,
            Status::Confirmed => GREEN,
            Status::RolledBack => YELLOW,
            Status::Failed => RED,
            _ => BLUE,
        }
    }
}

/// A node one of this node's links reaches (from wiring.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer<'a> {
    pub name: &'a str,
    /// This node's port the link runs on.
    pub link: &'a str,
}

/// Everything the home screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Home<'a> {
    pub name: &'a str,
    /// The running image's version.
    pub version: Version,
    pub status: Status,
    /// How far the write is, 0 to 100, while one is under way.
    pub percent: Option<u8>,
    pub peers: &'a [Peer<'a>],
    /// The firmware carries the public test key.
    pub test_key: bool,
}

pub const OWN_AT: Point = Point::new(120, 70);
pub const OWN_D: u32 = 90;
pub const LEFT: Point = Point::new(50, 160);
pub const BOTTOM: Point = Point::new(120, 172);
pub const RIGHT: Point = Point::new(190, 160);
pub const PEER_D: u32 = 56;
pub const RING: u32 = 5;
const ARC: u32 = 9;
/// The state word's top row: below this node's ring with a gap.
pub const WORD_TOP: i32 = OWN_AT.y + (OWN_D + RING) as i32 / 2 + 4;
/// The percentage's baseline, above this node's ring.
pub const PERCENT_AT: Point = Point::new(120, 22);
/// "TEST KEY" baseline: below the lowest ring and inside the round panel.
pub const TEST_KEY_AT: Point = Point::new(120, 226);

/// Where the peers' rings go, by how many there are; peers past the third
/// are not shown.
pub fn peers_at(count: usize) -> &'static [Point] {
    match count {
        0 => &[],
        1 => &[BOTTOM],
        2 => &[LEFT, RIGHT],
        _ => &[LEFT, BOTTOM, RIGHT],
    }
}

/// Draws `home` over the whole of `d`.
pub fn draw<D: DrawTarget<Color = Rgb565>>(d: &mut D, home: &Home<'_>) -> Result<(), D::Error> {
    d.clear(Rgb565::BLACK)?;
    let white = |font| MonoTextStyle::new(font, Rgb565::WHITE);
    let writing = home.percent.filter(|_| home.status == Status::Writing);
    match writing {
        Some(p) => {
            ring(d, OWN_AT, OWN_D, DIM)?;
            let sweep = f32::from(p.min(100)) * 3.6;
            Arc::with_center(OWN_AT, OWN_D, (-90.0f32).deg(), sweep.deg())
                .into_styled(PrimitiveStyle::with_stroke(BLUE, ARC))
                .draw(d)?;
            let mut line: String<8> = String::new();
            let _ = write!(line, "{}%", p.min(100));
            Text::with_alignment(&line, PERCENT_AT, white(&FONT_10X20), Alignment::Center)
                .draw(d)?;
        }
        None => ring(d, OWN_AT, OWN_D, home.status.colour())?,
    }
    let mut version: String<16> = String::new();
    let _ = write!(version, "{}", home.version);
    two_lines(d, OWN_AT, home.name, &version, &FONT_10X20)?;
    let word = home.status.word();
    let top = Point::new(OWN_AT.x, WORD_TOP);
    d.draw_iter(word_pixels(word, top).map(|p| Pixel(p, home.status.colour())))?;
    for (&at, peer) in peers_at(home.peers.len()).iter().zip(home.peers) {
        ring(d, at, PEER_D, GREY)?;
        two_lines(d, at, peer.name, peer.link, &FONT_6X10)?;
    }
    if home.test_key {
        // Built with the public test key: anyone can sign images for it.
        let style = MonoTextStyle::new(&FONT_10X20, RED);
        Text::with_alignment("TEST KEY", TEST_KEY_AT, style, Alignment::Center).draw(d)?;
    }
    Ok(())
}

fn ring<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    at: Point,
    diameter: u32,
    c: Rgb565,
) -> Result<(), D::Error> {
    Circle::with_center(at, diameter)
        .into_styled(PrimitiveStyle::with_stroke(c, RING))
        .draw(d)
}

/// `top` above the centre `at` and `bottom` below it, in white.
fn two_lines<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    at: Point,
    top: &str,
    bottom: &str,
    font: &MonoFont<'_>,
) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(font, Rgb565::WHITE);
    let line = font.character_size.height as i32;
    Text::with_alignment(
        top,
        at - Point::new(0, line / 2 + 2),
        style,
        Alignment::Center,
    )
    .draw(d)?;
    Text::with_alignment(
        bottom,
        at + Point::new(0, line / 2 - 2),
        style,
        Alignment::Center,
    )
    .draw(d)?;
    Ok(())
}

/// The lit pixels of `word` (characters of [`texts`]) centred on `top`,
/// its first row.
pub fn word_pixels(word: &str, top: Point) -> impl Iterator<Item = Point> + '_ {
    let cell = i32::from(CELL);
    let left = top.x - word.chars().count() as i32 * cell / 2;
    word.chars().enumerate().flat_map(move |(i, c)| {
        let rows = glyph(c);
        let x0 = left + i as i32 * cell;
        (0..cell).flat_map(move |dy| {
            (0..cell).filter_map(move |dx| {
                let row = rows?[dy as usize];
                (row & (1 << (cell - 1 - dx)) != 0).then_some(Point::new(x0 + dx, top.y + dy))
            })
        })
    })
}

/// The state colours, lightened where needed so each keeps at least 4.5:1
/// against the black background (pure blue does not).
pub const GREEN: Rgb565 = Rgb565::new(0, 56, 10); // about #00E052
pub const BLUE: Rgb565 = Rgb565::new(8, 40, 31); // about #42A2FF
pub const YELLOW: Rgb565 = Rgb565::new(31, 56, 0); // about #FFE000
pub const RED: Rgb565 = Rgb565::new(31, 12, 8); // about #FF3142
pub const GREY: Rgb565 = Rgb565::new(18, 37, 18); // about #949594
/// What the progress arc has not covered yet.
const DIM: Rgb565 = Rgb565::CSS_DIM_GRAY;

#[cfg(test)]
mod tests;
