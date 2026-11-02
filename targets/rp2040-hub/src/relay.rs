//! One link per child bus, over the dagon-port hub traits: the I2C mailbox
//! (write the request, poll the 3-byte state every 2 ms, read the response)
//! and a byte stream (write, then cut the response out of it). Every bus
//! access is bounded by the request's deadline.

use dagon_port::{BusError, I2cBus, StreamPort};
use dagon_proto::mailbox::{self, STATE_LEN};
use dagon_proto::payload::{HelloReq, HelloResp};
use dagon_proto::{flags, frame_len, max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};
use embassy_rp::i2c::{self, AbortReason, Async, I2c};
use embassy_rp::peripherals::{I2C0, PIO0};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartTx};
use embassy_time::{with_deadline, with_timeout, Duration, Instant, Timer};
use rp2040_common::frames::FrameReader;

const MAILBOX_POLL: Duration = Duration::from_millis(2);
pub const I2C_FRAME: usize = frame_len(max_payload::I2C);
pub const UART_FRAME: usize = frame_len(max_payload::UART);
/// Timeouts for the hub's own display HELLO.
const POLL_TIMEOUT_I2C_MS: u16 = 50;
const POLL_TIMEOUT_UART_MS: u16 = 200;
/// Bound on one I2C_RAW transaction (the PC's ROM frame deadline is 500 ms).
const ROM_TIMEOUT: Duration = Duration::from_millis(500);

/// I2C0 master (GPIO16/17).
pub struct RpI2c<'d>(pub I2c<'d, I2C0, Async>);

fn bus(e: i2c::Error) -> BusError {
    match e {
        i2c::Error::Abort(AbortReason::NoAcknowledge) => BusError::Nack,
        _ => BusError::Io,
    }
}

impl I2cBus for RpI2c<'_> {
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

/// PIO UART (TX GPIO26, RX GPIO27).
pub struct PioStream<'d> {
    pub tx: PioUartTx<'d, PIO0, 0>,
    pub rx: PioUartRx<'d, PIO0, 1>,
}

impl StreamPort for PioStream<'_> {
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
    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        for &b in data {
            self.tx.write_u8(b).await;
        }
        Ok(())
    }
}

fn deadline(timeout_ms: u16) -> Instant {
    Instant::now() + Duration::from_millis(timeout_ms as u64)
}

/// The hub's own read-only HELLO request (seq from the 0x80..=0xFF space).
fn hello_request(max: usize, seq: u8, out: &mut [u8; 32]) -> Option<usize> {
    let body = HelloReq {
        proto_ver: PROTO_VERSION as u32,
        max_payload: max as u16,
    }
    .encode(&mut out[8..])
    .ok()?;
    dagon_proto::finish(out, 0, seq, Cmd::Hello as u8, body).ok()
}

fn hello_answer(resp: &[u8], max: usize) -> Option<HelloResp> {
    let (f, _) = Frame::decode(resp, max).ok()?;
    match split_status(f.payload)? {
        (Status::Ok, b) => HelloResp::parse(b).ok(),
        _ => None,
    }
}

/// I2C0 master: mailbox exchanges and raw ROM transactions.
pub struct I2cLink<I> {
    bus: I,
}

impl<I: I2cBus> I2cLink<I> {
    pub fn new(bus: I) -> Self {
        I2cLink { bus }
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
        let deadline = deadline(timeout_ms);
        let timeout = |_| Status::Timeout;
        // A NACK (absent or busy node) is retried until the deadline.
        while with_deadline(deadline, self.bus.write(addr, inner))
            .await
            .map_err(timeout)?
            .is_err()
        {
            Timer::after(MAILBOX_POLL).await;
        }
        loop {
            Timer::after(MAILBOX_POLL).await;
            let mut st = [0u8; STATE_LEN];
            if with_deadline(deadline, self.bus.read(addr, &mut st))
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
            if n > out.len().min(I2C_FRAME) {
                return Err(Status::Frame);
            }
            with_deadline(deadline, self.bus.read(addr, &mut out[..n]))
                .await
                .map_err(timeout)?
                .map_err(|_| Status::Frame)?;
            return match Frame::decode(&out[..n], max_payload::I2C) {
                Ok((_, len)) if len == n => Ok(n),
                _ => Err(Status::Frame),
            };
        }
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

/// PIO UART: frames cut out of the byte stream.
pub struct UartLink<S> {
    port: S,
    reader: FrameReader<{ UART_FRAME * 2 }>,
}

impl<S: StreamPort> UartLink<S> {
    pub fn new(port: S) -> Self {
        UartLink {
            port,
            reader: FrameReader::new(max_payload::UART),
        }
    }

    /// Writes the request frame `inner` and stores the response frame with
    /// the same seq in `out`; returns its length.
    pub async fn exchange(
        &mut self,
        inner: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        let deadline = deadline(timeout_ms);
        self.reader.clear();
        with_deadline(deadline, self.port.write(inner))
            .await
            .map_err(|_| Status::Timeout)?
            .map_err(|_| Status::Timeout)?;
        // A request too short to carry a seq gets no answer (times out).
        let seq = inner.get(4).copied();
        loop {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u32;
            let mut b = [0u8; 1];
            if left == 0 || self.port.read(&mut b, left).await.is_err() {
                return Err(Status::Timeout);
            }
            // A partial frame left by a 20 ms pause is dropped first.
            self.reader.push_at(&b, Instant::now().as_millis());
            while let Some(n) = self.reader.poll() {
                let f = self.reader.frame(n);
                if f[3] & flags::RESP != 0 && Some(f[4]) == seq && n <= out.len() {
                    out[..n].copy_from_slice(f);
                    self.reader.consume(n);
                    return Ok(n);
                }
                self.reader.consume(n);
            }
        }
    }

    pub async fn hello(&mut self, seq: u8) -> Option<HelloResp> {
        let mut req = [0u8; 32];
        let n = hello_request(max_payload::UART, seq, &mut req)?;
        let mut resp = [0u8; I2C_FRAME];
        let len = self
            .exchange(&req[..n], POLL_TIMEOUT_UART_MS, &mut resp)
            .await
            .ok()?;
        hello_answer(&resp[..len], max_payload::UART)
    }
}
