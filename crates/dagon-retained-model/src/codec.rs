//! The shape a retained format's codec has to the model, the production
//! codecs in that shape, and the world that runs any of them.
//!
//! [`CodecWorld`] encodes a generated value, damages the bytes one fault at
//! a time (a foreign byte, a cut, bytes appended) and checks after every
//! fault:
//!
//! - K1: the undamaged bytes decode to the value;
//! - K2: whatever decodes, decodes the same once encoded again;
//! - K3 (canonical codecs): whatever decodes is, byte for byte, the
//!   encoding of what it decoded to, so damaged bytes are either refused
//!   or another whole value.
//!
//! The codecs here are the shipped ones: [`TopologyCodec`] is
//! `dagon_format::topology` (`encode` and `TopologyView::parse`),
//! [`TrialCellCodec`] is `dagon_core::bootcore` (`trial_cell` and `tried`).

use crate::harness::{Violation, World};
use crate::identity::{Identity, Production};
use crate::rng::Rng;
use dagon_core::bootcore::{trial_cell, tried, MAX_COUNT};
use dagon_core::log::SlotRef;
use dagon_format::header::McuId;
use dagon_format::manifest::{Constraint, MAX_CONSTRAINTS};
use dagon_format::topology::{self, TopoEnd, TopoLink, TopoNode, TopologyView, MAX_LINKS, NONE};
use dagon_format::{Version, MAX_NODES};
use std::fmt::Debug;
use std::marker::PhantomData;

pub trait Codec {
    type Value: Clone + Debug + PartialEq;
    /// Every byte string that decodes is the encoding of its value (K3).
    const CANONICAL: bool;

    fn generate(rng: &mut Rng) -> Self::Value;
    fn encode(v: &Self::Value) -> Vec<u8>;
    /// `None` when the codec refuses the bytes.
    fn decode(bytes: &[u8]) -> Option<Self::Value>;
}

/// One fault on the encoded bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Damage {
    /// The byte at `at` (modulo the length) reads as `to`.
    Set { at: u16, to: u8 },
    /// Only the first `len` bytes (modulo the length) are left.
    Cut { len: u16 },
    /// `byte` follows the last byte.
    Append { byte: u8 },
}

pub struct CodecWorld<C>(PhantomData<C>);

impl<C: Codec> CodecWorld<C> {
    fn decoded(bytes: &[u8], step: usize) -> Result<(), Violation> {
        let Some(v) = C::decode(bytes) else {
            return Ok(());
        };
        let again = C::encode(&v);
        if C::decode(&again).as_ref() != Some(&v) {
            return Err(Violation::new("K2", step, format!("{v:?}")));
        }
        if C::CANONICAL && again != bytes {
            return Err(Violation::new("K3", step, format!("{v:?}")));
        }
        Ok(())
    }
}

impl<C: Codec> World for CodecWorld<C> {
    type Config = C::Value;
    type Op = Damage;

    fn generate(rng: &mut Rng) -> (C::Value, Vec<Damage>) {
        let v = C::generate(rng);
        let ops = (0..rng.below(6))
            .map(|_| match rng.below(8) {
                0 => Damage::Cut {
                    len: rng.next_u64() as u16,
                },
                1 => Damage::Append { byte: rng.byte() },
                // A single bit, the fault a canonical codec is likeliest
                // to let through.
                2..=4 => Damage::Set {
                    at: rng.next_u64() as u16,
                    to: 1 << rng.below(8),
                },
                _ => Damage::Set {
                    at: rng.next_u64() as u16,
                    to: rng.byte(),
                },
            })
            .collect();
        (v, ops)
    }

    fn run(v: &C::Value, ops: &[Damage], _seed: u64) -> Result<(), Violation> {
        let mut bytes = C::encode(v);
        if C::decode(&bytes).as_ref() != Some(v) {
            return Err(Violation::new("K1", 0, format!("{v:?}")));
        }
        Self::decoded(&bytes, 0)?;
        for (step, op) in ops.iter().enumerate() {
            match *op {
                Damage::Set { at, to } => {
                    let n = bytes.len();
                    if let Some(b) = bytes.get_mut(usize::from(at) % n.max(1)) {
                        *b = to;
                    }
                }
                Damage::Cut { len } => bytes.truncate(usize::from(len) % bytes.len().max(1)),
                Damage::Append { byte } => bytes.push(byte),
            }
            Self::decoded(&bytes, step)?;
        }
        Ok(())
    }

    fn simpler(op: &Damage) -> Vec<Damage> {
        match *op {
            Damage::Set { at, to } if to != 0 => vec![Damage::Set { at, to: 0 }],
            Damage::Append { byte } if byte != 0 => vec![Damage::Append { byte: 0 }],
            _ => vec![],
        }
    }
}

