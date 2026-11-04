//! MCU identity: derived from the MCU definition's `id`, never assigned.
//!
//! `identity(id)` = the first 8 bytes of `SHA-256("dagon-mcu/" ‖ id)` as a
//! little-endian u64. Every carrier holds all 64 bits, so there is one
//! width to check and nothing to truncate: the image header, the identity
//! reply, VERSION 0x06 and the compiled topology's node.

use dagon_format::sha256;
use std::collections::BTreeMap;

pub const PREFIX: &str = "dagon-mcu/";

/// Carriers and the bits each holds. A carrier added with fewer bits makes
/// the loader check that width too.
pub const CARRIERS: &[(&str, u32)] = &[
    ("image-header", 64),
    ("identity-reply", 64),
    ("version-0x06", 64),
    ("topology-node", 64),
];

pub fn identity(id: &str) -> u64 {
    let d = sha256(format!("{PREFIX}{id}").as_bytes());
    u64::from_le_bytes(d[..8].try_into().unwrap())
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
                bits: 64
            }
        );
    }
}
