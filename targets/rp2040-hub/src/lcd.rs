//! Round LCD (GC9A01A, 240x240) home screen: the hub in the centre, the sub
//! nodes below it, each with its version and a state colour. While the PC
//! sends SHOW, the named node's ring becomes an arc that grows with the
//! percentage in the phase colour, with a line of text on top.

use core::fmt::Write as _;

use dagon_format::Version;
use dagon_proto::payload::{phase, Show, SHOW_TEXT_MAX};
use embassy_futures::select::{select, Either};
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Async, Spi};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Arc, Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Text};
use heapless::String;

use rp2040_common::arm::step;

use crate::gc9a01::{Frame, Panel};
use crate::nodes::{SubNode, MAX_SUBS};
use crate::state;
use crate::wdt::{self, Task};

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
/// Height of the small state line under a node's version.
const STATE_LINE: i32 = 10;
const ARC: u32 = 9;
/// Top text line; the round panel is narrower up there.
const TOP_AT: Point = Point::new(120, 22);
const TOP_CHARS: usize = 22;
/// "TEST KEY" marker near the bottom (above the diag seconds counter).
const TEST_KEY_AT: Point = Point::new(120, 218);
/// A small square at the top that toggles every second while the hub runs.
const HEARTBEAT_AT: Point = Point::new(117, 6);
const HEARTBEAT: Duration = Duration::from_secs(1);

pub struct Pins {
    pub spi: Spi<'static, SPI1, Async>,
    pub cs: Output<'static>,
    pub dc: Output<'static>,
    pub rst: Output<'static>,
    pub backlight: Output<'static>,
}

/// The LCD task (lowest priority, thread mode). It alone owns SPI1 and the
/// panel pins. It draws into the RAM frame and sends whole frames by DMA,
/// so it never holds the CPU for long. The backlight goes on after the
/// first full frame. It redraws when [`state::REDRAW`] fires and toggles a
/// heartbeat square every second. The `diag` build first shows a test
/// pattern and the clock set-up for 3 s and a seconds counter.
#[embassy_executor::task]
pub async fn task(mut pins: Pins, frame: &'static mut Frame) {
    wdt::busy(Task::Lcd);
    let mut panel = Panel::new(pins.spi, pins.cs, pins.dc, pins.rst).await;
    let _ = frame.clear(Rgb565::BLACK);
    if cfg!(feature = "diag") {
        let _ = test_pattern(frame);
        let _ = clock_info(frame);
        panel.flush(frame).await;
        pins.backlight.set_high();
        wdt::idle(Task::Lcd);
        Timer::after_secs(3).await;
        wdt::busy(Task::Lcd);
    }
    let mut shown = state::home();
    let _ = draw(frame, &shown);
    panel.flush(frame).await;
    pins.backlight.set_high();
    let mut beat = false;
    loop {
        wdt::idle(Task::Lcd);
        let event = select(state::REDRAW.wait(), Timer::after(HEARTBEAT)).await;
        wdt::busy(Task::Lcd);
        if matches!(event, Either::Second(())) {
            state::expire_progress();
            beat = !beat;
        }
        let home = state::home();
        if home != shown {
            step(15);
            let _ = draw(frame, &home);
            shown = home;
        }
        let c = if beat {
            Rgb565::CSS_GRAY
        } else {
            Rgb565::BLACK
        };
        let _ = Rectangle::new(HEARTBEAT_AT, Size::new(6, 6))
            .into_styled(PrimitiveStyle::with_fill(c))
            .draw(frame);
        if cfg!(feature = "diag") {
            let _ = uptime(frame);
        }
        panel.flush(frame).await;
    }
}

/// Which drawing paths land: red/green/blue bands (large solid fills), a
/// white square in the middle (small solid fill), a white line of single
/// pixels below it (per-pixel writes), the word DIAG (glyph streams) and a
/// yellow ring (circle).
fn test_pattern<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> Result<(), D::Error> {
    let bands = [(40, Rgb565::RED), (80, Rgb565::GREEN), (120, Rgb565::BLUE)];
    for (y, c) in bands {
        Rectangle::new(Point::new(0, y), Size::new(240, 40))
            .into_styled(PrimitiveStyle::with_fill(c))
            .draw(d)?;
    }
    Rectangle::new(Point::new(110, 90), Size::new(20, 20))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::WHITE))
        .draw(d)?;
    d.draw_iter((70..170).map(|x| Pixel(Point::new(x, 170), Rgb565::WHITE)))?;
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    Text::with_alignment("DIAG", Point::new(120, 200), style, Alignment::Center).draw(d)?;
    Circle::with_center(Point::new(120, 120), 200)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::YELLOW, 4))
        .draw(d)?;
    Ok(())
}

