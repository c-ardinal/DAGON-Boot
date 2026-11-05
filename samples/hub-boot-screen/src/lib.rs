//! A trusted boot extension (copied from targets/boot-extension-template)
//! that shows the boot core's steps on the hub's round LCD: a gauge of four
//! steps and a line of Japanese text, one step per boot event (watchdog
//! armed, log read, before decision, slot chosen). Nothing is drawn for
//! `boot.no_image` and `boot.halt`: the boot core's halt blink on the
//! backlight takes over. The app's display replaces the screen when the app
//! starts.
//!
//! As a trusted extension it takes what it drives itself: the panel's SPI
//! bus and pins and the microsecond timer straight from the HAL the boot
//! core runs on (the pins of the hub's board definition, see `hw`), and the
//! backlight through the MCU package's boot LED. Keeping it safe is the
//! extension's job: nothing else in the boot core uses these.
//!
//! No call waits for the screen to be seen. The panel needs its reset
//! released for a while before it takes commands, and the first three
//! events come within a few milliseconds of each other, so usually only
//! the last step's text shows, briefly, before the app starts. A call waits
//! for the panel only when the rest of that wait fits the call's budget
//! ([`wait_before_drawing`]); every call ends within [`DEADLINE_US`],
//! below the manifest's `max_run_us`. It changes nothing the boot core
//! decides.
#![cfg_attr(not(test), no_std)]

use dagon_boot::sequence;
use dagon_events::{Ctx, Event, Extension};

pub mod gc9a01;
mod glyphs;
pub mod texts;

/// What the boot core links: `boot_wiring.rs` names these two.
pub type DagonExtension = BootScreen;
pub const DAGON_EXTENSION: DagonExtension = BootScreen {
    lcd: None,
    panel: Panel::Off,
    shown: None,
};

/// The boot steps this extension shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Armed,
    LogRead,
    Checking,
    Chosen { trial: bool },
}

/// Segments of the gauge: one per boot event shown.
pub const STEPS: u8 = 4;

/// What the screen shows: the gauge's lit segments and the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub step: u8,
    pub text: &'static str,
}

pub fn screen(stage: Stage) -> Screen {
    let (step, text) = match stage {
        Stage::Armed => (1, texts::STARTING),
        Stage::LogRead => (2, texts::STARTING),
        Stage::Checking => (3, texts::CHECKING),
        Stage::Chosen { trial: false } => (4, texts::CONFIRMED),
        Stage::Chosen { trial: true } => (4, texts::TRIAL),
    };
    Screen { step, text }
}

/// The step of a boot event; `None` for the others (no image, halt, the
/// jump, the extension reports).
pub fn stage(e: &Event<'_>) -> Option<Stage> {
    if e.get::<sequence::WatchdogArmed>().is_some() {
        Some(Stage::Armed)
    } else if e.get::<sequence::LogRead>().is_some() {
        Some(Stage::LogRead)
    } else if e.get::<sequence::BeforeDecision>().is_some() {
        Some(Stage::Checking)
    } else {
        e.get::<sequence::SlotChosen>()
            .map(|c| Stage::Chosen { trial: c.trial })
    }
}

/// The panel is `SIZE` x `SIZE` pixels, a circle inscribed.
pub const SIZE: u16 = gc9a01::SIZE;
/// Glyph cell: the largest at which the longest text fits inside the circle
/// with 8 px to spare (the screen test checks both).
pub const CELL: u16 = 14;

/// A rectangle of panel pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// The longest text, in cells.
const TEXT_CELLS: u16 = 15;
/// The text line, as wide as the longest text, just above the centre.
pub const TEXT: Rect = Rect {
    x: (SIZE - TEXT_CELLS * CELL) / 2,
    y: 104,
    w: TEXT_CELLS * CELL,
    h: CELL,
};
pub const SEGMENT_W: u16 = 40;
pub const SEGMENT_GAP: u16 = 8;
const GAUGE_W: u16 = STEPS as u16 * SEGMENT_W + (STEPS as u16 - 1) * SEGMENT_GAP;
/// The gauge, 12 px below the text.
pub const GAUGE: Rect = Rect {
    x: (SIZE - GAUGE_W) / 2,
    y: TEXT.y + CELL + 12,
    w: GAUGE_W,
    h: 8,
};

