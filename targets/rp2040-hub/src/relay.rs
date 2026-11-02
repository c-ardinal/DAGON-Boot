//! Forwards whole frames to sub nodes over the dagon-port hub traits: I2C
//! mailbox (write the request, poll the 3-byte state every 2 ms, read the
//! response) or a byte stream (write, then cut the response out of it).
//! Every bus access is bounded by the RELAY timeout.

use dagon_port::{BusError, I2cBus, StreamPort};
use dagon_proto::mailbox::{self, STATE_LEN};
use dagon_proto::payload::{link, HelloReq, HelloResp};
use dagon_proto::{flags, frame_len, max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};
use embassy_rp::i2c::{self, AbortReason, Async, I2c};
use embassy_rp::peripherals::{I2C0, PIO0};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartTx};
use embassy_time::{with_deadline, with_timeout, Duration, Instant, Timer};
use rp2040_common::frames::FrameReader;

const MAILBOX_POLL: Duration = Duration::from_millis(2);
pub const I2C_FRAME: usize = frame_len(max_payload::I2C);
const UART_FRAME: usize = frame_len(max_payload::UART);
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

pub struct Links<I, S> {
    i2c: I,
    uart: S,
    reader: FrameReader<{ UART_FRAME * 2 }>,
}

/// Payload limit of a RELAY link, `None` for an unknown link.
pub fn link_max(l: u8) -> Option<usize> {
    match l {
        link::I2C0 => Some(max_payload::I2C),
        link::PIO_UART => Some(max_payload::UART),
        _ => None,
    }
}

impl<I: I2cBus, S: StreamPort> Links<I, S> {
    pub fn new(i2c: I, uart: S) -> Self {
        Links {
            i2c,
            uart,
            reader: FrameReader::new(max_payload::UART),
        }
    }

    /// Sends the request frame `inner` and stores the node's response frame
    /// in `out`; returns its length. `inner` is a valid frame for the link.
    pub async fn exchange(
        &mut self,
        l: u8,
        addr: u8,
        inner: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        match l {
            link::I2C0 => self.i2c_exchange(addr, inner, deadline, out).await,
            link::PIO_UART => self.uart_exchange(inner, deadline, out).await,
            _ => Err(Status::Arg),
        }
    }

    async fn i2c_exchange(
        &mut self,
        addr: u8,
        inner: &[u8],
        deadline: Instant,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        let timeout = |_| Status::Timeout;
        // A NACK (absent or busy node) is retried until the deadline.
        while with_deadline(deadline, self.i2c.write(addr, inner))
            .await
            .map_err(timeout)?
            .is_err()
        {
            Timer::after(MAILBOX_POLL).await;
        }
        loop {
            Timer::after(MAILBOX_POLL).await;
            let mut st = [0u8; STATE_LEN];
            if with_deadline(deadline, self.i2c.read(addr, &mut st))
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
            with_deadline(deadline, self.i2c.read(addr, &mut out[..n]))
                .await
                .map_err(timeout)?
                .map_err(|_| Status::Frame)?;
            return match Frame::decode(&out[..n], max_payload::I2C) {
                Ok((_, len)) if len == n => Ok(n),
                _ => Err(Status::Frame),
            };
        }
    }

    async fn uart_exchange(
        &mut self,
        inner: &[u8],
        deadline: Instant,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        self.reader.clear();
        with_deadline(deadline, self.uart.write(inner))
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
            if left == 0 || self.uart.read(&mut b, left).await.is_err() {
                return Err(Status::Timeout);
            }
            self.reader.push(&b);
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

    /// I2C_RAW: one write transaction (if `data` is not empty, closed by
    /// STOP), then one read transaction filling `read` (if not empty). A
    /// NACK or a stuck bus answers E_TIMEOUT.
    pub async fn rom(&mut self, addr: u8, data: &[u8], read: &mut [u8]) -> Result<(), Status> {
        if !data.is_empty() {
            with_timeout(ROM_TIMEOUT, self.i2c.write(addr, data))
                .await
                .map_err(|_| Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        if !read.is_empty() {
            with_timeout(ROM_TIMEOUT, self.i2c.read(addr, read))
                .await
                .map_err(|_| Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        Ok(())
    }

    /// The hub's own read-only HELLO (seq from the 0x80..=0xFF space).
    pub async fn hello(&mut self, l: u8, addr: u8, seq: u8) -> Option<HelloResp> {
        let max = link_max(l)?;
        let mut req = [0u8; 32];
        let body = HelloReq {
            proto_ver: PROTO_VERSION as u32,
            max_payload: max as u16,
        }
        .encode(&mut req[8..])
        .ok()?;
        let n = dagon_proto::finish(&mut req, 0, seq, Cmd::Hello as u8, body).ok()?;
        let timeout = if l == link::I2C0 {
            POLL_TIMEOUT_I2C_MS
        } else {
            POLL_TIMEOUT_UART_MS
        };
        let mut resp = [0u8; I2C_FRAME];
        let len = self
            .exchange(l, addr, &req[..n], timeout, &mut resp)
            .await
            .ok()?;
        let (f, _) = Frame::decode(&resp[..len], max).ok()?;
        match split_status(f.payload)? {
            (Status::Ok, b) => HelloResp::parse(b).ok(),
            _ => None,
        }
    }
}