/// The compiled topology (`topology-bin` 3): nodes, links and constraints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topology {
    pub nodes: Vec<TopoNode>,
    pub links: Vec<TopoLink>,
    pub constraints: Vec<Constraint>,
}

pub struct TopologyCodec;

impl Codec for TopologyCodec {
    type Value = Topology;
    const CANONICAL: bool = true;

    fn generate(rng: &mut Rng) -> Topology {
        let n = 1 + rng.below(MAX_NODES as u64) as u8;
        let nodes = (0..n)
            .map(|index| TopoNode {
                index,
                role: rng.byte(),
                mcu_id: McuId(Production::derive(&format!("mcu-{}", rng.below(8))).to_le_bytes()),
                port_kind: rng.byte(),
                max_attempts: rng.byte(),
                listen_window_ms: rng.next_u64() as u16,
                map_id: rng.byte(),
                recovery_tiers: [rng.byte(), rng.byte(), rng.byte()],
                order_rank: rng.byte(),
                target_tag: core::array::from_fn(|_| rng.byte()),
            })
            .collect();
        // Links in the order of their ends' nodes, no pair twice.
        let mut pairs: Vec<(u8, u8)> = (0..rng.below(MAX_LINKS as u64 + 1))
            .filter(|_| n > 1)
            .map(|_| {
                let a = rng.below(u64::from(n) - 1) as u8;
                (a, a + 1 + rng.below(u64::from(n - a) - 1) as u8)
            })
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        let links = pairs
            .into_iter()
            .map(|(a, b)| TopoLink {
                ends: [a, b].map(|node| TopoEnd {
                    node,
                    port: rng.byte(),
                    addr: rng.byte(),
                }),
                iface: rng.byte(),
                control: rng.byte(),
                tx_pin: rng.byte(),
                rx_pin: rng.byte(),
                nrst_pin: rng.byte(),
                boot_pin: rng.byte(),
                baud: rng.next_u64() as u32,
                rom_entry: rng.byte(),
                bus_id: rng.byte(),
                gpio_end: [0, 1, NONE][rng.below(3) as usize],
            })
            .collect();
        let constraints = (0..rng.below(MAX_CONSTRAINTS as u64 + 1))
            .map(|_| Constraint {
                kind: rng.byte(),
                node_a: rng.byte(),
                node_b: rng.byte(),
                node_mask: rng.next_u64() as u32,
                min_version: Version::from_raw(rng.next_u64() as u32),
            })
            .collect();
        Topology {
            nodes,
            links,
            constraints,
        }
    }

    fn encode(v: &Topology) -> Vec<u8> {
        let mut out = vec![0; topology::MAX_SIZE];
        let n = topology::encode(&v.nodes, &v.links, &v.constraints, &mut out)
            .expect("a generated or decoded topology encodes");
        out.truncate(n);
        out
    }

    fn decode(bytes: &[u8]) -> Option<Topology> {
        let v = TopologyView::parse(bytes).ok()?;
        Some(Topology {
            nodes: (0..v.node_count()).map(|i| v.node(i)).collect(),
            links: (0..v.link_count()).map(|i| v.link(i)).collect(),
            constraints: (0..v.constraint_count()).map(|i| v.constraint(i)).collect(),
        })
    }
}

/// What the boot core trial cell holds: the starts counted for the image
/// whose header digest begins with `digest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trial {
    pub digest: [u8; 2],
    pub count: u8,
}

/// Not canonical: a cell cut between its two stores holds one old word and
/// still counts, and any other bytes count nothing.
pub struct TrialCellCodec;

fn image(digest: [u8; 2]) -> SlotRef {
    let mut digest16 = [0; 16];
    digest16[..2].copy_from_slice(&digest);
    SlotRef {
        slot: 0,
        version: Version::new(0, 0, 0),
        digest16,
    }
}

impl Codec for TrialCellCodec {
    type Value = Trial;
    const CANONICAL: bool = false;

    fn generate(rng: &mut Rng) -> Trial {
        Trial {
            digest: [rng.byte(), rng.byte()],
            count: rng.below(u64::from(MAX_COUNT) + 1) as u8,
        }
    }

    fn encode(v: &Trial) -> Vec<u8> {
        trial_cell(&image(v.digest), v.count)
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    /// The count for the image either word names (the first that counts),
    /// or nothing counted for the image the first word names. Refuses
    /// anything but two words.
    fn decode(bytes: &[u8]) -> Option<Trial> {
        let bytes: &[u8; 8] = bytes.try_into().ok()?;
        let cell = [0, 4].map(|i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()));
        let named = cell.map(|w| [(w >> 16) as u8, (w >> 8) as u8]);
        let trial = |digest| Trial {
            digest,
            count: tried(cell, &image(digest)),
        };
        Some(
            named
                .into_iter()
                .map(trial)
                .find(|t| t.count > 0)
                .unwrap_or(trial(named[0])),
        )
    }
}