// RGB565. The hub home screen's colours (the board package's
// src/screen.rs): black ground, white text, dim grey for what is not lit.
pub const BACKGROUND: u16 = 0x0000;
pub const INK: u16 = 0xFFFF;
pub const FILLED: u16 = 0xFFFF;
/// CSS dim grey (#696969).
pub const EMPTY: u16 = 0x6B4D;

/// One row (`row` from the top of [`TEXT`]) of `text`, centred, as RGB565
/// high byte first into `out` (`TEXT.w * 2` bytes).
pub fn text_row(text: &str, row: u16, out: &mut [u8]) {
    put(out, 0..TEXT.w, BACKGROUND);
    let n = text.chars().count() as u16;
    let x0 = (TEXT.w - n.min(TEXT_CELLS) * CELL) / 2;
    for (i, c) in text.chars().take(usize::from(TEXT_CELLS)).enumerate() {
        let Some((_, rows)) = glyphs::GLYPHS.iter().find(|g| g.0 == c) else {
            continue;
        };
        let bits = rows[usize::from(row)];
        let at = x0 + i as u16 * CELL;
        for col in 0..CELL {
            if bits >> (CELL - 1 - col) & 1 == 1 {
                put(out, at + col..at + col + 1, INK);
            }
        }
    }
}

/// One row of the gauge with `step` segments lit (`GAUGE.w * 2` bytes).
pub fn gauge_row(step: u8, _row: u16, out: &mut [u8]) {
    put(out, 0..GAUGE.w, BACKGROUND);
    for i in 0..u16::from(STEPS) {
        let at = i * (SEGMENT_W + SEGMENT_GAP);
        let c = if i < u16::from(step) { FILLED } else { EMPTY };
        put(out, at..at + SEGMENT_W, c);
    }
}

fn put(out: &mut [u8], xs: core::ops::Range<u16>, c: u16) {
    for x in xs {
        let i = usize::from(x) * 2;
        out[i..i + 2].copy_from_slice(&c.to_be_bytes());
    }
}

fn pixel(out: &[u8], x: u16) -> u16 {
    let i = usize::from(x) * 2;
    u16::from_be_bytes([out[i], out[i + 1]])
}

/// The colour of panel pixel (`x`, `y`) inside [`TEXT`] showing `text`.
pub fn text_rgb(text: &str, x: u16, y: u16) -> u16 {
    let mut row = [0; ROW_BYTES];
    text_row(text, y - TEXT.y, &mut row);
    pixel(&row, x - TEXT.x)
}

/// The colour of panel pixel (`x`, `y`) inside [`GAUGE`] at `step`.
pub fn gauge_rgb(step: u8, x: u16, y: u16) -> u16 {
    let mut row = [0; ROW_BYTES];
    gauge_row(step, y - GAUGE.y, &mut row);
    pixel(&row, x - GAUGE.x)
}

/// Bytes of the widest row drawn.
const ROW_BYTES: usize = TEXT.w as usize * 2;
const _: () = assert!(GAUGE.w <= TEXT.w);

/// After the reset line goes high, the panel takes commands only this much
/// later; after sleep out, the same before the next command. Both are the
/// figures of the hub board package's driver (src/gc9a01.rs, `Panel::new`);
/// the panel datasheet's own values are not verified here.
pub const RESET_RELEASE_US: u32 = 120_000;
pub const SLEEP_OUT_US: u32 = 120_000;
/// Every call returns by this time from its start: the manifest's
/// `max_run_us` (45 ms) less a 5 ms margin for the last row sent after the
/// check and the call's own overhead.
pub const DEADLINE_US: u32 = 40_000;
/// The longest a call waits for the panel: half the deadline, leaving the
/// rest for the commands and both areas (the budget test checks the sum).
pub const WAIT_MAX_US: u32 = 20_000;
/// The slowest time per byte sent: 8 bits at 15.625 MHz (the bus's 20 MHz
/// from the board definition, as the HAL divides the 125 MHz peripheral
/// clock) are 512 ns; the rest covers the HAL's byte-by-byte blocking write
/// and drawing the rows. An estimate, not measured on the board.
pub const NS_PER_BYTE_MAX: u32 = 1_000;

/// What the panel is ready for (times from the microsecond timer's low word).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// Not taken yet.
    Off,
    /// Its reset was released at `at`; not initialised.
    Released { at: u32 },
    /// Initialised, out of sleep since `at`.
    On { at: u32 },
}

