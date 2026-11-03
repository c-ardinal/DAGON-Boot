//! Display task: owns the CO5300 AMOLED (QSPI on SPI2) and shows the running
//! version.
//!
//! The panel's power and reset sit on the internal I2C expander; `main` runs
//! [`crate::power::panel_power_on`] before DAGON takes that bus and passes
//! the result here. The command framing, init table, clock and window are
//! those of M5Stack's M5StopWatch-UserDemo V0.5 (M5GFX `Panel_AMOLED`). This
//! task runs in thread mode, below the DAGON link task, and announces a
//! liveness budget for each blocking step.

use dagon_esp32s3_app::digits::{self, Rect, MAX_CHARS, MAX_RECTS};
use dagon_mcu_esp32s3::liveness::Lease;
use dagon_mcu_esp32s3::{node, Version};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Pull},
    peripherals::{GPIO38, GPIO39, GPIO40, GPIO41, GPIO42, GPIO45, GPIO46, SPI2},
    spi::{
        master::{Address, Command, Config as SpiConfig, DataMode, Spi},
        Error as SpiError, Mode,
    },
    time::Rate,
    Blocking,
};

/// The AMOLED's pins.
pub struct DisplayPins {
    pub spi2: SPI2<'static>,
    pub cs: GPIO39<'static>,
    pub sck: GPIO40<'static>,
    pub d0: GPIO41<'static>,
    pub d1: GPIO42<'static>,
    pub d2: GPIO46<'static>,
    pub d3: GPIO45<'static>,
    pub te: GPIO38<'static>,
}

/// Visible area: 466 x 466, 6 columns into the CO5300's memory. The clear
/// covers 468 columns, as the official firmware does.
const COLUMN_OFFSET: u16 = 6;
const CLEAR_WIDTH: u16 = 468;
const SPI_MHZ: u32 = 80;
/// Bytes per pixel transaction.
const CHUNK: usize = 1024;

/// Opcodes: register write (single line), pixel write (quad data).
const OP_COMMAND: u8 = 0x02;
const OP_PIXELS: u8 = 0x32;
const SLPOUT: u8 = 0x11;
const SLPOUT_WAIT_MS: u32 = 150;
const CASET: u8 = 0x2A;
const RASET: u8 = 0x2B;
const RAMWR: u8 = 0x2C;
const RAMWRC: u8 = 0x3C;
/// After sleep-out: the official init table, then pixel format RGB565 and
/// rotation 0 as M5GFX sets them, then a moderate brightness: the digits are
/// the only lit pixels.
const INIT: [(u8, &[u8]); 11] = [
    (0xC4, &[0x80]),
    (0x35, &[0x80]),
    (0x44, &[0x01, 0xD2]),
    (0x53, &[0x20]),
    (0x20, &[]),
    (0x36, &[0x00]),
    (0x51, &[0xA0]),
    (0x29, &[]),
    (0x3A, &[0x55]),
    (0x36, &[0x00]),
    (0x51, &[BRIGHTNESS]),
];
const BRIGHTNESS: u8 = 0x60;

/// Panel initialisation, including the sleep-out wait.
const INIT_BUDGET_MS: u32 = 1_000;
/// One full redraw (clear plus the digits).
const DRAW_BUDGET_MS: u32 = 1_000;
/// How long to wait for DAGON to publish the running version.
const VERSION_WAIT: Duration = Duration::from_secs(1);
const VERSION_POLL: Duration = Duration::from_millis(20);

struct Panel {
    spi: Spi<'static, Blocking>,
    /// Not used; the official firmware keeps it pulled up.
    _te: Input<'static>,
}

impl Panel {
    fn open(p: DisplayPins) -> Option<Self> {
        let config = SpiConfig::default()
            .with_frequency(Rate::from_mhz(SPI_MHZ))
            .with_mode(Mode::_0);
        let spi = Spi::new(p.spi2, config)
            .ok()?
            .with_sck(p.sck)
            .with_cs(p.cs)
            .with_sio0(p.d0)
            .with_sio1(p.d1)
            .with_sio2(p.d2)
            .with_sio3(p.d3);
        let _te = Input::new(p.te, InputConfig::default().with_pull(Pull::Up));
        Some(Self { spi, _te })
    }

