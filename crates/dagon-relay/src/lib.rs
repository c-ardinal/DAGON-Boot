//! The relay: how a node reaches the nodes linked to it, independent of
//! any MCU.
//!
//! - [`cmd`]: the argument rules of the relay commands (RELAY, I2C_RAW).
//! - [`nodes`]: the nodes reached over the links, from the stored topology
//!   or a fallback list.
//! - [`view`]: what a home screen shows about this node and the linked
//!   nodes.
//!
//! The MCU package states the links its relay reaches nodes over, as a
//! list of [`RelayLink`], and where its flash holds the topology; this
//! crate names no link id or topology interface of its own.
#![no_std]
#![allow(async_fn_in_trait)]

pub mod cmd;
pub mod nodes;
pub mod view;

use dagon_format::topology::TopoLink;

/// How a link names the node it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Addressing {
    /// A 7-bit bus address; the topology gives it in the link's
    /// `i2c_addr`.
    SevenBit,
    /// One fixed address: the medium has no addresses.
    Fixed(u8),
}

impl Addressing {
    /// Whether `addr` names a node on such a link.
    pub fn accepts(self, addr: u8) -> bool {
        match self {
            Addressing::SevenBit => addr < 0x80,
            Addressing::Fixed(a) => addr == a,
        }
    }
}

/// One link a node relays over. A node's links form one list; the first
/// entry that carries a topology link is the one that reaches its node.
#[derive(Clone, Copy, Debug)]
pub struct RelayLink {
    /// Its link id (RELAY `link`).
    pub id: u8,
    /// Whether a topology link of this node runs over it: by the
    /// interface, and by the bus where one interface has several.
    pub carries: fn(&TopoLink) -> bool,
    /// Its largest payload (`dagon_proto::max_payload`).
    pub max_payload: usize,
    pub addressing: Addressing,
}
