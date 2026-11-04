//! Other record layouts the harness runs against [`crate::record::V1`]:
//! [`V2`] stands for any later layout (version skew between stages and
//! readers); [`Weak`] is a negative control, the payload-first/CRC-last
//! shape without a commit byte, which the model must reject.

use crate::record::{BootId, Entry, RecordCodec, COMMIT_BASE};
use dagon_format::crc32;

fn crc(layout: u8, writer: u8, body: &[u8]) -> u32 {
    let mut b = vec![layout, writer];
    b.extend_from_slice(body);
    crc32(&b)
}

fn le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b.try_into().unwrap())
}

/// A different field order and size, same append protocol.
pub struct V2;

impl RecordCodec for V2 {
    fn layout(&self) -> u8 {
        2
    }

    fn entry_size(&self) -> usize {
        32
    }

    fn append_stores(&self, writer: u8, e: &Entry) -> Vec<(usize, u8)> {
        let mut b = [0u8; 32];
        b[1] = e.fmt;
        b[2] = e.kind;
        b[3] = (e.start as u8) | (e.epoch as u8) << 1 | e.boot.opener << 4;
        b[4..8].copy_from_slice(&e.boot.boot.to_le_bytes());
        b[8..12].copy_from_slice(&e.eseq.to_le_bytes());
        b[12..16].copy_from_slice(&e.tag.to_le_bytes());
        b[16..20].copy_from_slice(&e.boot.tag.to_le_bytes());
        b[20..23].copy_from_slice(&e.payload);
        let c = crc(2, writer, &b[1..28]);
        b[28..32].copy_from_slice(&c.to_le_bytes());
        let mut s = vec![(0, 0)];
        s.extend((1..32).map(|i| (i, b[i])));
        s.push((0, COMMIT_BASE | 2));
        s
    }

    fn decode(&self, writer: u8, b: &[u8]) -> Option<Entry> {
        let ok = b[0] == COMMIT_BASE | 2
            && b[23..28].iter().all(|&x| x == 0)
            && le(&b[28..32]) == crc(2, writer, &b[1..28]);
        ok.then(|| Entry {
            start: b[3] & 1 != 0,
            epoch: b[3] & 2 != 0,
            boot: BootId {
                opener: b[3] >> 4,
                boot: le(&b[4..8]),
                tag: le(&b[16..20]),
            },
            fmt: b[1],
            kind: b[2],
            eseq: le(&b[8..12]),
            tag: le(&b[12..16]),
            payload: [b[20], b[21], b[22]],
        })
    }
}

/// Negative control: no commit byte, 8-bit check written last. A torn
/// overwrite leaves the old check behind the new bytes.
pub struct Weak;

impl RecordCodec for Weak {
    fn layout(&self) -> u8 {
        1
    }

    fn entry_size(&self) -> usize {
        24
    }

    fn append_stores(&self, writer: u8, e: &Entry) -> Vec<(usize, u8)> {
        let mut b = [0u8; 24];
        b[0] = COMMIT_BASE | 1;
        b[1] = (e.start as u8) | (e.epoch as u8) << 1 | e.boot.opener << 4;
        b[2] = e.fmt;
        b[3] = e.kind;
        b[4..8].copy_from_slice(&e.eseq.to_le_bytes());
        b[8..12].copy_from_slice(&e.boot.boot.to_le_bytes());
        b[12..16].copy_from_slice(&e.boot.tag.to_le_bytes());
        b[16..20].copy_from_slice(&e.tag.to_le_bytes());
        b[20..23].copy_from_slice(&e.payload);
        b[23] = crc(1, writer, &b[..23]) as u8;
        b.iter().copied().enumerate().collect()
    }

    fn decode(&self, writer: u8, b: &[u8]) -> Option<Entry> {
        let ok = b[0] == COMMIT_BASE | 1 && b[23] == crc(1, writer, &b[..23]) as u8;
        ok.then(|| Entry {
            start: b[1] & 1 != 0,
            epoch: b[1] & 2 != 0,
            boot: BootId {
                opener: b[1] >> 4,
                boot: le(&b[8..12]),
                tag: le(&b[12..16]),
            },
            fmt: b[2],
            kind: b[3],
            eseq: le(&b[4..8]),
            tag: le(&b[16..20]),
            payload: [b[20], b[21], b[22]],
        })
    }
}
