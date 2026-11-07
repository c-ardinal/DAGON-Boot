//! Link drivers over the dagon-port traits (an I2C controller, a PIO UART
//! stream) and the I2C mailbox link: write the request, poll the 3-byte
//! state every 2 ms, read the response, every access bounded by the
//! request's deadline. The UART link is [`crate::uart`].

pub use crate::kinds::*;
pub use dagon_mcu_common::link::{Link, LinkError};

/// Up `usb-cdc`: the USB CDC device (`UsbCdc::new(usb, product, serial)`).
#[cfg(feature = "usb")]
pub use crate::usb::CdcLink as UsbCdc;
/// Down `i2c`: an I2C controller with the DAGON mailbox
/// (`I2c::new(RpI2c(bus))`).
pub type I2c<'d, T> = I2cLink<RpI2c<'d, T>>;
/// Down `uart`: a PIO UART (`Uart::new(PioStream { tx, rx }, now_ms)`).
pub type Uart<'d, P, const TX: usize, const RX: usize> =
    crate::uart::UartLink<PioStream<'d, P, TX, RX>, fn() -> u64>;

use crate::uart::{hello_answer, hello_request};
use dagon_port::{BusError, I2cBus, StreamPort};
use dagon_proto::mailbox::{self, STATE_LEN};
use dagon_proto::payload::HelloResp;
use dagon_proto::{frame_len, max_payload, Frame, Status};
use embassy_rp::i2c::Instance as I2cInstance;
use embassy_rp::i2c::{self, AbortReason, Async};
use embassy_rp::pio::Instance as PioInstance;
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartTx};
use embassy_time::{with_deadline, with_timeout, Duration, Instant, Timer};

const MAILBOX_POLL: Duration = Duration::from_millis(2);
pub const I2C_FRAME: usize = frame_len(max_payload::I2C);
/// Bound on one byte entering the PIO UART TX FIFO.
const BYTE_WRITE_TIMEOUT: Duration = Duration::from_millis(5);
/// Timeout of the hub's own display HELLO on I2C.
const POLL_TIMEOUT_I2C_MS: u16 = 50;
/// Bound on one I2C_RAW transaction (the PC's ROM frame deadline is 500 ms).
const ROM_TIMEOUT: Duration = Duration::from_millis(500);

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

fn deadline(timeout_ms: u16) -> Instant {
    Instant::now() + Duration::from_millis(timeout_ms as u64)
}

/// I2C controller link to child nodes: mailbox exchanges and raw ROM
/// transactions. As a [`Link`], it talks to the node at the address set
/// by [`I2cLink::target`]: `write_frame` writes the request (a NACK, an
/// absent or busy node, is retried until the deadline, which starts
/// there) and `read_frame` polls the 3-byte state every 2 ms until the
/// response is ready, then reads it.
pub struct I2cLink<I> {
    bus: I,
    addr: u8,
    timeout_ms: u16,
    deadline: Instant,
    buf: [u8; I2C_FRAME],
}

impl<I: I2cBus> I2cLink<I> {
    pub fn new(bus: I) -> Self {
        I2cLink {
            bus,
            addr: 0,
            timeout_ms: 0,
            deadline: Instant::from_ticks(0),
            buf: [0; I2C_FRAME],
        }
    }

    /// The node the next request goes to, and its deadline from the start
    /// of the write.
    pub fn target(&mut self, addr: u8, timeout_ms: u16) {
        self.addr = addr;
        self.timeout_ms = timeout_ms;
    }

    /// Sends the request frame `inner` to `addr` and stores the node's
    /// response frame in `out`; returns its length.
    pub async fn exchange(
        &mut self,
        addr: u8,
        inner: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        self.target(addr, timeout_ms);
        self.write_frame(inner).await.map_err(status)?;
        let f = self.read_frame().await.map_err(status)?;
        let n = f.len();
        if n > out.len() {
            return Err(Status::Frame);
        }
        out[..n].copy_from_slice(f);
        Ok(n)
    }

    /// I2C_RAW: one write transaction (if `data` is not empty, closed by
    /// STOP), then one read transaction filling `read` (if not empty). A
    /// NACK or a stuck bus answers E_TIMEOUT.
    pub async fn rom(&mut self, addr: u8, data: &[u8], read: &mut [u8]) -> Result<(), Status> {
        if !data.is_empty() {
            with_timeout(ROM_TIMEOUT, self.bus.write(addr, data))
                .await
                .map_err(|_| Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        if !read.is_empty() {
            with_timeout(ROM_TIMEOUT, self.bus.read(addr, read))
                .await
                .map_err(|_| Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        Ok(())
    }

    pub async fn hello(&mut self, addr: u8, seq: u8) -> Option<HelloResp> {
        let mut req = [0u8; 32];
        let n = hello_request(max_payload::I2C, seq, &mut req)?;
        let mut resp = [0u8; I2C_FRAME];
        let len = self
            .exchange(addr, &req[..n], POLL_TIMEOUT_I2C_MS, &mut resp)
            .await
            .ok()?;
        hello_answer(&resp[..len], max_payload::I2C)
    }
}

/// The Status a relayed request answers for a link failure.
fn status(e: LinkError) -> Status {
    match e {
        LinkError::Timeout | LinkError::Closed => Status::Timeout,
        LinkError::Overflow | LinkError::Corrupt => Status::Frame,
    }
}

impl<I: I2cBus> Link for I2cLink<I> {
    fn max_payload(&self) -> usize {
        max_payload::I2C
    }

    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }

    fn kind(&self) -> Kind {
        Kind::I2c
    }

    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        let timeout = |_| LinkError::Timeout;
        loop {
            Timer::after(MAILBOX_POLL).await;
            let mut st = [0u8; STATE_LEN];
            if with_deadline(self.deadline, self.bus.read(self.addr, &mut st))
                .await
                .map_err(timeout)?
                .is_err()
            {
                continue;
            }
            let Some(mailbox::State::Ready(n)) = mailbox::State::parse(st) else {
                continue;
            };
            let n = n as usize;
            if n > I2C_FRAME {
                return Err(LinkError::Corrupt);
            }
            with_deadline(self.deadline, self.bus.read(self.addr, &mut self.buf[..n]))
                .await
                .map_err(timeout)?
                .map_err(|_| LinkError::Corrupt)?;
            return match Frame::decode(&self.buf[..n], max_payload::I2C) {
                Ok((_, len)) if len == n => Ok(&self.buf[..n]),
                _ => Err(LinkError::Corrupt),
            };
        }
    }

    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.deadline = deadline(self.timeout_ms);
        // A NACK (absent or busy node) is retried until the deadline.
        while with_deadline(self.deadline, self.bus.write(self.addr, frame))
            .await
            .map_err(|_| LinkError::Timeout)?
            .is_err()
        {
            Timer::after(MAILBOX_POLL).await;
        }
        Ok(())
    }
}