    /// `02 00 <reg> 00` and the arguments, single line, one CS cycle.
    fn command(&mut self, reg: u8, data: &[u8]) -> Result<(), SpiError> {
        self.spi.half_duplex_write(
            DataMode::Single,
            Command::_8Bit(OP_COMMAND.into(), DataMode::Single),
            Address::_24Bit(u32::from(reg) << 8, DataMode::Single),
            0,
            data,
        )
    }

    fn init(&mut self) -> Result<(), SpiError> {
        self.command(SLPOUT, &[])?;
        Delay::new().delay_millis(SLPOUT_WAIT_MS);
        INIT.iter()
            .try_for_each(|&(reg, data)| self.command(reg, data))
    }

    /// Fills a rectangle of the visible area (even origin and size).
    fn fill(&mut self, r: Rect, color: u16) -> Result<(), SpiError> {
        let x = r.x + COLUMN_OFFSET;
        let [x0h, x0l] = x.to_be_bytes();
        let [x1h, x1l] = (x + r.w - 1).to_be_bytes();
        let [y0h, y0l] = r.y.to_be_bytes();
        let [y1h, y1l] = (r.y + r.h - 1).to_be_bytes();
        self.command(CASET, &[x0h, x0l, x1h, x1l])?;
        self.command(RASET, &[y0h, y0l, y1h, y1l])?;
        let mut buf = [0u8; CHUNK];
        for px in buf.chunks_exact_mut(2) {
            px.copy_from_slice(&color.to_be_bytes());
        }
        let total = usize::from(r.w) * usize::from(r.h) * 2;
        let mut sent = 0;
        while sent < total {
            let n = CHUNK.min(total - sent);
            let reg = if sent == 0 { RAMWR } else { RAMWRC };
            self.spi.half_duplex_write(
                DataMode::Quad,
                Command::_8Bit(OP_PIXELS.into(), DataMode::Single),
                Address::_24Bit(u32::from(reg) << 8, DataMode::Single),
                0,
                &buf[..n],
            )?;
            sent += n;
        }
        Ok(())
    }

    fn draw(&mut self, rects: &[Rect]) -> Result<(), SpiError> {
        // The clear starts at column 0 of the CO5300 memory: 468 wide.
        let clear = Rect {
            x: 0,
            y: 0,
            w: CLEAR_WIDTH,
            h: digits::SIZE,
        };
        self.fill(clear, digits::BLACK)?;
        rects.iter().try_for_each(|&r| self.fill(r, digits::INK))
    }
}

async fn running_version() -> Option<Version> {
    let start = Instant::now();
    loop {
        if let Some(v) = node::running_version() {
            return Some(v);
        }
        if start.elapsed() >= VERSION_WAIT {
            return None;
        }
        Timer::after(VERSION_POLL).await;
    }
}

/// Shows the running version. `powered`: the panel supply came up. Gives up
/// quietly (the screen stays dark) if the panel cannot be driven.
#[embassy_executor::task]
pub async fn display_task(p: DisplayPins, lease: Lease, powered: bool) {
    if !powered {
        return;
    }
    let Some(mut panel) = Panel::open(p) else {
        return;
    };
    lease.busy(node::now_ms(), INIT_BUDGET_MS);
    let ready = panel.init().is_ok();
    lease.idle();
    if !ready {
        return;
    }
    let mut text = [0u8; MAX_CHARS];
    let n = digits::text(running_version().await, &mut text);
    let mut rects = [Rect::default(); MAX_RECTS];
    let k = digits::layout(&text[..n], &mut rects);
    lease.busy(node::now_ms(), DRAW_BUDGET_MS);
    let _ = panel.draw(&rects[..k]);
    lease.idle();
    // Nothing changes while this image runs: keep the panel (and its pins).
    core::future::pending::<()>().await;
}
