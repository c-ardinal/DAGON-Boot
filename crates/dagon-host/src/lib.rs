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
mod percent;
pub mod rom;

pub use dagon_romlink::{RomError, Stats as RomStats};
pub use ops::{
    check_release, reason, ConfirmOutcome, Discovery, Event, NodeOutcome, NodeState, Outcome,
    RecoverAssets, RecoverPath, WritePath, XferTimes, CONFIRM_RETRY_MS, NODE_POLL_MS,
    NODE_TRIAL_MAX_MS, NODE_TRIAL_MS,
};
pub use percent::{transfer_percent, trial_percent};

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use dagon_conform::timeout::REQUEST_MS;
use dagon_format::topology::{self, TopologyView};
use dagon_format::{FormatError, Version};
use dagon_proto::payload::{link, HelloReq, HelloResp, Relay};
use dagon_proto::{find_sof, flags, max_payload, split_status, Cmd, Frame, FrameError, Status};

/// What becomes of the link to the hub while the hub reboots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconnect {
    /// The link goes away with the hub (USB CDC, a TCP connection) and is
    /// opened again ([`Transport::reopen`]) until the hub answers.
    Reopen,
    /// The link stays (a UART adapter): the hub is polled until it answers.
    Stays,
}

/// Byte stream to the hub over its uplink (USB CDC, a UART adapter, a TCP
/// connection, ...). The link's own properties come from here, not from
/// USB constants.
#[allow(async_fn_in_trait)]
pub trait Transport {
    type Error: core::fmt::Debug;
    /// Largest frame payload this link carries to and from the hub.
    fn max_payload(&self) -> usize;
    /// What the link does while the hub reboots.
    fn reconnect_kind(&self) -> Reconnect;
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

