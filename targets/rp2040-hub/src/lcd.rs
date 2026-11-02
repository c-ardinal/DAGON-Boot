//! Round LCD (GC9A01A, 240x240) home screen: the hub in the centre, the sub
//! nodes below it, each with its version and a state colour. While the PC
//! sends SHOW, the named node's ring becomes an arc that grows with the
//! percentage in the phase colour, with a line of text on top.

use core::fmt::Write as _;

use dagon_format::Version;
use dagon_proto::payload::{phase, Show, SHOW_TEXT_MAX};
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Blocking, Spi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Arc, Circle, PrimitiveStyle};
use embedded_graphics::text::{Alignment, Text};
use heapless::String;

use crate::gc9a01::Panel;
use crate::nodes::{SubNode, MAX_SUBS};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shade {
    #[default]
    Unknown,
    Confirmed,
    Trial,
    NoAnswer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub node: SubNode,
    pub version: Option<Version>,
    pub shade: Shade,
}

/// The last SHOW.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub node: u8,
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
    /// Printable ASCII of the SHOW text (the font has nothing else).
    pub text: String<SHOW_TEXT_MAX>,
}

impl From<&Show<'_>> for Progress {
    fn from(s: &Show<'_>) -> Self {
        let mut text = String::new();
        if s.text.iter().all(|b| (0x20..0x7F).contains(b)) {
            s.text.iter().for_each(|&b| {
                let _ = text.push(b as char);
            });
        }
        Progress {
            node: s.node,
            phase: s.phase,
            percent: s.percent.min(100),
            version: s.version,
            text,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    pub hub: Node,
    pub subs: heapless::Vec<Node, MAX_SUBS>,
    pub progress: Option<Progress>,
}

pub static HOME: Signal<CriticalSectionRawMutex, Home> = Signal::new();

const HUB_AT: Point = Point::new(120, 80);
const HUB_D: u32 = 90;
const LEFT: Point = Point::new(55, 160);
const BOTTOM: Point = Point::new(120, 190);
const RIGHT: Point = Point::new(185, 160);
const SUB_D: u32 = 60;

/// Sub node positions by how many there are.
fn sub_at(count: usize) -> &'static [Point] {
    match count {
        0 => &[],
        1 => &[BOTTOM],
        2 => &[LEFT, RIGHT],
        _ => &[LEFT, BOTTOM, RIGHT],
    }
}

const RING: u32 = 5;
const ARC: u32 = 9;
/// Top text line; the round panel is narrower up there.
const TOP_AT: Point = Point::new(120, 22);
const TOP_CHARS: usize = 22;

pub struct Pins<'d> {
    pub spi: Spi<'d, SPI1, Blocking>,
    pub cs: Output<'d>,
    pub dc: Output<'d>,
    pub rst: Output<'d>,
}

/// Sets the panel up (cleared to black), then draws the home screen
/// whenever [`HOME`] changes.
pub async fn run(pins: Pins<'_>) {
    let mut display = Panel::new(pins.spi, pins.cs, pins.dc, pins.rst);
    let mut shown: Option<Home> = None;
    loop {
        let home = HOME.wait().await;
        if shown.as_ref() != Some(&home) {
            let _ = draw(&mut display, &home);
            shown = Some(home);
        }
    }
}

fn draw<D: DrawTarget<Color = Rgb565>>(d: &mut D, home: &Home) -> Result<(), D::Error> {
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
    Ok(())
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
        phase::CONFIRM => Rgb565::GREEN,
        phase::REJECTED => Rgb565::RED,
        phase::FAILED => Rgb565::YELLOW,
        phase::ROM => Rgb565::CSS_PURPLE,
        _ => Rgb565::BLUE,
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
    Text::with_alignment(
        n.node.name(),
        at - Point::new(0, line / 2),
        style,
        Alignment::Center,
    )
    .draw(d)?;
    let mut v: String<16> = String::new();
    match shown.map(|p| p.version).or(n.version) {
        Some(ver) => {
            let _ = write!(v, "{ver}");
        }
        None => {
            let _ = v.push_str("--");
        }
    }
    Text::with_alignment(
        &v,
        at + Point::new(0, line / 2 + 2),
        style,
        Alignment::Center,
    )
    .draw(d)?;
    Ok(())
}

fn colour(s: Shade) -> Rgb565 {
    match s {
        Shade::Unknown => Rgb565::CSS_GRAY,
        Shade::Confirmed => Rgb565::GREEN,
        Shade::Trial => Rgb565::BLUE,
        Shade::NoAnswer => Rgb565::RED,
    }
}
