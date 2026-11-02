//! Host-side procedures shared by the CLI and the browser: discover, status,
//! topology push, apply, rollback, confirm, recover.
//!
//! Everything runs through two small traits the caller supplies: a
//! [`Transport`] carrying the byte stream to the hub (serial port, Web
//! Serial, simulator) and a [`Clock`]. No std I/O and no async runtime, so
//! the crate builds for WASM unchanged.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod ops;
pub mod rom;

pub use dagon_romlink::{RomError, Stats as RomStats};
pub use ops::{
    reason, ConfirmOutcome, Discovery, Event, NodeOutcome, NodeState, Outcome, RecoverAssets,
    RecoverPath,
};

use alloc::vec;
use alloc::vec::Vec;
use dagon_conform::timeout::REQUEST_MS;
use dagon_format::topology::{self, TopologyView};
use dagon_format::{FormatError, Version};
use dagon_proto::payload::{link, HelloReq, HelloResp, Relay};
use dagon_proto::{find_sof, flags, max_payload, split_status, Cmd, Frame, FrameError, Status};

/// Byte stream to the hub (USB CDC).
#[allow(async_fn_in_trait)]
pub trait Transport {
    type Error: core::fmt::Debug;
    /// Sends all of `bytes`.
    async fn write(&mut self, bytes: &[u8]) -> core::result::Result<(), Self::Error>;
    /// Bytes received so far, up to `buf.len()`; may wait briefly. `Ok(0)`
    /// means nothing has arrived yet.
    async fn read(&mut self, buf: &mut [u8]) -> core::result::Result<usize, Self::Error>;
    /// Closes and opens the link again: the hub's USB device disappears on
    /// REBOOT. Called repeatedly until the hub answers.
    async fn reopen(&mut self) -> core::result::Result<(), Self::Error>;
}

/// Monotonic time source (the same trait the ROM procedure uses).
pub use dagon_romlink::Clock;

/// Where a request goes: the hub itself, or a node behind it via RELAY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Hub,
    /// 7-bit address on the hub's I2C0.
    I2c(u8),
    /// The node on the hub's PIO UART.
    Uart,
}

impl Target {
    /// Target of topology node `node`. `None`: no such node, or its link is
    /// not one the hub relays on.
    pub fn of_node(topo: &TopologyView<'_>, node: u8) -> Option<Target> {
        let n = (node as usize) < topo.node_count();
        if n && topo.node(node as usize).role == topology::role::COORDINATOR {
            return Some(Target::Hub);
        }
        let l = (0..topo.link_count())
            .map(|i| topo.link(i))
            .find(|l| l.to == node)?;
        match l.iface {
            topology::iface::I2C_BUS => Some(Target::I2c(l.i2c_addr)),
            topology::iface::PIO_UART => Some(Target::Uart),
            _ => None,
        }
    }

    fn relay(self) -> Option<(u8, u8)> {
        match self {
            Target::Hub => None,
            Target::I2c(a) => Some((link::I2C0, a)),
            Target::Uart => Some((link::PIO_UART, link::UART_ADDR)),
        }
    }

    fn link_max(self) -> usize {
        match self {
            Target::Hub => max_payload::USB,
            Target::I2c(_) => max_payload::I2C,
            Target::Uart => max_payload::UART,
        }
    }
}

#[derive(Debug)]
pub enum Error<E> {
    Transport(E),
    /// No matching response in time.
    Timeout,
    /// The hub answered RELAY with this status (`Timeout`: the node did not
    /// answer).
    Relay(Status),
    /// The target answered `cmd` with a non-OK status.
    Rejected {
        cmd: Cmd,
        status: Status,
    },
    /// A response that does not parse.
    BadResponse,
    /// The release or topology binary failed its checks.
    Format(FormatError),
    /// The release was built for another topology; nothing was sent.
    TopologyMismatch {
        release: [u8; 32],
        local: [u8; 32],
    },
    /// XFER_END digest differs from what was sent.
    DigestMismatch,
    /// The release has no image for the node's free slot.
    NoImage {
        node: u8,
    },
    /// The topology gives no relay route to the node.
    Unreachable {
        node: u8,
    },
    /// No valid image in the other slot to go back to.
    NoPreviousVersion,
    /// The target came back running another version.
    WrongVersion {
        expected: Version,
        running: Version,
        trial: bool,
    },
    /// The ROM loader session failed (after its retries).
    Rom(dagon_romlink::RomError),
    Unsupported,
}

impl<E> From<FormatError> for Error<E> {
    fn from(e: FormatError) -> Self {
        Error::Format(e)
    }
}

pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// PC requests use seq 0x00..=0x7F; the hub's own polling uses the rest.
const SEQ_MASK: u8 = 0x7F;
const READ_CHUNK: usize = 1024;

/// A session with one hub. Remembers the versions it applied, for the
/// confirm rule after a manual reconnect.
pub struct Host<T: Transport, C: Clock> {
    pub transport: T,
    pub clock: C,
    seq: u8,
    rx: Vec<u8>,
    applied: Vec<(Target, Version)>,
}

