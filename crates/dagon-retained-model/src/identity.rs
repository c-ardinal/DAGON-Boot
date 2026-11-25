//! MCU identity: derived from the MCU definition's `id`, never assigned.
//!
//! `identity(id)` = the first 8 bytes of `SHA-256("dagon-mcu/" ‖ id)` as a
//! little-endian u64. The image header, VERSION 0x06 and the compiled
//! topology's node hold all 64 bits; the identity reply holds the low 16,
//! so the loader checks both widths.

use crate::harness::{Violation, World};
use crate::rng::Rng;
use dagon_format::header::McuId;
use dagon_format::sha256;
use std::collections::BTreeMap;
use std::marker::PhantomData;

pub const PREFIX: &str = "dagon-mcu/";

/// Carriers and the bits each holds. A carrier added with fewer bits makes
/// the loader check that width too.
pub const CARRIERS: &[(&str, u32)] = &[
    ("image-header", 64),
    ("identity-reply", 16),
    ("version-0x06", 64),
    ("topology-node", 64),
];

pub fn identity(id: &str) -> u64 {
    let d = sha256(format!("{PREFIX}{id}").as_bytes());
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

/// An implementation of the identity: the full value and the 16 bits the
/// identity reply carries.
pub trait Identity {
    fn derive(id: &str) -> u64;
    fn reply(id: &str) -> u16;
}

/// The rule above, as written.
pub struct Spec;

impl Identity for Spec {
    fn derive(id: &str) -> u64 {
        identity(id)
    }
    fn reply(id: &str) -> u16 {
        identity(id) as u16
    }
}

/// The shipped implementation: `dagon_format::header::McuId`, the type
/// image headers, HELLO and the compiled topology's nodes carry.
pub struct Production;

impl Identity for Production {
    fn derive(id: &str) -> u64 {
        decode(&McuId::of_name(id).0)
    }
    fn reply(id: &str) -> u16 {
        McuId::of_name(id).low16()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Two definitions agree in the low `bits` of their identity.
    Collision {
        a: String,
        b: String,
        bits: u32,
    },
    /// An identity that reads as "none" (all zeros) or as erased flash (all
    /// ones) in some carrier.
    Reserved {
        id: String,
        bits: u32,
    },
    Duplicate {
        id: String,
    },
}

/// Identities of the loaded definitions. Matching is exact on 64 bits.
#[derive(Clone, Debug, Default)]
pub struct Table {
    by_value: BTreeMap<u64, String>,
}

fn mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1 << bits) - 1
    }
}

/// The definition loader's check. `derive` is [`identity`] in production;
/// tests pass a weaker function to force collisions.
pub fn load(ids: &[&str], derive: fn(&str) -> u64) -> Result<Table, LoadError> {
    let mut widths: Vec<u32> = CARRIERS.iter().map(|c| c.1).collect();
    widths.sort_unstable();
    widths.dedup();
    let mut t = Table::default();
    for &bits in &widths {
        let m = mask(bits);
        let mut seen: BTreeMap<u64, &str> = BTreeMap::new();
        for &id in ids {
            let v = derive(id) & m;
            if v == 0 || v == m {
                return Err(LoadError::Reserved {
                    id: id.into(),
                    bits,
                });
            }
            if let Some(prev) = seen.insert(v, id) {
                return Err(if prev == id {
                    LoadError::Duplicate { id: id.into() }
                } else {
                    LoadError::Collision {
                        a: prev.into(),
                        b: id.into(),
                        bits,
                    }
                });
            }
        }
    }
    for &id in ids {
        t.by_value.insert(derive(id), id.into());
    }
    Ok(t)
}

impl Table {
    /// The definition a carried identity names; `None` shows as unknown.
    pub fn lookup(&self, v: u64) -> Option<&str> {
        self.by_value.get(&v).map(String::as_str)
    }
}

pub fn encode(v: u64) -> [u8; 8] {
    v.to_le_bytes()
}

