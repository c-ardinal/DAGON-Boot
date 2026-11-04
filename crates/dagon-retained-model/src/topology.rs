//! Compiled topology: the body the host sends (node layout with the 64-bit
//! MCU identity) and the hub's two-bank copy store in NOR flash.
//!
//! The body codec is [`BodyCodec`]; [`BodyV1`] is the layout this model
//! proves. The store writes the bank the reader is not using: erase, body,
//! header, and the header's commit byte last. A bank counts only with its
//! commit byte, a matching header CRC and a body matching the header's
//! SHA-256; the newer of two counted banks (sequence, serial order) is the
//! copy.

use crate::identity;
use crate::mem::{Cut, Mem};
use dagon_format::manifest::{Constraint, CONSTRAINT_SIZE, MAX_CONSTRAINTS};
use dagon_format::topology::{TopoLink, LINK_SIZE, MAX_LINKS, MAX_NODES};
use dagon_format::{crc32, sha256};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub index: u8,
    pub role: u8,
    pub port_kind: u8,
    pub max_attempts: u8,
    pub listen_window_ms: u16,
    pub map_id: u8,
    pub order_rank: u8,
    pub recovery_tiers: [u8; 3],
    /// [`identity::identity`] of the node's MCU definition.
    pub mcu: u64,
    pub target_tag: [u8; 16],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub nodes: Vec<Node>,
    pub links: Vec<TopoLink>,
    pub constraints: Vec<Constraint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Head, counts or reserved bytes wrong, or not canonical.
    Malformed,
    /// A layout number this reader does not read.
    Unsupported(u16),
    /// Larger than the hub's bank.
    TooLarge,
}

pub trait BodyCodec {
    fn layout(&self) -> u16;
    fn encode(&self, t: &Topology) -> Result<Vec<u8>, Refused>;
    fn decode(&self, b: &[u8]) -> Result<Topology, Refused>;
}

pub const MAGIC: [u8; 4] = *b"DGTP";
pub const BODY_HEAD: usize = 16;
pub const NODE_SIZE: usize = 40;
pub const BODY_MAX: usize =
    BODY_HEAD + NODE_SIZE * MAX_NODES + LINK_SIZE * MAX_LINKS + CONSTRAINT_SIZE * MAX_CONSTRAINTS;

/// Layout 1.
///
/// Head (16 bytes): magic `DGTP`, layout u16 LE, node count, link count,
/// constraint count, 7 zero bytes. Then the nodes (40 bytes each: index,
/// role, port kind, max attempts, listen window u16, map id, order rank,
/// recovery tiers \[3\], 5 zero bytes, MCU identity u64 at 16, target tag
/// \[16\] at 24), the links (16 bytes, as the image tools encode them),
/// the constraints (16 bytes). Canonical: node `index` equals its
/// position, links sorted by `to`, reserved bytes zero.
pub struct BodyV1;

fn node_bytes(n: &Node) -> [u8; NODE_SIZE] {
    let mut b = [0u8; NODE_SIZE];
    b[..4].copy_from_slice(&[n.index, n.role, n.port_kind, n.max_attempts]);
    b[4..6].copy_from_slice(&n.listen_window_ms.to_le_bytes());
    b[6] = n.map_id;
    b[7] = n.order_rank;
    b[8..11].copy_from_slice(&n.recovery_tiers);
    b[16..24].copy_from_slice(&identity::encode(n.mcu));
    b[24..40].copy_from_slice(&n.target_tag);
    b
}

fn node_parse(b: &[u8]) -> Node {
    Node {
        index: b[0],
        role: b[1],
        port_kind: b[2],
        max_attempts: b[3],
        listen_window_ms: u16::from_le_bytes([b[4], b[5]]),
        map_id: b[6],
        order_rank: b[7],
        recovery_tiers: [b[8], b[9], b[10]],
        mcu: identity::decode(b[16..24].try_into().unwrap()),
        target_tag: b[24..40].try_into().unwrap(),
    }
}

