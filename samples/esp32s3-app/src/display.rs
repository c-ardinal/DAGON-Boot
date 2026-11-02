//! Display task: owns the CO5300 AMOLED (the board's `DisplayPins`: QSPI on
//! SPI2 with DMA channel 0) and shows the running version.
//!
//! The panel reset sits on the board's internal I2C expander, which the
//! DAGON side owns; the board's bring-up releases the reset once at start
//! and `node::display_ready` then reports it. This task waits for that, initialises the panel
//! and draws once. It runs in thread mode, below the DAGON link task, and
//! announces a liveness budget for each blocking step.

use dagon_board_m5stopwatch::{DisplayPins, PANEL_COLUMN_OFFSET, PANEL_SIZE};
use dagon_esp32s3_app::digits::{self, Rect, MAX_CHARS, MAX_RECTS};
use dagon_mcu_esp32s3::liveness::Lease;
use dagon_mcu_esp32s3::node;
use embassy_time::{Duration, Timer};
use esp_hal::{
    delay::Delay,
    dma_rx_buffer, dma_tx_buffer,
    spi::{
        master::{Address, Command, Config as SpiConfig, DataMode, Spi, SpiDma},
        Error as SpiError, Mode,
    },
    time::Rate,
    Blocking,
};
use esp_hal_co5300::{registers, Config, Driver, Interface};

// The digit layout is computed for this board's panel.
const _: () = assert!(digits::SIZE == PANEL_SIZE);

/// Bytes per DMA transfer; the driver splits pixel data to this size.
const CHUNK: usize = 4096;
const SPI_MHZ: u32 = 40;
const BRIGHTNESS: u8 = 200;
/// Panel initialisation, including its fixed settling waits.
const INIT_BUDGET_MS: u32 = 3_000;
/// One full redraw (clear plus the digits).
const DRAW_BUDGET_MS: u32 = 1_000;
const READY_POLL: Duration = Duration::from_millis(20);

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

fn open(p: DisplayPins) -> Option<Driver<PanelBus>> {
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
        column_offset: PANEL_COLUMN_OFFSET,
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
pub async fn display_task(p: DisplayPins, lease: Lease) {
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