/// How long a call at `now` waits before it may send the panel commands,
/// or `None` when it would wait longer than [`WAIT_MAX_US`] (the call then
/// draws nothing; a later one draws the screen it missed).
pub fn wait_before_drawing(panel: Panel, now: u32) -> Option<u32> {
    let ready = match panel {
        Panel::Off => return None,
        Panel::Released { at } => at.wrapping_add(RESET_RELEASE_US),
        Panel::On { at } => at.wrapping_add(SLEEP_OUT_US),
    };
    let rest = ready.wrapping_sub(now) as i32;
    match rest {
        r if r <= 0 => Some(0),
        r if r as u32 <= WAIT_MAX_US => Some(r as u32),
        _ => None,
    }
}

/// Bytes of the window commands and pixels of `r`.
fn area_bytes(r: Rect) -> u32 {
    gc9a01::WINDOW_BYTES + u32::from(r.w) * u32::from(r.h) * 2
}

/// The most bytes one call sends: the first, which initialises the panel
/// and draws both areas.
pub fn worst_call_bytes() -> u32 {
    gc9a01::INIT_BYTES + area_bytes(TEXT) + area_bytes(GAUGE) + gc9a01::WAKE_BYTES
}

pub struct BootScreen {
    lcd: Option<hw::Lcd>,
    panel: Panel,
    shown: Option<Screen>,
}

impl BootScreen {
    /// Sends the areas of `want` that differ from what the panel shows;
    /// false when the deadline cut it short.
    fn draw(lcd: &mut hw::Lcd, shown: Option<Screen>, want: Screen, start: u32) -> bool {
        let mut row = [0; ROW_BYTES];
        let text = shown.map(|s| s.text) != Some(want.text);
        let gauge = shown.map(|s| s.step) != Some(want.step);
        (!text || lcd.area(TEXT, start, &mut row, |y, out| text_row(want.text, y, out)))
            && (!gauge
                || lcd.area(GAUGE, start, &mut row, |y, out| {
                    gauge_row(want.step, y, out)
                }))
    }
}

impl Extension for BootScreen {
    fn id(&self) -> &'static str {
        // The manifest's id.
        "boot-screen"
    }

    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        let Some(stage) = stage(e) else { return };
        let start = hw::now_us();
        if self.panel == Panel::Off {
            // Taking the pins releases the panel's reset.
            self.lcd = hw::Lcd::take();
            if self.lcd.is_none() {
                return;
            }
            self.panel = Panel::Released { at: start };
        }
        let want = screen(stage);
        let Some(lcd) = self.lcd.as_mut() else { return };
        if self.shown == Some(want) {
            return;
        }
        let Some(wait) = wait_before_drawing(self.panel, start) else {
            return;
        };
        hw::wait_us(wait);
        let first = matches!(self.panel, Panel::Released { .. });
        if first {
            lcd.init();
            self.shown = None;
        }
        let done = Self::draw(lcd, self.shown, want, start);
        if first {
            lcd.wake();
            self.panel = Panel::On { at: hw::now_us() };
        }
        self.shown = done.then_some(want);
    }
}

#[cfg(target_os = "none")]
mod hw {
    //! The panel's pins and bus, taken from the HAL. The pin numbers are
    //! the hub board definition's
    //! (targets/boards/waveshare-rp2040-touch-lcd-1.28/dagon-board.toml):
    //! lcd_cs GPIO9 (line 44), lcd_dc GPIO8 (line 48), lcd_rst GPIO13
    //! (line 52), bus `lcd` on spi1 with sck GPIO10, mosi GPIO11 at
    //! 20 MHz (lines 72-75); the backlight is its `[boot] led` (line 35).

    use dagon_mcu_rp2040::arm::led;
    use embassy_rp::gpio::{Level, Output};
    use embassy_rp::pac;
    use embassy_rp::peripherals::{PIN_10, PIN_11, PIN_13, PIN_8, PIN_9, SPI1};
    use embassy_rp::spi::{Blocking, Config, Spi};

    use super::{gc9a01, Rect, DEADLINE_US};

    /// Bus `lcd`'s clock (board definition line 75).
    const LCD_HZ: u32 = 20_000_000;