impl BodyCodec for BodyV1 {
    fn layout(&self) -> u16 {
        1
    }

    fn encode(&self, t: &Topology) -> Result<Vec<u8>, Refused> {
        let (n, l, c) = (t.nodes.len(), t.links.len(), t.constraints.len());
        let counts = (1..=MAX_NODES).contains(&n) && l <= MAX_LINKS && c <= MAX_CONSTRAINTS;
        let canonical = t
            .nodes
            .iter()
            .enumerate()
            .all(|(i, x)| x.index as usize == i)
            && t.links.windows(2).all(|w| w[0].to < w[1].to);
        if !counts || !canonical {
            return Err(Refused::Malformed);
        }
        let mut b = vec![0u8; BODY_HEAD];
        b[..4].copy_from_slice(&MAGIC);
        b[4..6].copy_from_slice(&1u16.to_le_bytes());
        b[6..9].copy_from_slice(&[n as u8, l as u8, c as u8]);
        t.nodes
            .iter()
            .for_each(|x| b.extend_from_slice(&node_bytes(x)));
        t.links
            .iter()
            .for_each(|x| b.extend_from_slice(&x.to_bytes()));
        t.constraints
            .iter()
            .for_each(|x| b.extend_from_slice(&x.to_bytes()));
        Ok(b)
    }

    fn decode(&self, b: &[u8]) -> Result<Topology, Refused> {
        if b.len() < BODY_HEAD || b[..4] != MAGIC {
            return Err(Refused::Malformed);
        }
        let layout = u16::from_le_bytes([b[4], b[5]]);
        if layout != 1 {
            return Err(Refused::Unsupported(layout));
        }
        let (n, l, c) = (b[6] as usize, b[7] as usize, b[8] as usize);
        let len = BODY_HEAD + NODE_SIZE * n + LINK_SIZE * l + CONSTRAINT_SIZE * c;
        if b.len() != len {
            return Err(Refused::Malformed);
        }
        let mut at = BODY_HEAD;
        let mut take = |size: usize| {
            let s = &b[at..at + size];
            at += size;
            s
        };
        let t = Topology {
            nodes: (0..n).map(|_| node_parse(take(NODE_SIZE))).collect(),
            links: (0..l).map(|_| TopoLink::parse(take(LINK_SIZE))).collect(),
            constraints: (0..c)
                .map(|_| Constraint::parse(take(CONSTRAINT_SIZE)))
                .collect(),
        };
        match self.encode(&t) {
            Ok(again) if again == b => Ok(t),
            _ => Err(Refused::Malformed),
        }
    }
}

/// Bank header, 64 bytes at the bank start:
///
/// | off | field |
/// |---|---|
/// | 0 | commit: `0xC1` (layout 1); programmed last |
/// | 1 | zero, 3 bytes |
/// | 4 | sequence u32 LE: the other bank's + 1, or 1 |
/// | 8 | body length u32 LE |
/// | 12 | SHA-256 of the body |
/// | 44 | CRC-32 over `[1]` then bytes 1..44 |
/// | 48 | zero, 16 bytes |
pub const BANK_HEAD: usize = 64;
pub const COMMIT: u8 = 0xC1;

/// Two banks of `bank` bytes from `base`, each erasable on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Store {
    pub base: usize,
    pub bank: usize,
}

impl Store {
    pub fn capacity(&self) -> usize {
        self.bank - BANK_HEAD
    }

    fn at(&self, i: usize) -> usize {
        self.base + i * self.bank
    }