/// Clock set-up as the app sees it: the watchdog tick register (CYCLES in
/// bits 0-8, ENABLE bit 9, RUNNING bit 10) and the HAL's clock rates.
fn clock_info<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> Result<(), D::Error> {
    use embassy_rp::clocks;
    const WATCHDOG_TICK: *const u32 = 0x4005_802C as *const u32;
    // SAFETY: read-only access to a watchdog register.
    let tick = unsafe { WATCHDOG_TICK.read_volatile() };
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let mut line: String<40> = String::new();
    let rows = [
        ("tick", tick),
        ("ref", clocks::clk_ref_freq()),
        ("sys", clocks::clk_sys_freq()),
        ("xosc", clocks::xosc_freq()),
    ];
    for (i, (name, v)) in rows.iter().enumerate() {
        line.clear();
        let _ = write!(line, "{name}={v:#x} ({v})");
        let at = Point::new(120, 20 + 12 * i as i32);
        Rectangle::new(at - Point::new(100, 9), Size::new(200, 12))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(d)?;
        Text::with_alignment(&line, at, style, Alignment::Center).draw(d)?;
    }
    Ok(())
}

/// Seconds since the app started by the HAL clock; compare with a watch.
fn uptime<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> Result<(), D::Error> {
    let mut line: String<16> = String::new();
    let _ = write!(line, "t={}s", embassy_time::Instant::now().as_secs());
    let at = Point::new(120, 236);
    Rectangle::new(at - Point::new(30, 9), Size::new(60, 12))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(d)?;
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    Text::with_alignment(&line, at, style, Alignment::Center).draw(d)?;
    Ok(())
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
    if crate::TEST_KEY {
        // Built with the public test key: anyone can sign images for it.
        let style = MonoTextStyle::new(&FONT_10X20, RED);
        Text::with_alignment("TEST KEY", TEST_KEY_AT, style, Alignment::Center).draw(d)?;
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
    // Name, version, then the state in words so colour is never the only
    // cue.
    Text::with_alignment(
        n.node.name(),
        at - Point::new(0, line / 2 + STATE_LINE / 2),
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
        at + Point::new(0, line / 2 + 2 - STATE_LINE / 2),
        style,
        Alignment::Center,
    )
    .draw(d)?;
    let mut word: String<8> = String::new();
    let c = match shown {
        Some(p) => {
            match p.phase {
                phase::TRANSFER => {
                    let _ = write!(word, "{}%", p.percent);
                }
                _ => {
                    let _ = word.push_str(short_label(p.phase));
                }
            }
            phase_colour(p.phase)
        }
        None => {
            let _ = word.push_str(state_word(n.shade));
            colour(n.shade)
        }
    };
    let small = MonoTextStyle::new(&FONT_6X10, c);
    Text::with_alignment(
        &word,
        at + Point::new(0, line / 2 + 2 + STATE_LINE / 2 + 2),
        small,
        Alignment::Center,
    )
    .draw(d)?;
    Ok(())
}

/// At most 6 characters: fits under the version in a small node ring.
fn short_label(ph: u8) -> &'static str {
    match ph {
        phase::VERIFY => "verify",
        phase::REBOOT => "reboot",
        phase::CONFIRM => "ok",
        phase::REJECTED => "reject",
        phase::FAILED => "failed",
        phase::ROM => "ROM",
        _ => "",
    }
}

/// At most 6 characters, as [`short_label`].
fn state_word(s: Shade) -> &'static str {
    match s {
        Shade::Unknown => "...",
        Shade::Confirmed => "ok",
        Shade::Trial => "trial",
        Shade::NoAnswer => "silent",
    }
}

/// The five LCD state colours (green confirmed, blue transfer/trial,
/// yellow rolled back, red rejected/no answer, purple ROM path), lightened
/// where needed so each keeps at least 4.5:1 against the black background
/// (pure blue and dark purple do not).
const GREEN: Rgb565 = Rgb565::new(0, 56, 10); // about #00E052
const BLUE: Rgb565 = Rgb565::new(8, 40, 31); // about #42A2FF
const YELLOW: Rgb565 = Rgb565::new(31, 56, 0); // about #FFE000
const RED: Rgb565 = Rgb565::new(31, 12, 8); // about #FF3142
const PURPLE: Rgb565 = Rgb565::new(24, 28, 31); // about #C672FF
const GREY: Rgb565 = Rgb565::new(18, 37, 18); // about #949594

fn colour(s: Shade) -> Rgb565 {
    match s {
        Shade::Unknown => GREY,
        Shade::Confirmed => GREEN,
        Shade::Trial => BLUE,
        Shade::NoAnswer => RED,
    }
}
