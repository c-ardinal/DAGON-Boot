//! Port contract: what a board provides to DAGON.
//!
//! Node side: [`Flash`], [`Link`] (with [`LinkKind`], [`LinkError`],
//! [`Reconnect`], [`NO_ADDR`]), [`Reset`], boot selection ([`BootSelectS`]
//! for kind S, [`BootSelectV`] for kind V), [`Watchdog`].
//! Hub side: [`StreamPort`], [`I2cBus`], [`SubControl`], and [`NodeLink`]
//! for each link the relay reaches other nodes over (with its HELLO frames,
//! [`hello_request`], [`hello_answer`] and [`hello_reply`]).
//!
//! Link-type and hub-side traits are async (they wait on peripherals or on a
//! remote hub); flash and watchdog are blocking. This crate only
//! declares traits; `unsafe` belongs in the board implementations (jump,
//! RAM-resident flash routines, registers).
//!
//! Compatibility rule: these traits, [`Link`] included, are frozen. Any
//! method added later comes with a default implementation, so existing
//! ports and links keep compiling. [`LinkKind`] is non-exhaustive: a new
//! kind is one entry of `dagon_format::with_link_kinds!`.
#![no_std]
#![allow(async_fn_in_trait)]

mod relay_link;
pub mod timeout;

pub use relay_link::{Addressing, RelayLinkDef};

use dagon_format::{CryptoProvider as _, Digest as _};
use dagon_proto::payload::{HelloReq, HelloResp};
use dagon_proto::{split_status, Cmd, Frame, Status, PROTO_VERSION};

/// How a board switches between A/B images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PortKind {
    /// Kind S: a DAGON boot core picks and jumps to the slot.
    SelfBoot = 0x01,
    /// Kind V: the vendor second-stage loader picks the slot.
    VendorStage2 = 0x02,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashRegion {
    pub start: u32,
    pub len: u32,
    pub erase_size: u32,
    pub write_size: u32,
    pub erased_value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashError {
    /// Address range outside every region.
    Range,
    /// Not aligned to the erase or write unit.
    Align,
    /// Erase, program or read-back failure.
    Hw,
}

/// Contract: erase only whole erase units; write only aligned to the write
/// unit into erased memory; `read` reflects completed erase/write.
pub trait Flash {
    fn geometry(&self) -> &[FlashRegion];
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError>;
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError>;
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError>;

    /// The SHA-256 of the `len` bytes at `addr` (an image body). The
    /// default reads them through [`Flash::read`] into the software
    /// SHA-256; a port with a hash peripheral overrides it.
    fn sha256(&mut self, addr: u32, len: u32) -> Result<[u8; 32], FlashError> {
        let mut sha = dagon_format::Software.sha256();
        let mut buf = [0u8; 256];
        let mut done = 0u32;
        while done < len {
            let n = (len - done).min(buf.len() as u32) as usize;
            let at = addr.checked_add(done).ok_or(FlashError::Range)?;
            self.read(at, &mut buf[..n])?;
            sha.update(&buf[..n]);
            done += n as u32;
        }
        Ok(sha.finalize())
    }
}

/// `app.link_up` / `app.link_down` `addr` of a link without an address.
pub const NO_ADDR: u8 = 0xFF;

/// Why a frame could not be read or written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// Nothing complete arrived within the link's own wait (a tick, not a
    /// failure: serve loops use it to drop abandoned transfers).
    Timeout,
    /// A frame longer than the link carries.
    Overflow,
    /// What arrived is not a valid frame (checked by the link).
    Corrupt,
    /// The peer went away or the medium failed.
    Closed,
}

/// What the peer sees when this end restarts (a reboot after an update).
/// The same split as dagon-host's `Reconnect` (`Reopen` / `Stays`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconnect {
    /// The link goes away with this end and the peer opens it again: a USB
    /// device re-enumerates; a TCP connection (wifi-tcp, ethernet) or a BLE
    /// connection is dropped by the restart and connected anew.
    Reenumerates,
    /// The link stays (UART, I2C, CAN): the peer keeps it open and simply
    /// sees silence while the node restarts.
    Persistent,
}

/// One end of a DAGON hop carrying whole frames (frozen wire format,
/// unchanged by the medium): up to the PC or a parent node, or down to a
/// child node.
///
/// An I2C target implementation runs the mailbox: after `read_frame`
/// returns, the node may erase or write flash before answering, so the
/// implementation must never stretch the clock in that window (NACK the
/// own address, or answer BUSY from RAM).
pub trait Link {
    /// Largest payload one frame may carry on this link.
    fn max_payload(&self) -> usize;

    /// What the peer sees when this end restarts.
    fn reconnect(&self) -> Reconnect;

    /// The link's kind (its code goes into `app.link_up`, `app.request`).
    fn kind(&self) -> LinkKind;

