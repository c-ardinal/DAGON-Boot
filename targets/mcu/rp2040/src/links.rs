//! Link drivers over the dagon-port traits (an I2C controller, a PIO UART
//! stream) and the time of the I2C mailbox link (`dagon_mcu_common::i2c_link`:
//! write the request, poll the 3-byte state every 2 ms, read the response,
//! every access bounded by the request's deadline). The UART link is
//! [`crate::uart`].

pub use crate::kinds::*;
pub use dagon_mcu_common::link::{Link, LinkError};

/// Up `usb-cdc`: the USB CDC device (`UsbCdc::new(usb, product, serial)`).
#[cfg(feature = "usb")]
pub use crate::usb::CdcLink as UsbCdc;
/// Down `i2c`: an I2C controller with the DAGON mailbox
/// (`I2c::new(RpI2c(bus), RELAY_LINK_I2C0)`).
pub type I2c<'d, T> = I2cLink<RpI2c<'d, T>>;
/// Down `uart`: a PIO UART (`Uart::new(PioStream { tx, rx }, now_ms, RELAY_LINK_PIO_UART)`).
pub type Uart<'d, P, const TX: usize, const RX: usize> =
    crate::uart::UartLink<PioStream<'d, P, TX, RX>, fn() -> u64>;

use core::future::Future;

use dagon_mcu_common::i2c_link::Pace;
pub use dagon_mcu_common::i2c_link::I2C_FRAME;
use dagon_port::{BusError, I2cBus, StreamPort};
use embassy_rp::i2c::Instance as I2cInstance;
use embassy_rp::i2c::{self, AbortReason, Async};
use embassy_rp::interrupt::typelevel::Binding;
use embassy_rp::pio::Instance as PioInstance;
use embassy_rp::pio::{self, Pio, PioPin};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::Peri;
use embassy_time::{with_deadline, with_timeout, Duration, Instant, Timer};

/// Bound on one byte entering the PIO UART TX FIFO.
const BYTE_WRITE_TIMEOUT: Duration = Duration::from_millis(5);

/// An I2C controller (the board package names the bus and pins).
pub struct RpI2c<'d, T: I2cInstance>(pub i2c::I2c<'d, T, Async>);

fn bus(e: i2c::Error) -> BusError {
    match e {
        i2c::Error::Abort(AbortReason::NoAcknowledge) => BusError::Nack,
        _ => BusError::Io,
    }
}

impl<T: I2cInstance> I2cBus for RpI2c<'_, T> {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
        self.0
            .write_async(addr, data.iter().copied())
            .await
            .map_err(bus)
    }
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        self.0.read_async(addr, buf).await.map_err(bus)
    }
    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
        self.0
            .write_read_async(addr, data.iter().copied(), buf)
            .await
            .map_err(bus)
    }
}

/// A PIO UART (TX and RX state machines; the board package names the pins).
pub struct PioStream<'d, P: PioInstance, const TX: usize, const RX: usize> {
    pub tx: PioUartTx<'d, P, TX>,
    pub rx: PioUartRx<'d, P, RX>,
}

impl<P: PioInstance, const TX: usize, const RX: usize> StreamPort for PioStream<'_, P, TX, RX> {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        let Some(first) = buf.first_mut() else {
            return Ok(0);
        };
        let wait = Duration::from_millis(timeout_ms as u64);
        *first = with_timeout(wait, self.rx.read_u8())
            .await
            .map_err(|_| BusError::Timeout)?;
        Ok(1)
    }
    /// Each byte must enter the TX FIFO within [`BYTE_WRITE_TIMEOUT`] (one
    /// byte takes about 87 us at 115200 bit/s), so a stuck state machine
    /// cannot hold the link task.
    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        for &b in data {
            with_timeout(BYTE_WRITE_TIMEOUT, self.tx.write_u8(b))
                .await
                .map_err(|_| BusError::Timeout)?;
        }
        Ok(())
    }
}

impl<P: PioInstance> PioStream<'static, P, 0, 1> {
    /// The PIO UART of `pio` on pins `tx`, `rx` at `baud` bit/s: state
    /// machine 0 sends, 1 receives. The PIO runs it for the life of the
    /// app (it is never released).
    pub fn on(
        pio: Peri<'static, P>,
        tx: Peri<'static, impl PioPin>,
        rx: Peri<'static, impl PioPin>,
        irqs: impl Binding<P::Interrupt, pio::InterruptHandler<P>>,
        baud: u32,
    ) -> Self {
        let Pio {
            mut common,
            sm0,
            sm1,
            ..
        } = Pio::new(pio, irqs);
        let tx_prog = PioUartTxProgram::new(&mut common);
        let rx_prog = PioUartRxProgram::new(&mut common);
        let tx = PioUartTx::new(baud, &mut common, sm0, tx, &tx_prog);
        let rx = PioUartRx::new(baud, &mut common, sm1, rx, &rx_prog);
        core::mem::forget(common);
        PioStream { tx, rx }
    }
}

/// The I2C mailbox link's time: embassy-time.
#[derive(Clone, Copy, Default)]
pub struct RpPace;

impl Pace for RpPace {
    fn now_ms(&self) -> u64 {
        Instant::now().as_millis()
    }
    async fn pause(&mut self, ms: u32) {
        Timer::after(Duration::from_millis(u64::from(ms))).await;
    }
    async fn until<T>(&mut self, until_ms: u64, f: impl Future<Output = T>) -> Option<T> {
        with_deadline(Instant::from_millis(until_ms), f).await.ok()
    }
}

/// I2C controller link to other nodes on the bus: mailbox exchanges and
/// raw ROM transactions (`dagon_mcu_common::i2c_link`, which holds the
/// exchange and its host tests), timed by embassy-time.
pub type I2cLink<I> = dagon_mcu_common::i2c_link::I2cLink<I, RpPace>;
