//! The sub nodes polled with HELLO (and shown by a display): from the
//! stored topology display copy when it is valid, else the fallback list
//! the app passes to `relay::start`.

use crate::layout::TOPOLOGY;
use dagon_format::topology::{iface, TopologyView, HEAD_SIZE, MAX_SIZE};
use dagon_port::Flash;
use dagon_proto::payload::link;
use heapless::Vec;

/// Sub nodes the home screen has room for.
pub const MAX_SUBS: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubNode {
    /// Topology node index (SHOW `node`); the hub is 0.
    pub index: u8,
    /// ASCII, NUL padded.
    pub name: [u8; 16],
    pub link: u8,
    pub addr: u8,
}

impl SubNode {
    pub fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }
}

pub type Subs = Vec<SubNode, MAX_SUBS>;

/// The hub itself on the home screen.
pub const HUB: SubNode = named(0, "hub", 0, 0);

pub const fn named(index: u8, s: &str, link: u8, addr: u8) -> SubNode {
    SubNode {
        index,
        name: dagon_format::header::target_tag(s),
        link,
        addr,
    }
}

/// The node list: the topology display copy, else `fallback`.
pub fn load<F: Flash>(flash: &mut F, fallback: &[SubNode]) -> Subs {
    from_topology(flash).unwrap_or_else(|| fallback.iter().take(MAX_SUBS).copied().collect())
}

fn from_topology<F: Flash>(flash: &mut F) -> Option<Subs> {
    let mut raw = [0u8; MAX_SIZE];
    flash.read(TOPOLOGY, &mut raw[..HEAD_SIZE]).ok()?;
    let len = u32::from_le_bytes(raw[8..12].try_into().ok()?) as usize;
    if !(HEAD_SIZE..=MAX_SIZE).contains(&len) {
        return None;
    }
    flash.read(TOPOLOGY, &mut raw[..len]).ok()?;
    let t = TopologyView::parse(&raw[..len]).ok()?;
    let mut subs = Subs::new();
    for l in (0..t.link_count()).map(|i| t.link(i)) {
        let (lk, addr) = match l.iface {
            iface::I2C_BUS => (link::I2C0, l.i2c_addr),
            iface::PIO_UART => (link::PIO_UART, link::UART_ADDR),
            _ => continue,
        };
        let to = l.to as usize;
        let tag = (to < t.node_count()).then(|| t.node(to).target_tag)?;
        let _ = subs.push(SubNode {
            index: l.to,
            name: short_name(&tag),
            link: lk,
            addr,
        });
    }
    Some(subs)
}

/// `seg-f401-7seg` -> `seg`.
fn short_name(tag: &[u8; 16]) -> [u8; 16] {
    let end = tag.iter().position(|&b| b == b'-' || b == 0).unwrap_or(16);
    let mut out = [0u8; 16];
    out[..end].copy_from_slice(&tag[..end]);
    out
}
