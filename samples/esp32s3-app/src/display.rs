//! Display task: owns the CO5300 AMOLED (QSPI on SPI2) and shows the running
//! version, or while DAGON updates the watch the update phase
//! ([`dagon_esp32s3_app::screen`], from the node's published events).
//!
//! The panel's power and reset sit on the internal I2C expander; `main` runs
//! [`crate::power::panel_power_on`] before DAGON takes that bus and passes
//! the result here. The command framing, init table, clock and window are
//! those of M5Stack's M5StopWatch-UserDemo V0.5 (M5GFX `Panel_AMOLED`). This
//! task runs in thread mode, below the DAGON link task, and announces a
//! liveness budget for each blocking step.

use dagon_agent::snapshot::{AppSnapshot, Phase};
use dagon_esp32s3_app::digits::{self, Rect, MAX_RECTS};
use dagon_esp32s3_app::screen::{self, Screen};
use dagon_mcu_esp32s3::liveness::Lease;
use dagon_mcu_esp32s3::{node, Version};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_hal::{
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
/// A pause that hands the thread executor to the other demo tasks between
/// blocking panel steps (the SPI writes do not yield by themselves).
const YIELD: Duration = Duration::from_millis(1);
/// How often the screen is recomputed without a new event: the trial
/// blink and the results' hold time step at this rate.
/// Demo readability value, pending owner confirmation.
const TICK: Duration = Duration::from_millis(250);

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

    async fn init(&mut self) -> Result<(), SpiError> {
        self.command(SLPOUT, &[])?;
        Timer::after(Duration::from_millis(SLPOUT_WAIT_MS.into())).await;
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

    async fn draw(&mut self, rects: &[Rect]) -> Result<(), SpiError> {
        // The clear starts at column 0 of the CO5300 memory: 468 wide.
        let clear = Rect {
            x: 0,
            y: 0,
            w: CLEAR_WIDTH,
            h: digits::SIZE,
        };
        self.fill(clear, digits::BLACK)?;
        for &r in rects {
            Timer::after(YIELD).await;
            self.fill(r, digits::INK)?;
        }
        Ok(())
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

/// Shows the running version, and each update phase while it lasts
/// (`events`: the node's published events). `powered`: the panel supply
/// came up. Gives up quietly (the screen stays dark) if the panel cannot
/// be driven.
#[embassy_executor::task]
pub async fn display_task(p: DisplayPins, lease: Lease, powered: bool, events: node::AppEvents) {
    if !powered {
        return;
    }
    // The heartbeat runs first: CONFIRM waits for its first beat, and the
    // executor may poll this task before it.
    Timer::after(YIELD).await;
    let Some(mut panel) = Panel::open(p) else {
        return;
    };
    lease.busy(node::now_ms(), INIT_BUDGET_MS);
    let ready = panel.init().await.is_ok();
    lease.idle();
    if !ready {
        return;
    }
    let version = running_version().await;
    let normal = screen::screen(Phase::Idle, None, 0, version);
    show(&mut panel, &lease, &normal).await;
    // Without a receiver the version stays: keep the panel (and its pins).
    let Some(mut rx) = events.receiver() else {
        return core::future::pending().await;
    };
    let (mut shown, mut since) = (normal, Instant::now());
    let mut state: Option<AppSnapshot> = None;
    loop {
        if let Ok(latest) = with_timeout(TICK, rx.changed()).await {
            let s = latest.state;
            if state.map(|old| old.phase) != Some(s.phase) {
                since = Instant::now();
            }
            state = Some(s);
        }
        let next = state.map_or(normal, |s| {
            let ms = since.elapsed().as_millis() as u32;
            screen::screen(s.phase, s.progress, ms, version)
        });
        if next != shown {
            show(&mut panel, &lease, &next).await;
            shown = next;
        }
    }
}

/// Draws `s` (its line and progress bar) within the draw budget.
async fn show(panel: &mut Panel, lease: &Lease, s: &Screen) {
    let mut rects = [Rect::default(); MAX_RECTS];
    let k = digits::layout(s.chars(), &mut rects);
    lease.busy(node::now_ms(), DRAW_BUDGET_MS);
    if panel.draw(&rects[..k]).await.is_ok() {
        if let Some(bar) = s.percent.and_then(screen::bar) {
            let _ = panel.fill(bar, digits::INK);
        }
    }
    lease.idle();
}