    /// Step index (0-based, counting erased and programmed bytes) of the
    /// commit byte of a write of `len` body bytes: interrupted at a step
    /// below it the previous copy stands, at or after it the new one.
    pub fn commit_step(&self, len: usize) -> u64 {
        (self.bank + len + BANK_HEAD - 1) as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copy {
    pub bank: usize,
    pub seq: u32,
    pub body: Vec<u8>,
    pub digest: [u8; 32],
}

fn header(seq: u32, body: &[u8]) -> [u8; BANK_HEAD] {
    let mut h = [0u8; BANK_HEAD];
    h[0] = COMMIT;
    h[4..8].copy_from_slice(&seq.to_le_bytes());
    h[8..12].copy_from_slice(&(body.len() as u32).to_le_bytes());
    h[12..44].copy_from_slice(&sha256(body));
    let mut v = vec![1u8];
    v.extend_from_slice(&h[1..44]);
    h[44..48].copy_from_slice(&crc32(&v).to_le_bytes());
    h
}

fn bank(mem: &[u8], s: &Store, i: usize) -> Option<Copy> {
    let h = &mem[s.at(i)..s.at(i) + BANK_HEAD];
    let len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
    if h[0] != COMMIT || len > s.capacity() {
        return None;
    }
    let mut v = vec![1u8];
    v.extend_from_slice(&h[1..44]);
    let crc_ok = crc32(&v) == u32::from_le_bytes(h[44..48].try_into().unwrap());
    let body = &mem[s.at(i) + BANK_HEAD..s.at(i) + BANK_HEAD + len];
    let digest: [u8; 32] = h[12..44].try_into().unwrap();
    (crc_ok && sha256(body) == digest).then(|| Copy {
        bank: i,
        seq: u32::from_le_bytes(h[4..8].try_into().unwrap()),
        body: body.to_vec(),
        digest,
    })
}

/// The copy the hub uses, if any bank counts.
pub fn current(mem: &[u8], s: &Store) -> Option<Copy> {
    match (bank(mem, s, 0), bank(mem, s, 1)) {
        (Some(a), Some(b)) => Some(if (b.seq.wrapping_sub(a.seq) as i32) > 0 {
            b
        } else {
            a
        }),
        (a, b) => a.or(b),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The copy's topology; `relay.children_set` repeats `digest`.
    Copy { digest: [u8; 32] },
    /// No usable copy: the project's fallback list.
    Fallback(Option<Refused>),
}

/// What the hub's relay starts with.
pub fn load<B: BodyCodec>(b: &B, mem: &[u8], s: &Store) -> (Option<Topology>, Source) {
    match current(mem, s) {
        None => (None, Source::Fallback(None)),
        Some(c) => match b.decode(&c.body) {
            Ok(t) => (Some(t), Source::Copy { digest: c.digest }),
            Err(e) => (None, Source::Fallback(Some(e))),
        },
    }
}

/// The host's check before sending any byte.
pub fn host_plan(s: &Store, body: &[u8]) -> Result<[u8; 32], Refused> {
    if body.len() > s.capacity() {
        return Err(Refused::TooLarge);
    }
    Ok(sha256(body))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// `Absent → Written`.
    First,
    /// `Written → Written`.
    Replaced,
}

/// The hub's write (image lifecycle `written`, target TOPOLOGY).
pub fn write(mem: &mut Mem, s: &Store, body: &[u8]) -> Result<Result<Written, Refused>, Cut> {
    if body.len() > s.capacity() {
        return Ok(Err(Refused::TooLarge));
    }
    let now = current(mem.bytes(), s);
    let target = now.as_ref().map_or(0, |c| 1 - c.bank);
    let seq = now.as_ref().map_or(1, |c| c.seq.wrapping_add(1));
    let at = s.at(target);
    mem.erase(at..at + s.bank)?;
    for (i, &v) in body.iter().enumerate() {
        mem.program(at + BANK_HEAD + i, v)?;
    }
    let h = header(seq, body);
    for (i, &v) in h.iter().enumerate().skip(1) {
        mem.program(at + i, v)?;
    }
    mem.program(at, h[0])?;
    Ok(Ok(if now.is_some() {
        Written::Replaced
    } else {
        Written::First
    }))
}
