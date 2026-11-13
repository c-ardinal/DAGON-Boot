//! Topology canonical binary: what the hub needs from a topology (nodes,
//! roles, links, addresses, pins), without canvas positions or node names.
//! Unsigned; a release binds it through the manifest `topology_sha256`.
//!
//! Layout: 32-byte head, `node_count` 32-byte nodes, `link_count` 20-byte
//! links, `constraint_count` 16-byte constraints (manifest constraint layout).
//! A link has two ends and no direction; its GPIOs say which end's node they
//! are on. Canonical form: node `index` equals
//! its position, a link's ends in node order, links sorted by their ends' nodes
//! (no two links join the same two nodes), every reserved byte zero, so equal
//! topologies give equal bytes and equal SHA-256.

use crate::le::{arr, put_u16, put_u32, u16_at, u32_at};
use crate::manifest::{Constraint, CONSTRAINT_SIZE, MAX_CONSTRAINTS};
use crate::FormatError;

pub const MAGIC: [u8; 4] = *b"DGTP";
pub const TP_VERSION: u16 = 2;
pub const HEAD_SIZE: usize = 0x20;
pub const NODE_SIZE: usize = 32;
pub const LINK_SIZE: usize = 20;
/// The one node limit: the topology check, the Console and every write use it.
pub const MAX_NODES: usize = 32;
/// One link per node but one: a set of links that joins every node.
pub const MAX_LINKS: usize = MAX_NODES - 1;
// A count is one stored byte, and a constraint's node mask has 32 bits.
const _: () = assert!(MAX_NODES <= 32 && MAX_LINKS <= u8::MAX as usize);
/// Largest canonical binary.
pub const MAX_SIZE: usize = total_size(MAX_NODES, MAX_LINKS, MAX_CONSTRAINTS);

pub const fn total_size(nodes: usize, links: usize, constraints: usize) -> usize {
    HEAD_SIZE + NODE_SIZE * nodes + LINK_SIZE * links + CONSTRAINT_SIZE * constraints
}

pub mod role {
    pub const COORDINATOR: u8 = 0;
    pub const NODE: u8 = 1;
}

/// Flash map id: 0 = the MCU's default layout (the only one so far).
pub mod map_id {
    pub const DEFAULT: u8 = 0;
}

/// Recovery tier codes, up to 3 per node in order of use (0 = none).
pub mod tier {
    pub const NONE: u8 = 0;
    pub const LOCAL: u8 = 1;
    pub const REDISTRIBUTE: u8 = 2;
    pub const ROM_FORCED: u8 = 3;
    pub const ROM_COOP: u8 = 4;
}

pub mod iface {
    pub const HW_UART0: u8 = 1;
    pub const PIO_UART: u8 = 2;
    pub const I2C_BUS: u8 = 3;
}

pub mod control {
    pub const FULL: u8 = 1;
    pub const COOPERATIVE: u8 = 2;
}

pub mod rom_entry {
    pub const NONE: u8 = 0;
    pub const FORCE_DOWNLOAD_BOOT: u8 = 1;
    pub const M5PM1: u8 = 2;
    pub const STM32_I2C_ROM: u8 = 3;
}

/// Pin, I2C address or bus id meaning "not used".
pub const NONE: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopoNode {
    pub index: u8,
    pub role: u8,
    /// Image header `mcu_id` values.
    pub mcu_id: u16,
    /// Image header `port_kind` values.
    pub port_kind: u8,
    pub max_attempts: u8,
    pub listen_window_ms: u16,
    pub map_id: u8,
    pub recovery_tiers: [u8; 3],
    /// Position in the update order (0 first).
    pub order_rank: u8,
    /// Image `target_tag` this node accepts (ASCII, NUL padded).
    pub target_tag: [u8; 16],
}

impl TopoNode {
    pub fn parse(b: &[u8]) -> Self {
        TopoNode {
            index: b[0],
            role: b[1],
            mcu_id: u16_at(b, 2),
            port_kind: b[4],
            max_attempts: b[5],
            listen_window_ms: u16_at(b, 6),
            map_id: b[8],
            recovery_tiers: arr(b, 9),
            order_rank: b[12],
            target_tag: arr(b, 16),
        }
    }