    /// Microseconds from the TIMER's low word (RP2040 datasheet 4.6), which
    /// counts from the boot core's HAL init.
    pub fn now_us() -> u32 {
        pac::TIMER.timerawl().read()
    }

    pub fn wait_us(us: u32) {
        let start = now_us();
        while now_us().wrapping_sub(start) < us {}
    }

    pub struct Lcd {
        spi: Spi<'static, SPI1, Blocking>,
        cs: Output<'static>,
        dc: Output<'static>,
        /// Held high while the boot core runs: a dropped pin falls back to
        /// the pad's pull-down and resets the panel.
        _rst: Output<'static>,
    }

    impl Lcd {
        /// Takes the bus and pins, releasing the panel's reset (low since
        /// the chip's reset, through the pad's pull-down).
        pub fn take() -> Option<Lcd> {
            // SAFETY: the boot core uses none of these (it takes the flash
            // and the watchdog), the boot core's HAL init has run, and this
            // extension takes each once (BootScreen.panel leaves Off once).
            let (spi1, sck, mosi, cs, dc, rst) = unsafe {
                (
                    SPI1::steal(),
                    PIN_10::steal(),
                    PIN_11::steal(),
                    PIN_9::steal(),
                    PIN_8::steal(),
                    PIN_13::steal(),
                )
            };
            let mut cfg = Config::default();
            cfg.frequency = LCD_HZ;
            Some(Lcd {
                spi: Spi::new_blocking_txonly(spi1, sck, mosi, cfg),
                cs: Output::new(cs, Level::High),
                dc: Output::new(dc, Level::High),
                _rst: Output::new(rst, Level::High),
            })
        }

        /// One command and its parameters in one CS frame, DC low for the
        /// command byte.
        fn command(&mut self, cmd: u8, args: &[u8]) {
            self.cs.set_low();
            self.dc.set_low();
            let _ = self.spi.blocking_write(&[cmd]);
            self.dc.set_high();
            if !args.is_empty() {
                let _ = self.spi.blocking_write(args);
            }
            self.cs.set_high();
        }

        pub fn init(&mut self) {
            for &(cmd, args) in gc9a01::INIT {
                self.command(cmd, args);
            }
        }

        /// Display on, then sleep out (the panel shows its memory once it
        /// is awake; no command follows in this call), then the backlight.
        pub fn wake(&mut self) {
            self.command(gc9a01::DISPON, &[]);
            self.command(gc9a01::SLPOUT, &[]);
            if let Some(gpio) = led::selected() {
                led::init(gpio, led::active_high());
            }
        }

        /// Sends `r` row by row, each drawn by `row` into `buf`; stops
        /// before a row once the call is at its deadline (false).
        pub fn area(
            &mut self,
            r: Rect,
            start: u32,
            buf: &mut [u8],
            row: impl Fn(u16, &mut [u8]),
        ) -> bool {
            let [x0, x1] = [r.x, r.x + r.w - 1].map(u16::to_be_bytes);
            let [y0, y1] = [r.y, r.y + r.h - 1].map(u16::to_be_bytes);
            self.command(gc9a01::CASET, &[x0[0], x0[1], x1[0], x1[1]]);
            self.command(gc9a01::RASET, &[y0[0], y0[1], y1[0], y1[1]]);
            self.cs.set_low();
            self.dc.set_low();
            let _ = self.spi.blocking_write(&[gc9a01::RAMWR]);
            self.dc.set_high();
            let out = &mut buf[..usize::from(r.w) * 2];
            let mut done = true;
            for y in 0..r.h {
                if now_us().wrapping_sub(start) >= DEADLINE_US {
                    done = false;
                    break;
                }
                row(y, out);
                let _ = self.spi.blocking_write(out);
            }
            self.cs.set_high();
            done
        }
    }
}

/// Host builds (tests) drive no panel.
#[cfg(not(target_os = "none"))]
mod hw {
    use super::Rect;

    pub fn now_us() -> u32 {
        0
    }

    pub fn wait_us(_us: u32) {}

    pub struct Lcd;

    impl Lcd {
        pub fn take() -> Option<Lcd> {
            None
        }

        pub fn init(&mut self) {}

        pub fn wake(&mut self) {}

        pub fn area(
            &mut self,
            _r: Rect,
            _start: u32,
            _buf: &mut [u8],
            _row: impl Fn(u16, &mut [u8]),
        ) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests;