    /// This node's own address on the link (`app.link_up` `addr`): the I2C
    /// target address; [`NO_ADDR`] where a link has none.
    fn addr(&self) -> u8 {
        NO_ADDR
    }

    /// Waits for the next complete frame; the slice stays valid until the
    /// next call. [`LinkError::Timeout`] after the link's own tick with
    /// nothing complete.
    async fn read_frame(&mut self) -> Result<&[u8], LinkError>;

    /// Sends one frame.
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError>;

    /// Done with the request just read without answering it (not a valid
    /// request, or not for this node): the link must be ready for the next
    /// request (an I2C target stops NACKing; the request stays unanswered
    /// and the peer resends). Default: nothing to do.
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        Ok(())
    }

    /// Returns once everything written has left this end (the peer has
    /// read it, or it is on the wire), so a reset right after loses
    /// nothing. Default: at once.
    async fn flush(&mut self) -> Result<(), LinkError> {
        Ok(())
    }
}

/// Generates [`LinkKind`] from `dagon_format::with_link_kinds!`.
macro_rules! link_kind_enum {
    (() $( $v:ident = $c:literal, $n:literal; )*) => {
        /// A hop's medium, as the 構成 names it. The discriminant is the
        /// kind's code in events. Generated from the one list of link kinds
        /// (`dagon_format::with_link_kinds!`).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        #[non_exhaustive]
        pub enum LinkKind {
            $( $v = $c, )*
        }

        impl LinkKind {
            /// Every kind, in code order.
            pub const ALL: &'static [LinkKind] = &[$(LinkKind::$v),*];

            /// The kind's code in events (`app.link_up` `link`,
            /// `app.request` `via`).
            pub const fn code(self) -> u8 {
                self as u8
            }

            /// The 構成's name of the kind.
            pub const fn name(self) -> &'static str {
                match self {
                    $( LinkKind::$v => $n, )*
                }
            }
        }
    };
}

dagon_format::with_link_kinds!([link_kind_enum]());