    pub fn to_bytes(&self) -> [u8; NODE_SIZE] {
        let mut b = [0u8; NODE_SIZE];
        b[0] = self.index;
        b[1] = self.role;
        put_u16(&mut b, 2, self.mcu_id);
        b[4] = self.port_kind;
        b[5] = self.max_attempts;
        put_u16(&mut b, 6, self.listen_window_ms);
        b[8] = self.map_id;
        b[9..12].copy_from_slice(&self.recovery_tiers);
        b[12] = self.order_rank;
        b[16..32].copy_from_slice(&self.target_tag);
        b
    }
}

/// One end of a link: the node, the index of its MCU definition's `[[link_port]]`
/// the link runs on, and the 7-bit address the other end reaches it at
/// ([`NONE`] on a kind without addresses, or when it is not reached by one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopoEnd {
    pub node: u8,
    pub port: u8,
    pub addr: u8,
}

/// A link between two nodes, with no direction. Pins are GPIO numbers: a
/// UART's TX/RX and the NRST/BOOT lines on the node of end `gpio_end`, an I2C
/// link's SDA/SCL the bus's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopoLink {
    /// In node order.
    pub ends: [TopoEnd; 2],
    pub iface: u8,
    pub control: u8,
    /// UART TX, or I2C SDA.
    pub tx_pin: u8,
    /// UART RX, or I2C SCL.
    pub rx_pin: u8,
    pub nrst_pin: u8,
    pub boot_pin: u8,
    /// UART baud, or I2C bus speed in Hz.
    pub baud: u32,
    pub rom_entry: u8,
    /// Same value for links sharing a bus; [`NONE`] for UART.
    pub bus_id: u8,
    /// 0 or 1: the end whose node the UART pins and NRST/BOOT GPIOs are on;
    /// [`NONE`] when the link names none.
    pub gpio_end: u8,
}

impl TopoLink {
    /// The end at `node`, if the link has one there.
    pub fn end(&self, node: u8) -> Option<&TopoEnd> {
        self.ends.iter().find(|e| e.node == node)
    }

    /// The end that is not at `node`, when `node` is one of the ends.
    pub fn other(&self, node: u8) -> Option<&TopoEnd> {
        let [a, b] = &self.ends;
        if a.node == node {
            Some(b)
        } else if b.node == node {
            Some(a)
        } else {
            None
        }
    }

    pub fn parse(b: &[u8]) -> Self {
        let end = |i: usize| TopoEnd {
            node: b[i],
            port: b[i + 1],
            addr: b[i + 2],
        };
        TopoLink {
            ends: [end(0), end(3)],
            iface: b[6],
            control: b[7],
            tx_pin: b[8],
            rx_pin: b[9],
            nrst_pin: b[10],
            boot_pin: b[11],
            baud: u32_at(b, 12),
            rom_entry: b[16],
            bus_id: b[17],
            gpio_end: b[18],
        }
    }

    pub fn to_bytes(&self) -> [u8; LINK_SIZE] {
        let mut b = [0u8; LINK_SIZE];
        let [e0, e1] = self.ends;
        b[..12].copy_from_slice(&[
            e0.node,
            e0.port,
            e0.addr,
            e1.node,
            e1.port,
            e1.addr,
            self.iface,
            self.control,
            self.tx_pin,
            self.rx_pin,
            self.nrst_pin,
            self.boot_pin,
        ]);
        put_u32(&mut b, 12, self.baud);
        b[16] = self.rom_entry;
        b[17] = self.bus_id;
        b[18] = self.gpio_end;
        b
    }

    /// The ends' nodes, the order canonical links are sorted in.
    fn key(&self) -> (u8, u8) {
        (self.ends[0].node, self.ends[1].node)
    }
}

fn check_counts(nodes: usize, links: usize, constraints: usize) -> Result<(), FormatError> {
    let ok =
        (1..=MAX_NODES).contains(&nodes) && links <= MAX_LINKS && constraints <= MAX_CONSTRAINTS;
    if ok {
        Ok(())
    } else {
        Err(FormatError::BadField)
    }
}

fn canonical(nodes: &[TopoNode], links: &[TopoLink]) -> bool {
    nodes.iter().enumerate().all(|(i, x)| x.index as usize == i)
        && links
            .iter()
            .all(|l| l.ends[0].node < l.ends[1].node && matches!(l.gpio_end, 0 | 1 | NONE))
        && links.windows(2).all(|w| w[0].key() < w[1].key())
}

