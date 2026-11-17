//! DAGON link v2: frame format (shared by USB CDC, UART and I2C), the
//! command table, status codes, the field grammar of every payload with
//! its one reader and writer, the typed payloads and the I2C mailbox state.
#![no_std]
#![forbid(unsafe_code)]

pub mod field;
pub mod payload;
pub mod table;

pub use table::Cmd;

use dagon_format::crc32;
use dagon_format::le::{put_u16, u16_at, u32_at};

pub const SOF: [u8; 2] = [0xDA, 0x6F];
pub const PROTO_VERSION: u8 = 0x02;
pub const HEADER_LEN: usize = 8;
pub const CRC_LEN: usize = 4;

pub mod flags {
    pub const RESP: u8 = 1 << 0;
    pub const EVENT: u8 = 1 << 1;
    pub const RETRY: u8 = 1 << 2;
}

/// Payload limit per link; frames above it are dropped.
pub mod max_payload {
    pub const USB: usize = 1088;
    pub const UART: usize = 1024;
    pub const I2C: usize = 256;
}

pub const fn frame_len(payload_len: usize) -> usize {
    HEADER_LEN + payload_len + CRC_LEN
}

/// Largest frame on any link (USB CDC).
pub const MAX_FRAME: usize = frame_len(max_payload::USB);

/// First payload byte of every response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Ok = 0x00,
    Frame = 0x01,
    Unsupported = 0x02,
    Arg = 0x03,
    /// A must field the receiver does not know; the body names it
    /// (`table::error::BAD_FIELD`).
    Field = 0x04,
    State = 0x05,
    Range = 0x06,
    Flash = 0x07,
    Digest = 0x08,
    Signature = 0x09,
    Key = 0x0A,
    Compat = 0x0B,
    Timeout = 0x0F,
}

impl Status {
    pub const ALL: [Status; 13] = [
        Status::Ok,
        Status::Frame,
        Status::Unsupported,
        Status::Arg,
        Status::Field,
        Status::State,
        Status::Range,
        Status::Flash,
        Status::Digest,
        Status::Signature,
        Status::Key,
        Status::Compat,
        Status::Timeout,
    ];