impl<T: Transport, C: Clock> Host<T, C> {
    pub fn new(transport: T, clock: C) -> Self {
        Host {
            transport,
            clock,
            seq: 0,
            rx: Vec::new(),
            applied: Vec::new(),
        }
    }

    /// Version this session last applied to `to`.
    pub fn applied(&self, to: Target) -> Option<Version> {
        self.applied.iter().find(|a| a.0 == to).map(|a| a.1)
    }

    fn set_applied(&mut self, to: Target, v: Version) {
        self.applied.retain(|a| a.0 != to);
        self.applied.push((to, v));
    }

    fn frame(&mut self, cmd: Cmd, payload: &[u8]) -> (u8, Vec<u8>) {
        self.seq = (self.seq + 1) & SEQ_MASK;
        let mut out = vec![0u8; dagon_proto::frame_len(payload.len())];
        let f = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        };
        f.encode(&mut out).expect("sized for the payload");
        (self.seq, out)
    }

    /// Next response frame for (`seq`, `cmd`) in the receive buffer; other
    /// frames and noise are dropped.
    fn take_response(&mut self, seq: u8, cmd: Cmd) -> Option<Vec<u8>> {
        loop {
            let Some(start) = find_sof(&self.rx, 0) else {
                self.rx.clear();
                return None;
            };
            self.rx.drain(..start);
            match Frame::decode(&self.rx, max_payload::USB) {
                Ok((f, n)) => {
                    let hit = f.flags & flags::RESP != 0 && f.seq == seq && f.cmd == cmd as u8;
                    let payload = hit.then(|| f.payload.to_vec());
                    self.rx.drain(..n);
                    if payload.is_some() {
                        return payload;
                    }
                }
                Err(FrameError::Incomplete) => return None,
                Err(_) => {
                    self.rx.drain(..1);
                }
            }
        }
    }

    /// One request to the hub; returns the response payload (status first).
    async fn exchange(
        &mut self,
        cmd: Cmd,
        payload: &[u8],
        wait_ms: u32,
    ) -> Result<Vec<u8>, T::Error> {
        let (seq, frame) = self.frame(cmd, payload);
        self.transport
            .write(&frame)
            .await
            .map_err(Error::Transport)?;
        let deadline = self.clock.now_ms() + wait_ms as u64;
        let mut buf = [0u8; READ_CHUNK];
        loop {
            if let Some(p) = self.take_response(seq, cmd) {
                return Ok(p);
            }
            if self.clock.now_ms() >= deadline {
                return Err(Error::Timeout);
            }
            let n = self
                .transport
                .read(&mut buf)
                .await
                .map_err(Error::Transport)?;
            if n == 0 {
                self.clock.sleep_ms(1).await;
            }
            self.rx.extend_from_slice(&buf[..n]);
        }
    }

    /// One request to `to` (through RELAY for nodes) with `timeout_ms` for
    /// the target to answer; returns the body after an OK status.
    pub async fn call(
        &mut self,
        to: Target,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Vec<u8>, T::Error> {
        let Some((link, addr)) = to.relay() else {
            let p = self.exchange(cmd, payload, timeout_ms).await?;
            return ok_body(cmd, &p);
        };
        let (seq, inner) = self.frame(cmd, payload);
        let relay = Relay {
            link,
            addr,
            timeout_ms: timeout_ms.min(u16::MAX as u32) as u16,
            inner: &inner,
        };
        let mut p = vec![0u8; Relay::HEAD_LEN + inner.len()];
        relay.encode(&mut p).expect("sized for the frame");
        let resp = self
            .exchange(Cmd::Relay, &p, timeout_ms + REQUEST_MS)
            .await?;
        let (status, body) = split_status(&resp).ok_or(Error::BadResponse)?;
        if status != Status::Ok {
            return Err(Error::Relay(status));
        }
        let (f, _) = Frame::decode(body, to.link_max()).map_err(|_| Error::BadResponse)?;
        if f.flags & flags::RESP == 0 || f.seq != seq || f.cmd != cmd as u8 {
            return Err(Error::BadResponse);
        }
        ok_body(cmd, f.payload)
    }

    async fn hello_within(&mut self, to: Target, timeout_ms: u32) -> Result<HelloResp, T::Error> {
        let req = HelloReq {
            proto_ver: dagon_proto::PROTO_VERSION as u32,
            max_payload: to.link_max() as u16,
        };
        let mut b = [0u8; 6];
        let n = req.encode(&mut b).expect("6 bytes");
        let body = self.call(to, Cmd::Hello, &b[..n], timeout_ms).await?;
        HelloResp::parse(&body).map_err(|_| Error::BadResponse)
    }

    pub async fn hello(&mut self, to: Target) -> Result<HelloResp, T::Error> {
        self.hello_within(to, REQUEST_MS).await
    }
}

fn ok_body<E>(cmd: Cmd, payload: &[u8]) -> Result<Vec<u8>, E> {
    match split_status(payload) {
        Some((Status::Ok, body)) => Ok(body.to_vec()),
        Some((status, _)) => Err(Error::Rejected { cmd, status }),
        None => Err(Error::BadResponse),
    }
}