/// Writes the canonical binary into `out` and returns its length. Rejects
/// input that is not in canonical order.
pub fn encode(
    nodes: &[TopoNode],
    links: &[TopoLink],
    constraints: &[Constraint],
    out: &mut [u8],
) -> Result<usize, FormatError> {
    let (n, l, m) = (nodes.len(), links.len(), constraints.len());
    check_counts(n, l, m)?;
    if !canonical(nodes, links) {
        return Err(FormatError::BadField);
    }
    let total = total_size(n, l, m);
    let out = out.get_mut(..total).ok_or(FormatError::Truncated)?;
    out.fill(0);
    out[0..4].copy_from_slice(&MAGIC);
    put_u16(out, 4, TP_VERSION);
    out[6] = n as u8;
    out[7] = l as u8;
    put_u32(out, 8, total as u32);
    out[0x14] = m as u8;
    let mut off = HEAD_SIZE;
    for x in nodes {
        out[off..off + NODE_SIZE].copy_from_slice(&x.to_bytes());
        off += NODE_SIZE;
    }
    for x in links {
        out[off..off + LINK_SIZE].copy_from_slice(&x.to_bytes());
        off += LINK_SIZE;
    }
    for x in constraints {
        out[off..off + CONSTRAINT_SIZE].copy_from_slice(&x.to_bytes());
        off += CONSTRAINT_SIZE;
    }
    Ok(total)
}

/// Validated view over a canonical binary.
#[derive(Clone, Copy, Debug)]
pub struct TopologyView<'a> {
    raw: &'a [u8],
}

impl<'a> TopologyView<'a> {
    /// Checks head, sizes, zero reserved bytes and canonical order. `raw` must
    /// be exactly the binary.
    pub fn parse(raw: &'a [u8]) -> Result<Self, FormatError> {
        if raw.len() < HEAD_SIZE {
            return Err(FormatError::Truncated);
        }
        if raw[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if u16_at(raw, 4) != TP_VERSION {
            return Err(FormatError::UnsupportedVersion);
        }
        let (n, l, m) = (raw[6] as usize, raw[7] as usize, raw[0x14] as usize);
        check_counts(n, l, m)?;
        let total = total_size(n, l, m);
        if raw.len() != total {
            return Err(FormatError::Truncated);
        }
        let v = TopologyView { raw };
        // Re-encoding must reproduce the bytes exactly: rejects non-zero
        // reserved bytes inside records and non-canonical order.
        let mut nodes = [TopoNode::default(); MAX_NODES];
        let mut links = [TopoLink::default(); MAX_LINKS];
        let mut cons = [Constraint::default(); MAX_CONSTRAINTS];
        (0..n).for_each(|i| nodes[i] = v.node(i));
        (0..l).for_each(|i| links[i] = v.link(i));
        (0..m).for_each(|i| cons[i] = v.constraint(i));
        let mut buf = [0u8; MAX_SIZE];
        encode(&nodes[..n], &links[..l], &cons[..m], &mut buf)?;
        if buf[..total] != *raw {
            return Err(FormatError::BadField);
        }
        Ok(v)
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.raw
    }

    pub fn node_count(&self) -> usize {
        self.raw[6] as usize
    }

    pub fn link_count(&self) -> usize {
        self.raw[7] as usize
    }

    pub fn constraint_count(&self) -> usize {
        self.raw[0x14] as usize
    }

    /// Panics if `i >= node_count()`.
    pub fn node(&self, i: usize) -> TopoNode {
        assert!(i < self.node_count());
        let off = HEAD_SIZE + NODE_SIZE * i;
        TopoNode::parse(&self.raw[off..off + NODE_SIZE])
    }

    /// Panics if `i >= link_count()`.
    pub fn link(&self, i: usize) -> TopoLink {
        assert!(i < self.link_count());
        let off = HEAD_SIZE + NODE_SIZE * self.node_count() + LINK_SIZE * i;
        TopoLink::parse(&self.raw[off..off + LINK_SIZE])
    }

    /// Panics if `i >= constraint_count()`.
    pub fn constraint(&self, i: usize) -> Constraint {
        assert!(i < self.constraint_count());
        let off = HEAD_SIZE + NODE_SIZE * self.node_count() + LINK_SIZE * self.link_count();
        let off = off + CONSTRAINT_SIZE * i;
        Constraint::parse(&self.raw[off..off + CONSTRAINT_SIZE])
    }
}