    pub fn from_u8(v: u8) -> Option<Status> {
        Status::ALL.into_iter().find(|s| *s as u8 == v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Need more bytes.
    Incomplete,
    BadSof,
    BadVersion,
    /// `len` above the link's max payload.
    TooLong,
    BadCrc,
    BufferTooSmall,
    /// A field written out of rising order, numbered 0 or outside the
    /// lengths its table row gives.
    Field,
}

/// A decoded frame borrowing its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub flags: u8,
    pub seq: u8,
    /// Raw command byte; see [`Cmd::from_u8`].
    pub cmd: u8,
    pub payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Decodes one frame at the start of `buf`; returns it and its length.
    /// For byte streams, resynchronise with [`find_sof`] after an error.
    pub fn decode(buf: &'a [u8], max_payload: usize) -> Result<(Frame<'a>, usize), FrameError> {
        if buf.iter().zip(SOF).any(|(&b, s)| b != s) {
            return Err(FrameError::BadSof);
        }
        if buf.len() < HEADER_LEN {
            return Err(FrameError::Incomplete);
        }
        if buf[2] != PROTO_VERSION {
            return Err(FrameError::BadVersion);
        }
        let len = u16_at(buf, 6) as usize;
        if len > max_payload {
            return Err(FrameError::TooLong);
        }
        let total = frame_len(len);
        if buf.len() < total {
            return Err(FrameError::Incomplete);
        }
        if crc32(&buf[2..HEADER_LEN + len]) != u32_at(buf, HEADER_LEN + len) {
            return Err(FrameError::BadCrc);
        }
        let frame = Frame {
            flags: buf[3],
            seq: buf[4],
            cmd: buf[5],
            payload: &buf[HEADER_LEN..HEADER_LEN + len],
        };
        Ok((frame, total))
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let n = self.payload.len();
        if out.len() < frame_len(n) {
            return Err(FrameError::BufferTooSmall);
        }
        out[HEADER_LEN..HEADER_LEN + n].copy_from_slice(self.payload);
        finish(out, self.flags, self.seq, self.cmd, n)
    }
}

/// Completes a frame whose payload is already at `out[8..8 + payload_len]`:
/// writes the header and CRC, returns the frame length.
pub fn finish(
    out: &mut [u8],
    flags: u8,
    seq: u8,
    cmd: u8,
    payload_len: usize,
) -> Result<usize, FrameError> {
    let total = frame_len(payload_len);
    if out.len() < total || payload_len > u16::MAX as usize {
        return Err(FrameError::BufferTooSmall);
    }
    out[..2].copy_from_slice(&SOF);
    out[2] = PROTO_VERSION;
    out[3] = flags;
    out[4] = seq;
    out[5] = cmd;
    put_u16(out, 6, payload_len as u16);
    let crc = crc32(&out[2..HEADER_LEN + payload_len]);
    out[HEADER_LEN + payload_len..total].copy_from_slice(&crc.to_le_bytes());
    Ok(total)
}

/// The answer to a request frame of a wire version this crate does not
/// read, so that its sender is told instead of left to time out: a frame
/// of this version echoing bytes 4 and 5 (seq and cmd here), with
/// `Status::Frame` and the version read here (`table::error::WIRE`).
///
/// `None` for anything else: a frame of this version, a response or an
/// event (so two sides of different versions never answer each other's
/// answers), or fewer than [`HEADER_LEN`] bytes. The start bytes, the
/// version at byte 2 and the flags at byte 3 are where every version
/// keeps them. Stream cutters hand a frame that [`Frame::decode`] refuses
/// with [`FrameError::BadVersion`] over as its first [`HEADER_LEN`] bytes.
pub fn version_refusal(request: &[u8], out: &mut [u8]) -> Option<usize> {
    let head = request.get(..HEADER_LEN)?;
    let (&[s0, s1, ver, fl, seq, cmd], _) = head.split_first_chunk::<6>()?;
    if [s0, s1] != SOF || ver == PROTO_VERSION || fl & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let body = out.get_mut(HEADER_LEN..)?;
    *body.first_mut()? = Status::Frame as u8;
    let mut w = field::FieldWriter::new(body.get_mut(1..)?);
    w.field(&table::error::WIRE, &[PROTO_VERSION]).ok()?;
    let n = 1 + w.len();
    finish(out, flags::RESP, seq, cmd, n).ok()
}

/// The wire version of the peer when `frame` is its answer to the request
/// (`seq`, `cmd`) in another wire version, as [`version_refusal`] makes it:
/// read from the bytes every version keeps in place (start, version, flags,
/// then the echoed seq and cmd).
pub fn refusing_version(frame: &[u8], seq: u8, cmd: u8) -> Option<u8> {
    let (&[s0, s1, ver, fl, q, c], _) = frame.split_first_chunk::<6>()?;
    let answers = [s0, s1] == SOF && fl & flags::RESP != 0 && (q, c) == (seq, cmd);
    (answers && ver != PROTO_VERSION).then_some(ver)
}

/// Length of the refused frame's start a stream cutter hands over.
pub const REFUSED_LEN: usize = HEADER_LEN;

/// Whether a stream cutter hands the [`REFUSED_LEN`] bytes at the start of
/// `buf` over, after [`Frame::decode`] refused them with
/// [`FrameError::BadVersion`]. Not when another frame start lies within
/// them: that is noise before a frame, and taking it would take the
/// frame's first bytes with it.
pub fn is_refused_start(buf: &[u8]) -> bool {
    buf.get(1..REFUSED_LEN)
        .is_some_and(|rest| !rest.contains(&SOF[0]))
}

/// Offset of the next possible frame start (`0xDA`) at or after `from`.
pub fn find_sof(buf: &[u8], from: usize) -> Option<usize> {
    buf.get(from..)?
        .iter()
        .position(|&b| b == SOF[0])
        .map(|p| p + from)
}

/// Splits a response payload into its status and body.
pub fn split_status(payload: &[u8]) -> Option<(Status, &[u8])> {
    let (&s, body) = payload.split_first()?;
    Some((Status::from_u8(s)?, body))
}

/// I2C mailbox: the 3-byte state the hub polls after writing a request.
pub mod mailbox {
    pub const STATE_LEN: usize = 3;
    pub const BUSY: u8 = 0x00;
    pub const READY: u8 = 0x01;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum State {
        Busy,
        /// Response frame of this many bytes is ready to read.
        Ready(u16),
    }

