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
use embassy_time::{Duration, Instant, Timer};
use f401_app::screen::{Bus, Fault, Screen, Time};
use f401_app::{seg, shield};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
});

pub use shield::SHOW_BOUND_MS;
/// Below DAGON's executor (P6) and every default-priority interrupt.
const IRQ_PRIORITY: Priority = Priority::P7;

/// The Arduino I2C as the display's bus.
struct ArduinoBus(I2c<'static, Async, Master>);

impl Bus for ArduinoBus {
    type Error = i2c::Error;
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), i2c::Error> {
        self.0.write(addr, bytes).await
    }
}

/// The board's timer as the display's time.
struct BoardTime;

impl Time for BoardTime {
    fn now_us(&self) -> u64 {
        Instant::now().as_micros()
    }
    async fn after_ms(&self, ms: u64) {
        Timer::after(Duration::from_millis(ms)).await
    }
}

/// The 4-digit display, sensors held in XSHUT.
pub struct SegDisplay {
    screen: Screen<ArduinoBus, BoardTime>,
    /// The longest write logged so far, in microseconds.
    logged_us: u64,
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
            screen: Screen::new(ArduinoBus(i2c), BoardTime),
            logged_us: 0,
        }
    }

    /// Shows `codes` ([`seg`] text): [`Screen::show`], and the longest
    /// write goes to RTT when it grew (the measured side of
    /// [`shield::WRITE_TIMEOUT_MS`]).
    pub async fn show(&mut self, codes: [u8; seg::DIGITS]) -> Result<(), Fault<i2c::Error>> {
        let shown = self.screen.show(codes).await;
        let us = self.screen.longest_us();
        if us > self.logged_us {
            self.logged_us = us;
            defmt::info!(
                "display: longest write {} us (free bus {} us, limit {} ms)",
                us,
                shield::WRITE_FREE_US,
                shield::WRITE_TIMEOUT_MS
            );
        }
        shown
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
