//! Round LCD (GC9A01A, 240x240) home screen: the hub in the centre, the sub
//! nodes below it, each with its version and a state colour.

use core::fmt::Write as _;

use dagon_format::Version;
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Blocking, Spi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Delay;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle};
use embedded_graphics::text::{Alignment, Text};
use embedded_hal_bus::spi::ExclusiveDevice;
use heapless::String;
use mipidsi::interface::SpiInterface;
use mipidsi::models::GC9A01;
use mipidsi::options::ColorInversion;

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    pub hub: Node,
    pub subs: heapless::Vec<Node, MAX_SUBS>,
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

pub struct Pins<'d> {
    pub spi: Spi<'d, SPI1, Blocking>,
    pub cs: Output<'d>,
    pub dc: Output<'d>,
    pub rst: Output<'d>,
}

/// Draws the home screen whenever [`HOME`] changes. Returns only if the
/// panel cannot be set up (the hub keeps working without it).
pub async fn run(pins: Pins<'_>) {
    let dev = ExclusiveDevice::new_no_delay(pins.spi, pins.cs).unwrap_or_else(|e| match e {});
    let mut buf = [0u8; 512];
    let di = SpiInterface::new(dev, pins.dc, &mut buf);
    // Calibration knobs if colours look wrong on the panel: inversion and
    // `.color_order(ColorOrder::Bgr)`.
    let Ok(mut display) = mipidsi::Builder::new(GC9A01, di)
        .reset_pin(pins.rst)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Delay)
    else {
        return;
    };
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
    node(d, HUB_AT, HUB_D, &home.hub, &FONT_10X20)?;
    for (&at, n) in sub_at(home.subs.len()).iter().zip(&home.subs) {
        node(d, at, SUB_D, n, &FONT_6X10)?;
    }
    Ok(())
}

fn node<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    at: Point,
    diameter: u32,
    n: &Node,
    font: &MonoFont<'_>,
) -> Result<(), D::Error> {
    Circle::with_center(at, diameter)
        .into_styled(PrimitiveStyle::with_stroke(colour(n.shade), RING))
        .draw(d)?;
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
    match n.version {
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
