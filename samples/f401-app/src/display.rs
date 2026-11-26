//! The shield's display on the board's Arduino I2C (async, in the app's
//! task: its interrupts run below DAGON's executor, so it never holds up
//! the link).

use dagon_board_nucleo_f401re::ArduinoI2c;
use dagon_format::header::{ImageHeader, HEADER_SIZE};
use dagon_format::Version;
use dagon_mcu_stm32f401re::{layout, node};
use embassy_stm32::i2c::{self, I2c, Master};
use embassy_stm32::interrupt::{self, InterruptExt, Priority};
use embassy_stm32::mode::Async;
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_time::{with_timeout, Duration, Instant};
use f401_app::{seg, shield};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
});

const WRITE_TIMEOUT: Duration = Duration::from_millis(shield::WRITE_TIMEOUT_MS);
pub use shield::SHOW_BOUND_MS;
/// Below DAGON's executor (P6) and every default-priority interrupt.
const IRQ_PRIORITY: Priority = Priority::P7;

/// The 4-digit display, sensors held in XSHUT.
pub struct SegDisplay {
    i2c: I2c<'static, Async, Master>,
    ready: bool,
    /// The longest write so far, in microseconds (logged when it grows:
    /// the measured side of [`shield::WRITE_TIMEOUT_MS`]).
    longest_us: u64,
}

impl SegDisplay {
    /// Opens the Arduino I2C (open drain, [`shield::BUS_HZ`]). Nothing is written
    /// until [`SegDisplay::show`]; until then the sensors are on (XSHUT
    /// pulled up) and the segments dark.
    pub fn open(bus: ArduinoI2c) -> Self {
        let mut cfg = i2c::Config::default();
        cfg.frequency = Hertz(shield::BUS_HZ);
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        let i2c = I2c::new(bus.i2c, bus.scl, bus.sda, bus.tx_dma, bus.rx_dma, Irqs, cfg);
        interrupt::I2C1_EV.set_priority(IRQ_PRIORITY);
        interrupt::I2C1_ER.set_priority(IRQ_PRIORITY);
        interrupt::DMA1_STREAM6.set_priority(IRQ_PRIORITY);
        interrupt::DMA1_STREAM0.set_priority(IRQ_PRIORITY);
        SegDisplay {
            i2c,
            ready: false,
            longest_us: 0,
        }
    }

    /// Shows `codes` ([`seg`] text). The first call sets the expanders up
    /// (all off, XSHUT low, then outputs); a failed set-up is retried on
    /// the next call.
    pub async fn show(&mut self, codes: [u8; seg::DIGITS]) -> Result<(), i2c::Error> {
        if !self.ready {
            self.write_all(&shield::INIT).await?;
            self.ready = true;
        }
        self.write_all(&shield::show(&codes)).await
    }

    async fn write_all(&mut self, writes: &[shield::Write]) -> Result<(), i2c::Error> {
        for (addr, bytes) in writes {
            let started = Instant::now();
            with_timeout(WRITE_TIMEOUT, self.i2c.write(*addr, bytes))
                .await
                .map_err(|_| i2c::Error::Timeout)??;
            let us = started.elapsed().as_micros();
            if us > self.longest_us {
                self.longest_us = us;
                defmt::info!(
                    "display: longest write {} us (free bus {} us, limit {} ms)",
                    us,
                    shield::WRITE_FREE_US,
                    shield::WRITE_TIMEOUT_MS
                );
            }
        }
        Ok(())
    }
}

/// The running image's version, from its header at the start of its slot
/// (default if unreadable).
pub fn running_version() -> Version {
    let addr = layout::SLOTS.slots[node::ACTIVE as usize].header as usize;
    // SAFETY: the slot start is in the memory-mapped internal flash
    // (always readable, aligned for u8), and DAGON never erases or writes
    // the running image's slot.
    let raw = unsafe { &*(addr as *const [u8; HEADER_SIZE]) };
    ImageHeader::parse(raw)
        .map(|h| h.version)
        .unwrap_or_default()
}
