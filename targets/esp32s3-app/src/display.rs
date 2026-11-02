//! Display task: owns the CO5300 AMOLED (QSPI on SPI2 with DMA channel 0,
//! CS G39, SCK G40, D0..D3 G41/G42/G46/G45) and shows the running version.
//!
//! The panel reset sits on the internal I2C expander, which the DAGON side
//! owns; it releases the reset once at start and publishes
//! `node::display_ready`. This task waits for that, initialises the panel
//! and draws once. It runs in thread mode, below the DAGON link task, and
//! announces a liveness budget for each blocking step.

use dagon_esp32s3_agent::liveness::Lease;
use dagon_esp32s3_agent::node;
use dagon_esp32s3_app::digits::{self, Rect, MAX_CHARS, MAX_RECTS};
use embassy_time::{Duration, Timer};
use esp_hal::{
    delay::Delay,
    dma_rx_buffer, dma_tx_buffer,
    peripherals::{DMA_CH0, GPIO39, GPIO40, GPIO41, GPIO42, GPIO45, GPIO46, SPI2},
    spi::{
        master::{Address, Command, Config as SpiConfig, DataMode, Spi, SpiDma},
        Error as SpiError, Mode,
    },
    time::Rate,
    Blocking,
};
use esp_hal_co5300::{registers, Config, Driver, Interface};

/// Bytes per DMA transfer; the driver splits pixel data to this size.
const CHUNK: usize = 4096;
const SPI_MHZ: u32 = 40;
/// The 1.75" 466×466 panel starts 6 columns into the controller's memory.
const COLUMN_OFFSET: u16 = 6;
const BRIGHTNESS: u8 = 200;
/// Panel initialisation, including its fixed settling waits.
const INIT_BUDGET_MS: u32 = 3_000;
/// One full redraw (clear plus the digits).
const DRAW_BUDGET_MS: u32 = 1_000;
const READY_POLL: Duration = Duration::from_millis(20);

/// What the display task owns.
pub struct DisplayPeripherals {
    pub spi2: SPI2<'static>,
    pub dma: DMA_CH0<'static>,
    pub cs: GPIO39<'static>,
    pub sck: GPIO40<'static>,
    pub d0: GPIO41<'static>,
    pub d1: GPIO42<'static>,
    pub d2: GPIO46<'static>,
    pub d3: GPIO45<'static>,
}

/// QSPI transport for the CO5300 driver. The reset is not here: the DAGON
/// side has already pulsed it.
struct PanelBus {
    spi: SpiDma<'static, Blocking>,
}

impl Interface for PanelBus {
    type Error = SpiError;

    fn max_write_len(&self) -> usize {
        CHUNK
    }

    fn write(&mut self, register: u8, data: &[u8], quad: bool) -> Result<(), SpiError> {
        let (mode, opcode) = if quad {
            (DataMode::Quad, registers::WRITE_COLOR)
        } else {
            (DataMode::Single, registers::WRITE_COMMAND)
        };
        self.spi.half_duplex_write(
            mode,
            Command::_8Bit(u16::from(opcode), DataMode::Single),
            Address::_24Bit(u32::from(register) << 8, DataMode::Single),
            0,
            data,
        )
    }

    fn read(&mut self, register: u8, data: &mut [u8]) -> Result<(), SpiError> {
        self.spi.half_duplex_read(
            DataMode::Single,
            Command::_8Bit(u16::from(registers::READ_COMMAND), DataMode::Single),
            Address::_24Bit(u32::from(register) << 8, DataMode::Single),
            0,
            data,
        )
    }

    fn reset(&mut self, _high: bool) {}

    fn delay_ms(&mut self, ms: u32) {
        Delay::new().delay_millis(ms);
    }
}

fn open(p: DisplayPeripherals) -> Option<Driver<PanelBus>> {
    let rx = dma_rx_buffer!(64).ok()?;
    let tx = dma_tx_buffer!(CHUNK).ok()?;
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
        .with_sio3(p.d3)
        .with_dma(p.dma)
        .with_buffers(rx, tx);
    let config = Config {
        width: digits::SIZE,
        height: digits::SIZE,
        column_offset: COLUMN_OFFSET,
        row_offset: 0,
        brightness: BRIGHTNESS,
        ..Config::default()
    };
    Driver::with_interface(PanelBus { spi }, config).ok()
}

fn draw(
    panel: &mut Driver<PanelBus>,
    rects: &[Rect],
) -> Result<(), esp_hal_co5300::DriverError<SpiError>> {
    panel.clear(digits::BLACK)?;
    for r in rects {
        panel.fill_rect(r.x, r.y, r.w, r.h, digits::INK)?;
    }
    panel.wait();
    Ok(())
}

/// Shows the running version once the DAGON side has released the panel.
/// Gives up quietly (the screen stays dark) if the panel does not answer.
#[embassy_executor::task]
pub async fn display_task(p: DisplayPeripherals, lease: Lease) {
    while !node::display_ready() {
        Timer::after(READY_POLL).await;
    }
    let Some(mut panel) = open(p) else {
        return;
    };
    lease.busy(node::now_ms(), INIT_BUDGET_MS);
    let ready = panel.init().is_ok();
    lease.idle();
    if !ready {
        return;
    }
    let mut text = [0u8; MAX_CHARS];
    let n = digits::text(node::running_version(), &mut text);
    let mut rects = [Rect::default(); MAX_RECTS];
    let k = digits::layout(&text[..n], &mut rects);
    lease.busy(node::now_ms(), DRAW_BUDGET_MS);
    let _ = draw(&mut panel, &rects[..k]);
    lease.idle();
    // Nothing changes while this image runs: keep the panel (and its pins).
    core::future::pending::<()>().await;
}
