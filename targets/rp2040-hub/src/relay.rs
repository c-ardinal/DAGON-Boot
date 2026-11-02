//! Forwards whole frames to sub nodes: I2C0 mailbox (write the request, poll
//! the 3-byte state every 2 ms, read the response) or the PIO UART (write,
//! then cut the response out of the byte stream).

use dagon_proto::mailbox::{self, STATE_LEN};
use dagon_proto::payload::{link, HelloReq, HelloResp};
use dagon_proto::{flags, frame_len, max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::{I2C0, PIO0};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartTx};
use embassy_time::{with_deadline, Duration, Instant, Timer};
use rp2040_common::frames::FrameReader;

const MAILBOX_POLL: Duration = Duration::from_millis(2);
pub const I2C_FRAME: usize = frame_len(max_payload::I2C);
const UART_FRAME: usize = frame_len(max_payload::UART);
/// Timeouts for the hub's own display HELLO.
const POLL_TIMEOUT_I2C_MS: u16 = 50;
const POLL_TIMEOUT_UART_MS: u16 = 200;

pub struct Links<'d> {
    pub i2c: I2c<'d, I2C0, Async>,
    pub tx: PioUartTx<'d, PIO0, 0>,
    pub rx: PioUartRx<'d, PIO0, 1>,
    uart: FrameReader<{ UART_FRAME * 2 }>,
}

/// Payload limit of a RELAY link, `None` for an unknown link.
pub fn link_max(l: u8) -> Option<usize> {
    match l {
        link::I2C0 => Some(max_payload::I2C),
        link::PIO_UART => Some(max_payload::UART),
        _ => None,
    }
}

impl<'d> Links<'d> {
    pub fn new(
        i2c: I2c<'d, I2C0, Async>,
        tx: PioUartTx<'d, PIO0, 0>,
        rx: PioUartRx<'d, PIO0, 1>,
    ) -> Self {
        Links {
            i2c,
            tx,
            rx,
            uart: FrameReader::new(max_payload::UART),
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
        // A NACK (absent or busy node) is retried until the deadline.
        loop {
            let w = self.i2c.write_async(addr, inner.iter().copied());
            match with_deadline(deadline, w).await {
                Ok(Ok(())) => break,
                Ok(Err(_)) => Timer::after(MAILBOX_POLL).await,
                Err(_) => return Err(Status::Timeout),
            }
            if Instant::now() >= deadline {
                return Err(Status::Timeout);
            }
        }
        loop {
            Timer::after(MAILBOX_POLL).await;
            if Instant::now() >= deadline {
                return Err(Status::Timeout);
            }
            let mut st = [0u8; STATE_LEN];
            if self.i2c.read_async(addr, &mut st).await.is_err() {
                continue;
            }
            let Some(mailbox::State::Ready(n)) = mailbox::State::parse(st) else {
                continue;
            };
            let n = n as usize;
            if n > out.len().min(I2C_FRAME) {
                return Err(Status::Frame);
            }
            self.i2c
                .read_async(addr, &mut out[..n])
                .await
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
        self.uart.clear();
        for &b in inner {
            self.tx.write_u8(b).await;
        }
        let seq = inner[4];
        loop {
            let b = with_deadline(deadline, self.rx.read_u8())
                .await
                .map_err(|_| Status::Timeout)?;
            self.uart.push(&[b]);
            while let Some(n) = self.uart.poll() {
                let f = self.uart.frame(n);
                if f[3] & flags::RESP != 0 && f[4] == seq && n <= out.len() {
                    out[..n].copy_from_slice(f);
                    self.uart.consume(n);
                    return Ok(n);
                }
                self.uart.consume(n);
            }
        }
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