impl LinkKind {
    /// The kind the 構成 names `name`; `None` for a name not in the list.
    pub fn from_name(name: &str) -> Option<LinkKind> {
        LinkKind::ALL.iter().copied().find(|k| k.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetCause {
    PowerOn,
    Watchdog,
    Software,
    Pin,
    Unknown,
}

pub trait Reset {
    /// Normal system reset.
    fn reset(&mut self) -> !;

    fn reset_cause(&self) -> ResetCause {
        ResetCause::Unknown
    }
}

/// Kind S: start the image whose body (vector table) begins at `entry`.
pub trait BootSelectS {
    fn boot(&mut self, entry: u32) -> !;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootSelectError {
    /// Vendor selection data could not be written or read back.
    Write,
    /// The slot is not a valid boot candidate.
    Slot,
}

/// Kind V: drive the vendor loader's selection data (ESP: otadata).
/// Slots: 0 = A, 1 = B, 2 = factory.
pub trait BootSelectV {
    fn current_slot(&self) -> u8;
    fn set_next(&mut self, slot: u8) -> Result<(), BootSelectError>;
    fn select_factory(&mut self) -> Result<(), BootSelectError>;
}

/// Resets the system unless fed within the timeout after `start`.
pub trait Watchdog {
    fn start(&mut self, timeout_ms: u32);
    fn feed(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError {
    Nack,
    Timeout,
    Io,
}

/// Hub: point-to-point byte stream to a UART node (hardware or PIO UART).
pub trait StreamPort {
    /// Reads at least one byte, waiting up to `timeout_ms`; returns the count.
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError>;
    async fn write(&mut self, data: &[u8]) -> Result<(), BusError>;
}

/// Hub: master on the shared I2C bus. `addr` is the 7-bit address.
pub trait I2cBus {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError>;
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError>;
    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError>;
}

/// The node's own read-only HELLO request frame (`seq` from the 0x80..=0xFF
/// space), offering `max` payload bytes; returns its length.
pub fn hello_request(max: usize, seq: u8, out: &mut [u8; 32]) -> Option<usize> {
    let body = HelloReq {
        proto_ver: PROTO_VERSION as u32,
        max_payload: max as u16,
    }
    .encode(&mut out[8..])
    .ok()?;
    dagon_proto::finish(out, 0, seq, Cmd::Hello as u8, body).ok()
}

/// The HELLO answer in the response frame `resp`; `None` for a frame that
/// is not a successful answer.
pub fn hello_answer(resp: &[u8], max: usize) -> Option<HelloResp> {
    let (f, _) = Frame::decode(resp, max).ok()?;
    match split_status(f.payload)? {
        (Status::Ok, b) => HelloResp::parse(b).ok(),
        _ => None,
    }
}

/// Why the node at the other end gave no HELLO answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoHello {
    /// Nothing that answers the request: silence, noise, a refusal of
    /// this wire version's HELLO by status, or a broken frame.
    NoAnswer,
    /// The node speaks another wire version, the one it names: it refused
    /// the request as [`dagon_proto::version_refusal`] makes the refusal.
    WireVersion(u8),
}

/// The HELLO answer in `resp`, the response frame to the HELLO request
/// `seq`; a refusal in another wire version is told apart from no answer.
pub fn hello_reply(resp: &[u8], max: usize, seq: u8) -> Result<HelloResp, NoHello> {
    if let Some(v) = dagon_proto::refusing_version(resp, seq, Cmd::Hello as u8) {
        return Err(NoHello::WireVersion(v));
    }
    hello_answer(resp, max).ok_or(NoHello::NoAnswer)
}

/// Room for the longest HELLO answer frame in [`NodeLink::hello`]: its
/// status, then the longest body. A longer frame is not a HELLO answer.
const HELLO_ANSWER_MAX: usize = dagon_proto::frame_len(1 + HelloResp::MAX_LEN);

/// Another node, reachable over this link at an address. The link has no
/// direction: the node at the other end may be anywhere in the 構成. A
/// link whose medium has no addresses (a point-to-point UART) ignores
/// `addr`.
pub trait NodeLink {
    /// The link's id in relayed requests (`dagon_proto::payload::link`).
    fn link_id(&self) -> u8;

    /// Largest payload one frame may carry on this link.
    fn max_payload(&self) -> usize;

    /// How long [`NodeLink::hello`] waits for the answer.
    fn hello_timeout_ms(&self) -> u16;

    /// Sends the request frame `frame` to the node at `addr` and stores its
    /// response frame in `out`; returns the response's length.
    /// [`Status::Timeout`] when no response is complete within `timeout_ms`.
    async fn exchange(
        &mut self,
        addr: u8,
        frame: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status>;

    /// Asks the node at `addr` for its HELLO with request `seq`; `None`
    /// without a valid answer within [`NodeLink::hello_timeout_ms`].
    async fn hello(&mut self, addr: u8, seq: u8) -> Option<HelloResp> {
        self.ask_hello(addr, seq).await.ok()
    }

    /// [`NodeLink::hello`], telling a node of another wire version
    /// ([`NoHello::WireVersion`]) apart from one that does not answer.
    async fn ask_hello(&mut self, addr: u8, seq: u8) -> Result<HelloResp, NoHello> {
        let max = self.max_payload();
        let mut req = [0u8; 32];
        let n = hello_request(max, seq, &mut req).ok_or(NoHello::NoAnswer)?;
        let mut resp = [0u8; HELLO_ANSWER_MAX];
        let timeout_ms = self.hello_timeout_ms();
        let len = self
            .exchange(addr, &req[..n], timeout_ms, &mut resp)
            .await
            .map_err(|_| NoHello::NoAnswer)?;
        hello_reply(&resp[..len], max, seq)
    }

    /// Raw bus access to `addr`, outside DAGON frames: one write
    /// transaction of `write` (if not empty), then one read transaction
    /// filling `read` (if not empty). Default: [`Status::Unsupported`].
    async fn raw(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), Status> {
        let _ = (addr, write, read);
        Err(Status::Unsupported)
    }
}

/// NRST action, as in the SUBCTL command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Nrst {
    /// Release (Hi-Z).
    Release = 0,
    /// Hold low.
    Low = 1,
    /// Low for about 10 ms, then release; timed by the hub.
    Pulse = 2,
}

/// Hub: control lines of a sub MCU. NRST is only pulled low or released
/// (Hi-Z), never driven high.
///
/// Implemented on the hub over GPIO, and on the PC side by sending one SUBCTL
/// frame per call and awaiting the reply (a hub timeout maps to
/// [`BusError::Timeout`]).
pub trait SubControl {
    /// Sets BOOT0 (`true` = high, ROM loader on the next reset) and applies
    /// `nrst`, in that order, so a [`Nrst::Pulse`] samples the new BOOT0.
    /// Returns after the action (including the whole pulse) is done.
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError>;

    /// Resets the sub MCU with a ~10 ms NRST pulse, BOOT0 as given.
    async fn pulse_reset(&mut self, boot0_high: bool) -> Result<(), BusError> {
        self.set_lines(Nrst::Pulse, boot0_high).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_kind_names_round_trip() {
        for k in LinkKind::ALL {
            assert_eq!(LinkKind::from_name(k.name()), Some(*k));
        }
        assert_eq!(LinkKind::from_name("usb"), None);
    }

    /// Polls a future that never waits.
    fn ready<F: core::future::Future>(f: F) -> F::Output {
        let mut f = core::pin::pin!(f);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(v) => v,
            core::task::Poll::Pending => panic!("the future waited"),
        }
    }

    /// Answers a HELLO from `addr` only, and records the request.
    struct OneNode {
        addr: u8,
        asked: Option<(u8, u16, [u8; 32], usize)>,
    }

    impl NodeLink for OneNode {
        fn link_id(&self) -> u8 {
            7
        }
        fn max_payload(&self) -> usize {
            256
        }
        fn hello_timeout_ms(&self) -> u16 {
            40
        }
        async fn exchange(
            &mut self,
            addr: u8,
            frame: &[u8],
            timeout_ms: u16,
            out: &mut [u8],
        ) -> Result<usize, Status> {
            let mut req = [0u8; 32];
            req[..frame.len()].copy_from_slice(frame);
            self.asked = Some((addr, timeout_ms, req, frame.len()));
            if addr != self.addr {
                return Err(Status::Timeout);
            }
            let h = HelloResp {
                proto_ver: 1,
                role: 1,
                port_kind: 2,
                mcu_id: 3,
                mode: 0,
                active_version: dagon_format::Version::new(1, 2, 0),
                trial: false,
                key_id: [0; 8],
                max_payload: 256,
                // Every command number: the longest answer.
                cmds: dagon_proto::payload::CmdSet::from_bytes(&[0xFF; 32]).unwrap(),
            };
            out[8] = Status::Ok as u8;
            let n = h.encode(&mut out[9..]).unwrap();
            Ok(dagon_proto::finish(
                out,
                dagon_proto::flags::RESP,
                frame[4],
                Cmd::Hello as u8,
                1 + n,
            )
            .unwrap())
        }
    }

    #[test]
    fn hello_asks_with_the_links_payload_and_timeout() {
        let mut link = OneNode {
            addr: 0x42,
            asked: None,
        };
        let h = ready(link.hello(0x42, 0x85)).expect("answer");
        assert_eq!(h.active_version, dagon_format::Version::new(1, 2, 0));
        let (addr, timeout_ms, req, n) = link.asked.unwrap();
        assert_eq!((addr, timeout_ms), (0x42, 40));
        let (f, _) = Frame::decode(&req[..n], 256).unwrap();
        assert_eq!((f.seq, f.cmd), (0x85, Cmd::Hello as u8));
        assert_eq!(HelloReq::parse(f.payload).unwrap().max_payload, 256);
    }

    /// A node of the next wire version: it refuses every request as
    /// `version_refusal` makes the refusal, in its own version.
    struct NextWire;

    impl NodeLink for NextWire {
        fn link_id(&self) -> u8 {
            7
        }
        fn max_payload(&self) -> usize {
            256
        }
        fn hello_timeout_ms(&self) -> u16 {
            40
        }
        async fn exchange(
            &mut self,
            _addr: u8,
            frame: &[u8],
            _timeout_ms: u16,
            out: &mut [u8],
        ) -> Result<usize, Status> {
            // The request as the node reads it: of a version not its own.
            let mut theirs = [0u8; 32];
            theirs[..frame.len()].copy_from_slice(frame);
            theirs[2] = PROTO_VERSION + 1;
            let n = dagon_proto::version_refusal(&theirs[..frame.len()], out).unwrap();
            out[2] = PROTO_VERSION + 1;
            Ok(n)
        }
    }

    #[test]
    fn a_node_of_another_wire_version_is_told_apart_from_silence() {
        assert_eq!(
            ready(NextWire.ask_hello(0x42, 0x87)),
            Err(NoHello::WireVersion(PROTO_VERSION + 1))
        );
        assert!(ready(NextWire.hello(0x42, 0x87)).is_none());
        let mut link = OneNode {
            addr: 0x42,
            asked: None,
        };
        assert_eq!(ready(link.ask_hello(0x43, 0x88)), Err(NoHello::NoAnswer));
        assert!(ready(link.ask_hello(0x42, 0x89)).is_ok());
    }

    #[test]
    fn hello_without_an_answer_is_none() {
        let mut link = OneNode {
            addr: 0x42,
            asked: None,
        };
        assert!(ready(link.hello(0x43, 0x86)).is_none());
    }

    #[test]
    fn raw_is_unsupported_by_default() {
        let mut link = OneNode {
            addr: 0x42,
            asked: None,
        };
        assert_eq!(
            ready(link.raw(0x42, &[1], &mut [0; 1])),
            Err(Status::Unsupported)
        );
    }

    #[test]
    fn link_kind_codes_are_distinct_and_never_zero() {
        for (i, k) in LinkKind::ALL.iter().enumerate() {
            assert_ne!(k.code(), 0);
            assert!(LinkKind::ALL[i + 1..].iter().all(|o| o.code() != k.code()));
        }
    }
}
