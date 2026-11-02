//! Request/response over a byte stream (the hub's PIO UART to the watch),
//! independent of the board so the framing runs in host tests:
//! - every wait is bounded by the request's deadline (the port bounds its
//!   own writes);
//! - a partial frame is dropped after a 20 ms pause ([`GAP_MS`]);
//! - garbage (boot logs, broken frames) is skipped by SOF search and CRC;
//! - only a response (RESP flag) with the request's seq is taken.

use dagon_port::StreamPort;
use dagon_proto::payload::{HelloReq, HelloResp};
use dagon_proto::{flags, frame_len, max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};

use crate::frames::{FrameReader, GAP_MS};

pub const UART_FRAME: usize = frame_len(max_payload::UART);
/// Timeout of the hub's own display HELLO on the UART.
pub const HELLO_TIMEOUT_MS: u16 = 200;

/// The hub's own read-only HELLO request (seq from the 0x80..=0xFF space).
pub fn hello_request(max: usize, seq: u8, out: &mut [u8; 32]) -> Option<usize> {
    let body = HelloReq {
        proto_ver: PROTO_VERSION as u32,
        max_payload: max as u16,
    }
    .encode(&mut out[8..])
    .ok()?;
    dagon_proto::finish(out, 0, seq, Cmd::Hello as u8, body).ok()
}

pub fn hello_answer(resp: &[u8], max: usize) -> Option<HelloResp> {
    let (f, _) = Frame::decode(resp, max).ok()?;
    match split_status(f.payload)? {
        (Status::Ok, b) => HelloResp::parse(b).ok(),
        _ => None,
    }
}

/// UART link; `now` gives milliseconds on a monotonic clock.
pub struct UartLink<S, C> {
    port: S,
    now: C,
    reader: FrameReader<{ UART_FRAME * 2 }>,
}

impl<S: StreamPort, C: Fn() -> u64> UartLink<S, C> {
    pub fn new(port: S, now: C) -> Self {
        UartLink {
            port,
            now,
            reader: FrameReader::new(max_payload::UART),
        }
    }

    /// Writes the request frame `inner` and stores the response frame with
    /// the same seq in `out`; returns its length. E_TIMEOUT when no such
    /// response is complete by `timeout_ms`.
    pub async fn exchange(
        &mut self,
        inner: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        let deadline = (self.now)() + timeout_ms as u64;
        self.reader.clear();
        self.port.write(inner).await.map_err(|_| Status::Timeout)?;
        // A request too short to carry a seq gets no answer (times out).
        let seq = inner.get(4).copied();
        loop {
            let now = (self.now)();
            let left = deadline.saturating_sub(now);
            if left == 0 {
                return Err(Status::Timeout);
            }
            let mut b = [0u8; 1];
            // Waits are cut at the gap so a stalled partial frame is
            // dropped even when no further byte arrives.
            match self.port.read(&mut b, left.min(GAP_MS) as u32).await {
                Ok(1) => {
                    self.reader.push_at(&b, (self.now)());
                }
                Ok(_) => {}
                Err(_) => {
                    self.reader.push_at(&[], (self.now)());
                    continue;
                }
            }
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
        let mut resp = [0u8; 64];
        let len = self
            .exchange(&req[..n], HELLO_TIMEOUT_MS, &mut resp)
            .await
            .ok()?;
        hello_answer(&resp[..len], max_payload::UART)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::cell::Cell;
    use dagon_port::BusError;
    use dagon_proto::payload::HelloResp;
    use dagon_sim::block_on;
    use std::rc::Rc;
    use std::vec::Vec;

    /// Scripted node: after the request is written, delivers `(gap_ms,
    /// byte)` pairs, advancing a shared clock; silence afterwards.
    struct Script {
        clock: Rc<Cell<u64>>,
        bytes: Vec<(u64, u8)>,
        next: usize,
        written: Vec<u8>,
    }

    impl StreamPort for Script {
        async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
            match self.bytes.get(self.next) {
                Some(&(gap, b)) if gap <= timeout_ms as u64 => {
                    self.clock.set(self.clock.get() + gap);
                    self.next += 1;
                    buf[0] = b;
                    Ok(1)
                }
                Some(&(gap, b)) => {
                    // The wait ends first; the byte comes later.
                    self.clock.set(self.clock.get() + timeout_ms as u64);
                    self.bytes[self.next] = (gap - timeout_ms as u64, b);
                    Err(BusError::Timeout)
                }
                None => {
                    self.clock.set(self.clock.get() + timeout_ms as u64);
                    Err(BusError::Timeout)
                }
            }
        }
        async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
            self.written.extend_from_slice(data);
            Ok(())
        }
    }

