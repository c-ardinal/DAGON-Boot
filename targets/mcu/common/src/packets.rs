//! DAGON frames over a packet pipe (a USB CDC data interface), independent
//! of the device so the cutting runs in host tests:
//! - a frame may span packets and a packet may hold several frames; what a
//!   packet holds beyond the frame handed out stays for the next read;
//! - a failed read (the host went away) drops what was buffered;
//! - a frame is written in packets of the pipe's size, and one that fills
//!   its last packet is closed by an empty packet, so the host's read
//!   ends.

use crate::frames::FrameReader;

/// The pipe: one packet in, one packet out.
#[allow(async_fn_in_trait)]
pub trait PacketPort {
    type Error;
    /// Waits for the next packet from the host; returns its length.
    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    /// Writes one packet (an empty one too).
    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

/// The reading side: frames cut out of packets of up to `P` bytes, with
/// `N` bytes of room (a whole frame and a packet).
pub struct PacketFrames<const N: usize, const P: usize> {
    rx: FrameReader<N>,
    pkt: [u8; P],
    /// Unread part of `pkt`: `at..len`.
    at: usize,
    len: usize,
    /// Length of the frame handed out last (dropped on the next read).
    last: usize,
}

impl<const N: usize, const P: usize> PacketFrames<N, P> {
    pub const fn new(max_payload: usize) -> Self {
        PacketFrames {
            rx: FrameReader::new(max_payload),
            pkt: [0; P],
            at: 0,
            len: 0,
            last: 0,
        }
    }

    /// Waits for the next complete frame from `port` (never times out).
    pub async fn read_frame<Q: PacketPort>(&mut self, port: &mut Q) -> &[u8] {
        if self.last > 0 {
            self.rx.consume(self.last);
            self.last = 0;
        }
        loop {
            if let Some(n) = self.rx.poll() {
                self.last = n;
                return self.rx.frame(n);
            }
            if self.at < self.len {
                self.at += self.rx.push(&self.pkt[self.at..self.len]);
                continue;
            }
            match port.read_packet(&mut self.pkt).await {
                Ok(n) => (self.at, self.len) = (0, n),
                Err(_) => {
                    self.rx.clear();
                    (self.at, self.len) = (0, 0);
                }
            }
        }
    }
}

/// Writes the frame `data` to `port` in packets of `packet` bytes; a frame
/// that fills its last packet is followed by an empty one.
pub async fn write_frame<Q: PacketPort>(
    port: &mut Q,
    data: &[u8],
    packet: usize,
) -> Result<(), Q::Error> {
    if data.is_empty() {
        return Ok(());
    }
    for chunk in data.chunks(packet) {
        port.write_packet(chunk).await?;
    }
    if data.len().is_multiple_of(packet) {
        port.write_packet(&[]).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::collections::VecDeque;
    use std::vec;
    use std::vec::Vec;

    use dagon_proto::{frame_len, max_payload, Frame};
    use dagon_sim::block_on;

    use super::*;

    const PACKET: usize = 64;
    const ROOM: usize = frame_len(max_payload::USB) + PACKET;

    /// A host: the packets it sends (`Err`: it went away), and what it got.
    #[derive(Default)]
    struct Host {
        sends: VecDeque<Result<Vec<u8>, ()>>,
        got: Vec<Vec<u8>>,
        /// Writes that succeed before the host stops reading.
        reads: Option<usize>,
    }

    impl PacketPort for Host {
        type Error = ();
        async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let p = self.sends.pop_front().expect("a read with nothing sent")?;
            buf[..p.len()].copy_from_slice(&p);
            Ok(p.len())
        }
        async fn write_packet(&mut self, data: &[u8]) -> Result<(), ()> {
            if self.reads == Some(self.got.len()) {
                return Err(());
            }
            self.got.push(data.to_vec());
            Ok(())
        }
    }

    fn frame(seq: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0; frame_len(payload.len())];
        Frame {
            flags: 0,
            seq,
            cmd: 0x10,
            payload,
        }
        .encode(&mut out)
        .unwrap();
        out
    }

    fn host(stream: &[u8]) -> Host {
        Host {
            sends: stream.chunks(PACKET).map(|c| Ok(c.to_vec())).collect(),
            ..Host::default()
        }
    }

    fn reader() -> PacketFrames<ROOM, PACKET> {
        PacketFrames::new(max_payload::USB)
    }

    #[test]
    fn frames_are_cut_wherever_the_packets_end() {
        // One frame over many packets, two in one packet, one across a
        // packet's end, the largest frame, and noise between them.
        let frames = [
            frame(1, &[0xA5; 300]),
            frame(2, &[]),
            frame(3, &[1]),
            frame(4, &[7; 70]),
            frame(5, &[0x5A; max_payload::USB]),
        ];
        let mut stream = vec![0x00, 0xDA, 0x13];
        for f in &frames {
            stream.extend(f);
            stream.extend([0xFF, 0xDA]);
        }
        let mut host = host(&stream);
        let mut rx = reader();
        for f in &frames {
            assert_eq!(block_on(rx.read_frame(&mut host)), f.as_slice());
        }
    }

    #[test]
    fn a_failed_read_drops_the_part_of_a_frame_before_it() {
        let whole = frame(9, &[3; 100]);
        let mut host = Host::default();
        host.sends.push_back(Ok(whole[..PACKET].to_vec()));
        host.sends.push_back(Err(()));
        // The rest of the cut frame, then a whole one.
        host.sends.push_back(Ok(whole[PACKET..].to_vec()));
        for c in frame(10, &[4; 5]).chunks(PACKET) {
            host.sends.push_back(Ok(c.to_vec()));
        }
        let mut rx = reader();
        assert_eq!(block_on(rx.read_frame(&mut host)), frame(10, &[4; 5]));
    }

    #[test]
    fn a_frame_that_fills_its_last_packet_is_closed_by_an_empty_one() {
        for (len, packets, closed) in [
            (1, 1, false),
            (63, 1, false),
            (64, 1, true),
            (65, 2, false),
            (128, 2, true),
        ] {
            let mut host = Host::default();
            let data = vec![0x11; len];
            block_on(write_frame(&mut host, &data, PACKET)).unwrap();
            let empty = host.got.last().is_some_and(|p| p.is_empty());
            assert_eq!(empty, closed, "{len} bytes");
            assert_eq!(host.got.len(), packets + closed as usize, "{len} bytes");
            assert_eq!(host.got.concat(), data, "{len} bytes");
        }
        // Nothing to write: no packet at all.
        let mut host = Host::default();
        block_on(write_frame(&mut host, &[], PACKET)).unwrap();
        assert!(host.got.is_empty());
    }

    #[test]
    fn a_write_stops_at_the_packet_the_host_does_not_take() {
        let mut host = Host {
            reads: Some(1),
            ..Host::default()
        };
        assert_eq!(block_on(write_frame(&mut host, &[0; 200], PACKET)), Err(()));
        assert_eq!(host.got.len(), 1);
    }
}
