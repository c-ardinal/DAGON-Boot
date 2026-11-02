//! Cuts DAGON frames out of a UART byte stream: SOF search, length bound and
//! CRC come from `dagon_proto::Frame::decode`; anything that fails is skipped
//! up to the next possible SOF.

use dagon_proto::{find_sof, frame_len, max_payload, Frame, FrameError, SOF};

/// Largest frame on the UART link (1024-byte payload).
pub const MAX_UART_FRAME: usize = frame_len(max_payload::UART);
/// A partial frame is dropped after this long without a byte.
pub const INTER_BYTE_TIMEOUT_MS: u32 = 20;

pub struct FrameCutter {
    buf: [u8; MAX_UART_FRAME],
    len: usize,
    /// Length of the frame handed out by the last `push`, dropped on the next.
    done: usize,
    /// Arrival time (ms, wrapping) of the last byte given to `push_at`.
    last_ms: u32,
}

impl Default for FrameCutter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCutter {
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_UART_FRAME],
            len: 0,
            done: 0,
            last_ms: 0,
        }
    }

    /// [`FrameCutter::push`] for a byte that arrived at `now_ms`: a partial
    /// frame older than [`INTER_BYTE_TIMEOUT_MS`] is dropped first, and the
    /// search for the next SOF starts over.
    pub fn push_at(&mut self, byte: u8, now_ms: u32) -> Option<&[u8]> {
        if self.len > self.done && now_ms.wrapping_sub(self.last_ms) >= INTER_BYTE_TIMEOUT_MS {
            self.clear();
        }
        self.last_ms = now_ms;
        self.push(byte)
    }

    /// Forgets any partial frame (after a receive error).
    pub fn clear(&mut self) {
        self.len = 0;
        self.done = 0;
    }

    /// Feeds one byte; returns the frame it completed, valid until the next call.
    pub fn push(&mut self, byte: u8) -> Option<&[u8]> {
        if self.done > 0 {
            self.drop_front(self.done);
            self.done = 0;
        }
        if self.len == 0 && byte != SOF[0] {
            return None;
        }
        // Never full here: an incomplete frame is shorter than MAX_UART_FRAME,
        // longer ones fail decode with TooLong and get skipped.
        self.buf[self.len] = byte;
        self.len += 1;
        while self.len > 0 {
            match Frame::decode(&self.buf[..self.len], max_payload::UART).map(|(_, n)| n) {
                Ok(n) => {
                    self.done = n;
                    return Some(&self.buf[..n]);
                }
                Err(FrameError::Incomplete) => return None,
                Err(_) => {
                    let next = find_sof(&self.buf[..self.len], 1).unwrap_or(self.len);
                    self.drop_front(next);
                }
            }
        }
        None
    }

    fn drop_front(&mut self, n: usize) {
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::{finish, HEADER_LEN};

    fn frame(seq: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0; frame_len(payload.len())];
        out[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
        finish(&mut out, 0, seq, 0x01, payload.len()).unwrap();
        out
    }

    fn cut(stream: &[u8]) -> Vec<Vec<u8>> {
        let mut c = FrameCutter::new();
        stream
            .iter()
            .filter_map(|&b| c.push(b).map(<[u8]>::to_vec))
            .collect()
    }

    #[test]
    fn cuts_back_to_back_frames() {
        let (a, b) = (frame(1, b"hello"), frame(2, &[]));
        assert_eq!(cut(&[a.clone(), b.clone()].concat()), vec![a, b]);
    }

    #[test]
    fn skips_boot_log_and_stray_sof_bytes() {
        let f = frame(7, &[0xDA, 0x6F, 0xDA]);
        let stream = [b"ESP-ROM:esp32s3\r\n\xDA\xDA\x00".as_slice(), &f].concat();
        assert_eq!(cut(&stream), vec![f]);
    }

    #[test]
    fn drops_bad_crc_and_resyncs() {
        let good = frame(3, b"payload");
        let mut bad = frame(4, b"payload");
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert_eq!(cut(&[bad, good.clone()].concat()), vec![good]);
    }

    #[test]
    fn rejects_oversized_length_without_waiting_for_it() {
        let mut huge = frame(5, &[]);
        huge[6..8].copy_from_slice(&((max_payload::UART + 1) as u16).to_le_bytes());
        let good = frame(6, b"x");
        assert_eq!(cut(&[huge, good.clone()].concat()), vec![good]);
    }

    #[test]
    fn accepts_max_payload_frame() {
        let f = frame(8, &[0x55; max_payload::UART]);
        assert_eq!(f.len(), MAX_UART_FRAME);
        assert_eq!(cut(&[f.clone(), f.clone()].concat()), vec![f.clone(), f]);
    }

    #[test]
    fn a_20_ms_gap_drops_the_partial_frame() {
        let f = frame(10, b"abcdef");
        let mut c = FrameCutter::new();
        // Head of a frame, then silence: the rest alone never completes.
        for &b in &f[..6] {
            assert!(c.push_at(b, 100).is_none());
        }
        let rest: Vec<_> = f[6..]
            .iter()
            .filter_map(|&b| c.push_at(b, 120).map(<[u8]>::to_vec))
            .collect();
        assert!(rest.is_empty());
        // A whole frame with gaps below 20 ms goes through.
        let got: Vec<_> = f
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| c.push_at(b, 200 + 19 * i as u32).map(<[u8]>::to_vec))
            .collect();
        assert_eq!(got, vec![f.clone()]);
        // A completed frame is not a partial one: the next frame may start
        // much later.
        let got: Vec<_> = f
            .iter()
            .filter_map(|&b| c.push_at(b, 5_000).map(<[u8]>::to_vec))
            .collect();
        assert_eq!(got, vec![f]);
    }

    #[test]
    fn clear_discards_partial_frame() {
        let f = frame(9, b"abc");
        let mut c = FrameCutter::new();
        for &b in &f[..5] {
            assert!(c.push(b).is_none());
        }
        c.clear();
        let got: Vec<Vec<u8>> = f
            .iter()
            .filter_map(|&b| c.push(b).map(<[u8]>::to_vec))
            .collect();
        assert_eq!(got, vec![f]);
    }
}