    fn hello_resp(seq: u8) -> Vec<u8> {
        let h = HelloResp {
            proto_ver: 1,
            role: 1,
            port_kind: 2,
            mcu_id: 3,
            mode: 0,
            active_version: dagon_format::Version::new(1, 2, 0),
            trial: false,
            key_id: [0; 8],
            max_payload: 1024,
        };
        let mut b = [0u8; 64];
        b[8] = 0;
        let n = h.encode(&mut b[9..]).unwrap();
        let len = dagon_proto::finish(&mut b, flags::RESP, seq, Cmd::Hello as u8, 1 + n).unwrap();
        b[..len].to_vec()
    }

    /// Bytes with a 1 ms gap each.
    fn steady(bytes: &[u8]) -> Vec<(u64, u8)> {
        bytes.iter().map(|&b| (1, b)).collect()
    }

    fn link(bytes: Vec<(u64, u8)>) -> (UartLink<Script, impl Fn() -> u64>, Rc<Cell<u64>>) {
        let clock = Rc::new(Cell::new(0));
        let c = clock.clone();
        let port = Script {
            clock: clock.clone(),
            bytes,
            next: 0,
            written: Vec::new(),
        };
        (UartLink::new(port, move || c.get()), clock)
    }

    #[test]
    fn answer_found_after_boot_log_and_broken_frame() {
        let mut stream = steady(b"ets Jun 8 2016 rst:0x1 boot\r\n");
        let mut broken = hello_resp(0x81);
        broken[10] ^= 0xFF;
        stream.extend(steady(&broken));
        stream.extend(steady(&hello_resp(0x81)));
        let (mut l, _) = link(stream);
        let h = block_on(l.hello(0x81)).expect("answer");
        assert_eq!(h.active_version, dagon_format::Version::new(1, 2, 0));
    }

    #[test]
    fn stalled_partial_frame_is_dropped() {
        // The start of a frame announcing 1000 payload bytes, a 30 ms
        // stall, then the real answer. Kept, the partial frame would swallow
        // the answer while waiting for its 1000 bytes and time out.
        let head = [
            0xDA,
            0x6F,
            0x01,
            flags::RESP,
            0x82,
            Cmd::Hello as u8,
            0xE8,
            0x03,
        ];
        let mut stream = steady(&head);
        let mut rest = steady(&hello_resp(0x82));
        rest[0].0 = 30;
        stream.extend(rest);
        let (mut l, _) = link(stream.clone());
        assert!(block_on(l.hello(0x82)).is_some());
        // Same bytes without the stall: no answer (shows the gap matters).
        for b in stream.iter_mut() {
            b.0 = 1;
        }
        let (mut l, _) = link(stream);
        assert!(block_on(l.hello(0x82)).is_none());
    }

    #[test]
    fn other_seq_and_requests_are_skipped() {
        let mut stream = steady(&hello_resp(0x90));
        stream.extend(steady(&hello_resp(0x83)));
        let (mut l, _) = link(stream);
        assert!(block_on(l.hello(0x83)).is_some());
    }

    #[test]
    fn silence_times_out_on_the_deadline() {
        let (mut l, clock) = link(Vec::new());
        assert!(block_on(l.hello(0x84)).is_none());
        let t = clock.get();
        assert!(
            (HELLO_TIMEOUT_MS as u64..HELLO_TIMEOUT_MS as u64 + GAP_MS).contains(&t),
            "{t}"
        );
    }
}
