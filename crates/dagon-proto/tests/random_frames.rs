//! Frame handling over generated input: every reader of raw bytes (the
//! decoder, the stream cutters' helpers, the version refusal) takes any
//! byte string without a panic and keeps its promises, a frame survives
//! its own encoding, and no single flipped bit passes for the frame it
//! was. The generator is seed-fixed, so a failure names its case.

use dagon_proto::{
    find_sof, frame_len, is_refused_start, max_payload, refusing_version, split_status,
    version_refusal, whole_or_refused, Frame, FrameError, HEADER_LEN, MAX_FRAME, SOF,
};

/// xorshift64*: enough for case generation, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

const LIMITS: [usize; 4] = [
    max_payload::SMALLEST,
    max_payload::I2C,
    max_payload::UART,
    max_payload::USB,
];

fn valid(rng: &mut Rng, limit: usize) -> Vec<u8> {
    let payload = {
        let n = rng.below(limit + 1);
        rng.bytes(n)
    };
    let frame = Frame {
        flags: rng.next() as u8,
        seq: rng.next() as u8,
        cmd: rng.next() as u8,
        payload: &payload,
    };
    let mut out = vec![0; frame_len(payload.len())];
    let n = frame.encode(&mut out).unwrap();
    assert_eq!(n, out.len());
    out
}

/// Byte strings that reach past the first checks: noise, noise behind a
/// frame start, and a valid frame with a few bytes replaced or cut.
fn hostile(rng: &mut Rng, case: u64) -> Vec<u8> {
    match case % 4 {
        0 => {
            let n = rng.below(48);
            rng.bytes(n)
        }
        1 => {
            let n = rng.below(48);
            let mut b = SOF.to_vec();
            b.extend(rng.bytes(n));
            b
        }
        2 => {
            let mut b = valid(rng, max_payload::SMALLEST);
            for _ in 0..1 + rng.below(3) {
                let at = rng.below(b.len());
                b[at] = rng.next() as u8;
            }
            b
        }
        _ => {
            let mut b = valid(rng, max_payload::SMALLEST);
            b.truncate(rng.below(b.len() + 1));
            let n = rng.below(4);
            b.extend(rng.bytes(n));
            b
        }
    }
}

#[test]
fn no_byte_string_panics_a_reader_and_each_keeps_its_bounds() {
    let mut rng = Rng(0xDA60_0001);
    for case in 0..200_000u64 {
        let buf = hostile(&mut rng, case);
        let limit = LIMITS[rng.below(LIMITS.len())];
        match Frame::decode(&buf, limit) {
            Ok((f, n)) => {
                assert!(n <= buf.len(), "case {case}");
                assert_eq!(n, frame_len(f.payload.len()), "case {case}");
                assert!(f.payload.len() <= limit, "case {case}");
            }
            Err(FrameError::BufferTooSmall | FrameError::Field) => {
                panic!("case {case}: a decode answered an encoder's error")
            }
            Err(_) => {}
        }
        if let Some(n) = whole_or_refused(&buf, limit) {
            assert!(n <= buf.len(), "case {case}");
        }
        let from = rng.below(buf.len() + 2);
        if let Some(at) = find_sof(&buf, from) {
            assert!(at >= from && buf[at] == SOF[0], "case {case}");
        }
        let _ = is_refused_start(&buf);
        let _ = split_status(&buf);
        let _ = refusing_version(&buf, rng.next() as u8, rng.next() as u8);
        let mut out = [0u8; MAX_FRAME];
        let room = rng.below(HEADER_LEN + 16);
        if let Some(n) = version_refusal(&buf, &mut out[..room]) {
            // The refusal is a frame of this version that its reader takes.
            let (f, m) = Frame::decode(&out[..n], max_payload::SMALLEST).expect("refusal");
            assert_eq!((m, f.seq, f.cmd), (n, buf[4], buf[5]), "case {case}");
            assert_eq!(
                version_refusal(&out[..n], &mut [0; 64]),
                None,
                "case {case}"
            );
        }
    }
}

#[test]
fn a_frame_survives_its_encoding_and_the_bytes_behind_it() {
    let mut rng = Rng(0xDA60_0002);
    for case in 0..20_000u64 {
        let limit = LIMITS[rng.below(LIMITS.len())];
        let mut wire = valid(&mut rng, limit);
        let len = wire.len();
        let (flags, seq, cmd) = (wire[3], wire[4], wire[5]);
        let n = rng.below(8);
        wire.extend(rng.bytes(n));
        let (f, n) = Frame::decode(&wire, limit).unwrap_or_else(|e| panic!("case {case}: {e:?}"));
        assert_eq!(n, len, "case {case}");
        assert_eq!((f.flags, f.seq, f.cmd), (flags, seq, cmd), "case {case}");
        assert_eq!(f.payload, &wire[HEADER_LEN..len - 4], "case {case}");
        // One byte short is not a frame yet, on any link.
        assert_eq!(
            Frame::decode(&wire[..len - 1], limit),
            Err(FrameError::Incomplete),
            "case {case}"
        );
        // A link with less room drops it.
        if !f.payload.is_empty() {
            assert_eq!(
                Frame::decode(&wire, f.payload.len() - 1),
                Err(FrameError::TooLong),
                "case {case}"
            );
        }
    }
}

/// Every bit of a frame, flipped alone: the result is never taken as a
/// frame (the CRC covers everything behind the start bytes, and the
/// start bytes are compared).
#[test]
fn no_single_flipped_bit_passes() {
    let mut rng = Rng(0xDA60_0003);
    for case in 0..300u64 {
        let limit = max_payload::SMALLEST;
        let wire = valid(&mut rng, limit);
        for bit in 0..wire.len() * 8 {
            let mut hit = wire.clone();
            hit[bit / 8] ^= 1 << (bit % 8);
            // A longer length reads past the end (not a frame yet); with
            // the bytes that follow on a stream it fails its CRC.
            let got = Frame::decode(&hit, max_payload::USB);
            assert!(got.is_err(), "case {case} bit {bit}: {got:?}");
            hit.extend(rng.bytes(max_payload::USB));
            let got = Frame::decode(&hit, max_payload::USB);
            assert!(got.is_err(), "case {case} bit {bit} on a stream: {got:?}");
        }
    }
}