    impl State {
        pub fn to_bytes(self) -> [u8; STATE_LEN] {
            match self {
                State::Busy => [BUSY, 0, 0],
                State::Ready(n) => {
                    let l = n.to_le_bytes();
                    [READY, l[0], l[1]]
                }
            }
        }

        pub fn parse(b: [u8; STATE_LEN]) -> Option<State> {
            match b[0] {
                BUSY => Some(State::Busy),
                READY => Some(State::Ready(u16::from_le_bytes([b[1], b[2]]))),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_errors() {
        let mut buf = [0u8; 64];
        let f = Frame {
            flags: 0,
            seq: 7,
            cmd: Cmd::Hello as u8,
            payload: &[1, 2, 3],
        };
        let n = f.encode(&mut buf).unwrap();
        assert_eq!(n, 15);
        let (g, used) = Frame::decode(&buf[..n], max_payload::I2C).unwrap();
        assert_eq!((g, used), (f, n));
        assert_eq!(
            Frame::decode(&buf[..n - 1], 256),
            Err(FrameError::Incomplete)
        );
        assert_eq!(Frame::decode(&buf[..n], 2), Err(FrameError::TooLong));
        buf[9] ^= 1;
        assert_eq!(Frame::decode(&buf[..n], 256), Err(FrameError::BadCrc));
        assert_eq!(Frame::decode(b"xx", 256), Err(FrameError::BadSof));
        assert_eq!(Frame::decode(b"x", 256), Err(FrameError::BadSof));
        assert_eq!(Frame::decode(b"\xDAx", 256), Err(FrameError::BadSof));
        assert_eq!(Frame::decode(b"\xDA", 256), Err(FrameError::Incomplete));
        assert_eq!(find_sof(b"boot log\xDA\x6F", 0), Some(8));
    }

    #[test]
    fn command_and_status_tables() {
        assert_eq!(Cmd::ALL.len(), 17);
        assert_eq!(Cmd::from_u8(0x03), Some(Cmd::Version));
        assert_eq!(Cmd::from_u8(0x24), Some(Cmd::EnterRom));
        assert_eq!(Status::ALL.len(), 13);
        assert_eq!(Cmd::from_u8(0x15), Some(Cmd::XferHdr));
        assert_eq!(Cmd::from_u8(0x04), None);
        assert_eq!(Status::from_u8(0x0F), Some(Status::Timeout));
        assert_eq!(Status::from_u8(0x04), Some(Status::Field));
    }

    /// STAY (0x05): an empty request a boot core takes in its listen
    /// window; the number is the design's and new beside the others.
    #[test]
    fn stay_round_trips() {
        assert_eq!(Cmd::Stay as u8, 0x05);
        assert_eq!(Cmd::from_u8(0x05), Some(Cmd::Stay));
        let mut buf = [0u8; 16];
        let f = Frame {
            flags: 0,
            seq: 9,
            cmd: Cmd::Stay as u8,
            payload: &[],
        };
        let n = f.encode(&mut buf).unwrap();
        assert_eq!(n, frame_len(0));
        assert_eq!(Frame::decode(&buf[..n], max_payload::I2C), Ok((f, n)));
        let mut seen = [false; 256];
        for c in Cmd::ALL {
            assert!(!seen[c as usize], "{c:?} reuses a number");
            seen[c as usize] = true;
        }
    }

    #[test]
    fn mailbox_state() {
        let s = mailbox::State::Ready(268);
        assert_eq!(s.to_bytes(), [1, 0x0C, 0x01]);
        assert_eq!(mailbox::State::parse(s.to_bytes()), Some(s));
        assert_eq!(mailbox::State::parse([0, 0, 0]), Some(mailbox::State::Busy));
    }
}
