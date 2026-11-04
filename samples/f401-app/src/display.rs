//! The shield's display on the board's Arduino I2C (async, in the app's
//! task: its interrupts run below DAGON's executor, so it never holds up
//! the link).

use dagon_board_nucleo_f401re::ArduinoI2c;
use embassy_stm32::i2c::{self, I2c, Master};
use embassy_stm32::interrupt::{self, InterruptExt, Priority};
use embassy_stm32::mode::Async;
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_time::{with_timeout, Duration};
use f401_app::{seg, shield};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
});

/// Longest one expander write may take (a 4-byte write at 100 kHz takes
/// about 0.4 ms; a held bus fails instead of hanging the task).
const WRITE_TIMEOUT: Duration = Duration::from_millis(20);
/// Below DAGON's executor (P6) and every default-priority interrupt.
const IRQ_PRIORITY: Priority = Priority::P7;

/// The 4-digit display, sensors held in XSHUT.
pub struct SegDisplay {
    i2c: I2c<'static, Async, Master>,
    ready: bool,
}

impl SegDisplay {
    /// Opens the Arduino I2C (open drain, 100 kHz). Nothing is written
    /// until [`SegDisplay::show`]; until then the sensors are on (XSHUT
    /// pulled up) and the segments dark.
    pub fn open(bus: ArduinoI2c) -> Self {
        let mut cfg = i2c::Config::default();
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        let i2c = I2c::new(bus.i2c, bus.scl, bus.sda, bus.tx_dma, bus.rx_dma, Irqs, cfg);
        interrupt::I2C1_EV.set_priority(IRQ_PRIORITY);
        interrupt::I2C1_ER.set_priority(IRQ_PRIORITY);
        interrupt::DMA1_STREAM6.set_priority(IRQ_PRIORITY);
        interrupt::DMA1_STREAM0.set_priority(IRQ_PRIORITY);
        SegDisplay { i2c, ready: false }
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
            with_timeout(WRITE_TIMEOUT, self.i2c.write(*addr, bytes))
                .await
                .map_err(|_| i2c::Error::Timeout)??;
        }
        Ok(())
    }
}