    /// Largest payload of the hub's link to this node; None for the hub.
    fn node_link_max(self) -> Option<usize> {
        match self {
            Target::Hub => None,
            Target::I2c(_) => Some(max_payload::I2C),
            Target::Uart => Some(max_payload::UART),
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
    /// The node does not answer and its ROM path was not confirmed; nothing
    /// was touched.
    RomNotConfirmed {
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
    /// The new boot core did not take over: stage0 fell back to the core
    /// running `running` (the new one failed its own check or its image
    /// check; `reason` is the boot core log's state log reason code).
    /// The node takes no boot core: it refused XFER_BEGIN with target
    /// BOOTCORE as unsupported (its trial cell does not hold, so stage0
    /// could not count a new core's starts).
    BootCoreRefused,
    BootCoreRolledBack {
        expected: Version,
        running: Version,
        reason: u8,
    },
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
/// Reads that clearing out what the link still holds may take (each
/// returns at once or after the transport's short read wait).
const DRAIN_READS: usize = 64;

/// Receives [`Host::note`] lines.
pub type Note = Box<dyn FnMut(&str) + Send>;

/// A session with one hub. Remembers the versions it applied, for the
/// confirm rule after a manual reconnect.
pub struct Host<T: Transport, C: Clock> {
    pub transport: T,
    pub clock: C,
    seq: u8,
    rx: Vec<u8>,
    applied: Vec<(Target, Version)>,
    /// Targets that answered this session's opening HELLO.
    opened: Vec<Target>,
    /// What the link holds may predate this session (a new session, or
    /// the port reopened): it is read away before the next request.
    stale: bool,
    /// Pause between a node's HELLO polls after REBOOT
    /// ([`NODE_POLL_MS`]; tests show a longer one misses the trial).
    pub node_poll_ms: u32,
    /// A node's boot core is being written: the events say so.
    core_write: bool,
    /// The node being written, for its one progress number.
    write: percent::WriteTrack,
    /// Each node's trial period by topology index (`[watchdog] trial_ms`):
    /// how much of its progress number the trial weighs. Unset: only bytes count.
    pub trial_ms: Vec<(u8, u32)>,
    /// Lines about steps that are retried (a CONFIRM sent again), for the
    /// caller's log; `None`: not shown.
    pub note: Option<Note>,
}

impl<T: Transport, C: Clock> Host<T, C> {
    pub fn new(transport: T, clock: C) -> Self {
        Host {
            transport,
            clock,
            seq: 0,
            rx: Vec::new(),
            applied: Vec::new(),
            opened: Vec::new(),
            stale: true,
            node_poll_ms: ops::NODE_POLL_MS,
            core_write: false,
            write: percent::WriteTrack::default(),
            trial_ms: Vec::new(),
            note: None,
        }
    }

    fn note(&mut self, line: &str) {
        if let Some(f) = self.note.as_mut() {
            f(line);
        }
    }

    /// Drops everything received so far and what the link still holds:
    /// none of it can answer a request not sent yet.
    async fn drain(&mut self) -> Result<(), T::Error> {
        self.rx.clear();
        let mut buf = [0u8; READ_CHUNK];
        for _ in 0..DRAIN_READS {
            let n = self
                .transport
                .read(&mut buf)
                .await
                .map_err(Error::Transport)?;
            if n == 0 {
                break;
            }
        }
        self.stale = false;
        Ok(())
    }

    /// Closes and opens the link again (the hub's USB device comes back
    /// after REBOOT); what the old link held is not read as an answer.
    pub async fn reopen(&mut self) -> Result<(), T::Error> {
        self.transport.reopen().await.map_err(Error::Transport)?;
        self.stale = true;
        Ok(())
    }

    /// Largest payload a request to `to` may carry: the uplink's, and for a
    /// node also its link's and what fits inside a RELAY on the uplink.
    pub(crate) fn link_max(&self, to: Target) -> usize {
        let up = self.transport.max_payload();
        match to.node_link_max() {
            None => up,
            Some(node) => node.min(up.saturating_sub(Relay::HEAD_LEN + dagon_proto::frame_len(0))),
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

    fn next_seq(&mut self) -> u8 {
        self.seq = (self.seq + 1) & SEQ_MASK;
        self.seq
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
            match Frame::decode(&self.rx, self.transport.max_payload()) {
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

    /// Sends one request frame to the hub and waits for its response
    /// payload (status first).
    async fn exchange(
        &mut self,
        seq: u8,
        cmd: Cmd,
        frame: &[u8],
        wait_ms: u32,
    ) -> Result<Vec<u8>, T::Error> {
        self.transport
            .write(frame)
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
    ///
    /// A request in the plain-response class (`timeout_ms` =
    /// [`REQUEST_MS`]), or a CONFIRM, of a command that is safe to repeat is
    /// sent again
    /// when nothing answers or the node NACKs (RELAY E_TIMEOUT), up to
    /// [`RETRY_BACKOFF_MS`]`.len()` times. A resend keeps the seq (outer and
    /// inner) and sets the RETRY flag; receivers keep the last seq and reply
    /// and answer a repeat with that reply instead of running it again, so
    /// a request that did run is not run twice.
    pub async fn call(
        &mut self,
        to: Target,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Vec<u8>, T::Error> {
        let resend = timeout_ms == REQUEST_MS || cmd == Cmd::Confirm;
        let retries = if resend && repeatable(cmd) {
            RETRY_BACKOFF_MS.len()
        } else {
            0
        };
        self.call_with(to, cmd, payload, timeout_ms, retries).await
    }

    /// [`Host::call`] without resending (polls that repeat by themselves).
    pub async fn call_once(
        &mut self,
        to: Target,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Vec<u8>, T::Error> {
        self.call_with(to, cmd, payload, timeout_ms, 0).await
    }

    /// Opens the session with `to` on first use, then sends the request.
    ///
    /// Receivers keep the last (seq, cmd) and its reply across host
    /// sessions, and a new session restarts its seq count, so its first
    /// request could match the previous session's last one and get that
    /// stored reply. The first request to each target in a session is
    /// therefore an opening HELLO whose reply is dropped; only after the
    /// target has answered it does the caller's request go out, with a seq
    /// different from the HELLO's, so it cannot match what the target keeps.
    ///
    /// Replies of the previous session can also still be on the link. The
    /// target answers in order, so they come before this session's HELLO
    /// reply; but one of them could be taken for it (the same seq and
    /// command). What arrived with or after that reply, and anything left
    /// from before the session or a reopen, is read away before the
    /// caller's request is sent.
    async fn call_with(
        &mut self,
        to: Target,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
        retries: usize,
    ) -> Result<Vec<u8>, T::Error> {
        if self.stale {
            self.drain().await?;
        }
        if !self.opened.contains(&to) {
            let hello = hello_request(self.link_max(to));
            self.attempts(to, Cmd::Hello, &hello, timeout_ms.min(REQUEST_MS), retries)
                .await
                .or_else(|e| match e {
                    // An answer, even a refusal, means the target now keeps
                    // this session's seq.
                    Error::Rejected { .. } | Error::BadResponse => Ok(Vec::new()),
                    e => Err(e),
                })?;
            self.opened.push(to);
            self.drain().await?;
        }
        self.attempts(to, cmd, payload, timeout_ms, retries).await
    }

    /// One request, sent up to `retries` more times when unanswered.
    async fn attempts(
        &mut self,
        to: Target,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
        retries: usize,
    ) -> Result<Vec<u8>, T::Error> {
        let seq = self.next_seq();
        let relay = to.relay().map(|r| (r, self.next_seq()));
        let mut attempt = 0;
        loop {
            let fl = if attempt == 0 { 0 } else { flags::RETRY };
            let r = match relay {
                None => {
                    let frame = encode(fl, seq, cmd as u8, payload);
                    self.exchange(seq, cmd, &frame, timeout_ms).await
                }
                Some(((link, addr), outer)) => {
                    let inner = encode(fl, seq, cmd as u8, payload);
                    let relay = Relay {
                        link,
                        addr,
                        timeout_ms: timeout_ms.min(u16::MAX as u32) as u16,
                        inner: &inner,
                    };
                    let mut p = vec![0u8; Relay::HEAD_LEN + inner.len()];
                    relay.encode(&mut p).expect("sized for the frame");
                    let frame = encode(fl, outer, Cmd::Relay as u8, &p);
                    self.exchange(outer, Cmd::Relay, &frame, timeout_ms + REQUEST_MS)
                        .await
                        .and_then(|resp| relayed(&resp, to, seq, cmd))
                }
            };
            let again = matches!(r, Err(Error::Timeout | Error::Relay(Status::Timeout)));
            if !again || attempt >= retries {
                return r.and_then(|p| ok_body(cmd, &p));
            }
            self.clock.sleep_ms(RETRY_BACKOFF_MS[attempt]).await;
            attempt += 1;
        }
    }

    async fn hello_with(
        &mut self,
        to: Target,
        timeout_ms: u32,
        retry: bool,
    ) -> Result<HelloResp, T::Error> {
        let b = hello_request(self.link_max(to));
        let body = if retry {
            self.call(to, Cmd::Hello, &b, timeout_ms).await?
        } else {
            self.call_once(to, Cmd::Hello, &b, timeout_ms).await?
        };
        HelloResp::parse(&body).map_err(|_| Error::BadResponse)
    }

    /// One HELLO with no resend (discovery probes, polls after REBOOT).
    async fn hello_within(&mut self, to: Target, timeout_ms: u32) -> Result<HelloResp, T::Error> {
        self.hello_with(to, timeout_ms, false).await
    }

    pub async fn hello(&mut self, to: Target) -> Result<HelloResp, T::Error> {
        self.hello_with(to, REQUEST_MS, true).await
    }
}

fn hello_request(max: usize) -> [u8; 6] {
    let mut b = [0u8; 6];
    HelloReq {
        proto_ver: dagon_proto::PROTO_VERSION as u32,
        max_payload: max.min(u16::MAX as usize) as u16,
    }
    .encode(&mut b)
    .expect("6 bytes");
    b
}

/// Waits before each resend of a plain request.
pub const RETRY_BACKOFF_MS: [u32; 3] = [50, 100, 200];

/// Commands a resend cannot harm: reads, the hub display, and the transfer
/// and state steps the receiver deduplicates by seq (CONFIRM among them).
/// Not REBOOT or ENTER_ROM (a node that already reset, or left for its ROM
/// loader, has no memory of the seq), nor CONF_RUN (a conformance step
/// acts on the node every time it runs), nor the hub's line and raw I2C
/// controls (no seq memory on the hub side).
fn repeatable(cmd: Cmd) -> bool {
    !NEVER_RESENT.contains(&cmd)
}

/// Commands whose second run is not the first one's answer.
const NEVER_RESENT: [Cmd; 5] = [
    Cmd::Reboot,
    Cmd::EnterRom,
    Cmd::ConfRun,
    Cmd::SubCtl,
    Cmd::I2cRaw,
];

fn encode(fl: u8, seq: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; dagon_proto::frame_len(payload.len())];
    let f = Frame {
        flags: fl,
        seq,
        cmd,
        payload,
    };
    f.encode(&mut out).expect("sized for the payload");
    out
}

/// The node's response payload inside a RELAY response.
fn relayed<E>(resp: &[u8], to: Target, seq: u8, cmd: Cmd) -> Result<Vec<u8>, E> {
    let (status, body) = split_status(resp).ok_or(Error::BadResponse)?;
    if status != Status::Ok {
        return Err(Error::Relay(status));
    }
    let max = to.node_link_max().unwrap_or(max_payload::USB);
    let (f, _) = Frame::decode(body, max).map_err(|_| Error::BadResponse)?;
    if f.flags & flags::RESP == 0 || f.seq != seq || f.cmd != cmd as u8 {
        return Err(Error::BadResponse);
    }
    Ok(f.payload.to_vec())
}

fn ok_body<E>(cmd: Cmd, payload: &[u8]) -> Result<Vec<u8>, E> {
    match split_status(payload) {
        Some((Status::Ok, body)) => Ok(body.to_vec()),
        Some((status, _)) => Err(Error::Rejected { cmd, status }),
        None => Err(Error::BadResponse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_commands_a_resend_cannot_harm_are_resent() {
        let never = [
            Cmd::Reboot,
            Cmd::EnterRom,
            Cmd::ConfRun,
            Cmd::SubCtl,
            Cmd::I2cRaw,
        ];
        for c in Cmd::ALL {
            assert_eq!(repeatable(c), !never.contains(&c), "{c:?}");
        }
        // The rest are reads, the hub display and steps the receiver
        // deduplicates by seq.
        for c in [
            Cmd::Hello,
            Cmd::Status,
            Cmd::Version,
            Cmd::XferBegin,
            Cmd::XferData,
            Cmd::XferEnd,
            Cmd::XferHdr,
            Cmd::SetPending,
            Cmd::Confirm,
            Cmd::Show,
            Cmd::Relay,
        ] {
            assert!(repeatable(c), "{c:?}");
        }
    }
}