pub fn decode(b: &[u8; 8]) -> u64 {
    u64::from_le_bytes(*b)
}

/// A carried value looked up in the loaded table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The identity of the loaded definition `nth` (modulo their number).
    Loaded { nth: u8 },
    /// That identity with the bits of `flip` inverted.
    Damaged { nth: u8, flip: u64 },
    /// The identity of a definition that was not loaded.
    Stranger { n: u16 },
}

/// P12 for the implementation `I`: its identity is the specified function
/// of the id at both carried widths, the loader accepts and refuses what
/// the specification's does, and a carried value matches a definition only
/// when it is that definition's whole identity.
pub struct IdentityWorld<I>(PhantomData<I>);

impl<I: Identity> World for IdentityWorld<I> {
    /// The loaded definitions' ids.
    type Config = Vec<String>;
    type Op = Probe;

    fn generate(rng: &mut Rng) -> (Vec<String>, Vec<Probe>) {
        let mut ids: Vec<String> = (0..1 + rng.below(24))
            .map(|_| format!("mcu-{:x}", rng.below(1 << 20)))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let ops = (0..rng.below(8))
            .map(|_| {
                let nth = rng.byte();
                match rng.below(3) {
                    0 => Probe::Loaded { nth },
                    1 => Probe::Damaged {
                        nth,
                        flip: 1 << rng.below(64),
                    },
                    _ => Probe::Stranger {
                        n: rng.next_u64() as u16,
                    },
                }
            })
            .collect();
        (ids, ops)
    }

    fn run(ids: &Vec<String>, ops: &[Probe], _seed: u64) -> Result<(), Violation> {
        let p12 = |step, detail: String| Err(Violation::new("P12", step, detail));
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        for id in &refs {
            if I::derive(id) != identity(id) || I::reply(id) != identity(id) as u16 {
                return p12(0, format!("{id}: not the specified identity"));
            }
        }
        let table = match (load(&refs, I::derive), load(&refs, identity)) {
            (Ok(t), Ok(_)) => t,
            (Err(a), Err(b)) if a == b => return Ok(()),
            (a, b) => return p12(0, format!("load: {:?}, specified {:?}", a.err(), b.err())),
        };
        for (step, op) in ops.iter().enumerate() {
            let nth = |n: u8| refs[usize::from(n) % refs.len()];
            let (carried, expect) = match *op {
                Probe::Loaded { nth: n } => (I::derive(nth(n)), Some(nth(n))),
                Probe::Damaged { nth: n, flip } => {
                    let v = I::derive(nth(n)) ^ flip;
                    (v, refs.iter().copied().find(|id| identity(id) == v))
                }
                Probe::Stranger { n } => {
                    let id = format!("stranger-{n}");
                    (I::derive(&id), None)
                }
            };
            if table.lookup(carried) != expect {
                return p12(
                    step,
                    format!("{carried:#018x} matched {:?}", table.lookup(carried)),
                );
            }
        }
        Ok(())
    }

    fn simpler(op: &Probe) -> Vec<Probe> {
        match *op {
            Probe::Loaded { nth } if nth != 0 => vec![Probe::Loaded { nth: 0 }],
            Probe::Damaged { nth, flip } if nth != 0 => vec![Probe::Damaged { nth: 0, flip }],
            Probe::Stranger { n } if n != 0 => vec![Probe::Stranger { n: 0 }],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_pure_and_round_trips() {
        let v = identity("stm32f401re");
        assert_eq!(v, identity("stm32f401re"));
        assert_ne!(v, identity("rp2040"));
        assert_eq!(decode(&encode(v)), v);
    }

    #[test]
    fn collision_names_both_ids() {
        fn weak(id: &str) -> u64 {
            1 + id.len() as u64
        }
        let e = load(&["aa", "bb"], weak).unwrap_err();
        assert_eq!(
            e,
            LoadError::Collision {
                a: "aa".into(),
                b: "bb".into(),
                bits: 16
            }
        );
    }
}
