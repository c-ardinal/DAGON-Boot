//! Cuts DAGON frames out of a byte stream (USB CDC, UART): SOF search,
//! length and CRC check, resync on garbage, and (for UART links, through
//! [`FrameReader::push_at`]) a partial frame dropped after [`GAP_MS`]
//! without a byte.

use dagon_proto::{Frame, FrameError, SOF};

/// Longest pause between two bytes of one frame on a UART link.
pub const GAP_MS: u64 = 20;

pub struct FrameReader<const N: usize> {
    buf: [u8; N],
    len: usize,
    max_payload: usize,
    /// Time of the last byte pushed with [`FrameReader::push_at`].
    last_ms: u64,
}

impl<const N: usize> FrameReader<N> {
    pub const fn new(max_payload: usize) -> Self {
        FrameReader {
            buf: [0; N],
            len: 0,
            max_payload,
            last_ms: 0,
        }
    }

    /// [`FrameReader::push`] for a link with an inter-byte limit: bytes
    /// arriving at `now_ms` [`GAP_MS`] or more after the previous ones
    /// first drop the partial frame buffered so far. With no bytes it only
    /// applies that rule (a timed-out wait).
    pub fn push_at(&mut self, data: &[u8], now_ms: u64) -> usize {
        if self.len > 0 && now_ms.saturating_sub(self.last_ms) >= GAP_MS {
            self.clear();
        }
        if !data.is_empty() {
            self.last_ms = now_ms;
        }
        self.push(data)
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Buffers bytes; returns how many were taken (at least one if `data` is
    /// not empty). Call [`FrameReader::poll`] after each push.
    pub fn push(&mut self, data: &[u8]) -> usize {
        if self.len == N {
            self.consume(1);
        }
        let n = data.len().min(N - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&data[..n]);
        self.len += n;
        n
    }

    /// Length of a complete, valid frame at the start of the buffer.
    pub fn poll(&mut self) -> Option<usize> {
        loop {
            let start = self.buf[..self.len]
                .iter()
                .position(|&b| b == SOF[0])
                .unwrap_or(self.len);
            self.consume(start);
            if self.len == 0 {
                return None;
            }
            match Frame::decode(&self.buf[..self.len], self.max_payload) {
                Ok((_, n)) => return Some(n),
                Err(FrameError::Incomplete) => return None,
                Err(_) => self.consume(1),
            }
        }
    }

    pub fn frame(&self, n: usize) -> &[u8] {
        &self.buf[..n]
    }

    pub fn consume(&mut self, n: usize) {
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::finish;

    fn frame(seq: u8, payload: &[u8]) -> ([u8; 64], usize) {
        let mut b = [0u8; 64];
        b[8..8 + payload.len()].copy_from_slice(payload);
        let n = finish(&mut b, 0, seq, 1, payload.len()).unwrap();
        (b, n)
    }

    #[test]
    fn finds_frames_through_noise_and_split_reads() {
        let (f1, n1) = frame(1, &[1, 2, 3]);
        let (f2, n2) = frame(2, &[]);
        let mut stream = std::vec::Vec::new();
        stream.extend_from_slice(b"boot log \xDA\x6F junk");
        stream.extend_from_slice(&f1[..n1]);
        stream.extend_from_slice(&f2[..n2]);
        let mut r = FrameReader::<128>::new(256);
        let mut got = std::vec::Vec::new();
        for chunk in stream.chunks(5) {
            let mut rest = chunk;
            while !rest.is_empty() {
                let k = r.push(rest);
                rest = &rest[k..];
                while let Some(n) = r.poll() {
                    got.push(r.frame(n)[4]);
                    r.consume(n);
                }
            }
        }
        assert_eq!(got, [1, 2]);
    }

    #[test]
    fn partial_frame_is_dropped_after_a_gap() {
        let (f, n) = frame(7, &[9, 9]);
        let mut r = FrameReader::<128>::new(256);
        // First half, then silence longer than the gap: the half is gone,
        // and the frame sent again in full is found on its own.
        r.push_at(&f[..5], 0);
        assert_eq!(r.poll(), None);
        r.push_at(&f[..n], GAP_MS);
        assert_eq!(r.poll(), Some(n));
        r.consume(n);
        // Within the gap the bytes are kept together.
        r.push_at(&f[..5], 100);
        r.push_at(&f[5..n], 100 + GAP_MS - 1);
        assert_eq!(r.poll(), Some(n));
    }

    extern crate std;
}
