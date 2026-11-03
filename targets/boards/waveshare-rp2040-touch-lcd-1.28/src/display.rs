//! The round LCD (GC9A01A, 240x240) task of the 親機 relay: it draws the
//! home screen ([`crate::screen`]) from `dagon_mcu_rp2040::relay::state`
//! and sends whole frames to the panel.

use core::fmt::Write as _;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Text};
use heapless::String;

use dagon_mcu_rp2040::arm::step;
use dagon_mcu_rp2040::relay::state;
use dagon_mcu_rp2040::watchdog::Lease;

use crate::gc9a01::{Frame, Panel};
use crate::screen;
use crate::DisplayPins as Pins;

/// A small square at the top that toggles every second while the hub runs.
const HEARTBEAT_AT: Point = Point::new(117, 6);
const HEARTBEAT: Duration = Duration::from_secs(1);

/// The LCD task (lowest priority, thread mode). It alone owns SPI1 and the
/// panel pins. It draws into the RAM frame and sends whole frames by DMA,
/// so it never holds the CPU for long. The backlight goes on after the
/// first full frame. It redraws when [`state::REDRAW`] fires and toggles a
/// heartbeat square every second. The `diag` build first shows a test
/// pattern and the clock set-up for 3 s and a seconds counter.
/// `lease`: the display's guard task (thread mode, `Background`).
#[embassy_executor::task]
pub async fn task(mut pins: Pins, frame: &'static mut Frame, lease: Lease) {
    // A whole piece of drawing must end within the default work limit.
    let busy = || lease.watch.busy(lease.task);
    busy();
    let mut panel = Panel::new(pins.spi, pins.cs, pins.dc, pins.rst).await;
    let _ = frame.clear(Rgb565::BLACK);
    if cfg!(feature = "diag") {
        let _ = test_pattern(frame);
        let _ = clock_info(frame);
        panel.flush(frame).await;
        pins.backlight.set_high();
        lease.idle();
        Timer::after_secs(3).await;
        busy();
    }
    let mut shown = state::home();
    let _ = screen::draw(frame, &shown, dagon_mcu_rp2040::TEST_KEY);
    panel.flush(frame).await;
    pins.backlight.set_high();
    let mut beat = false;
    loop {
        lease.idle();
        let event = select(state::REDRAW.wait(), Timer::after(HEARTBEAT)).await;
        busy();
        if matches!(event, Either::Second(())) {
            state::expire_progress();
            beat = !beat;
        }
        let home = state::home();
        if home != shown {
            step(15);
            let _ = screen::draw(frame, &home, dagon_mcu_rp2040::TEST_KEY);
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
