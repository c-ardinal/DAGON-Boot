//! The linked nodes polled with HELLO (and shown by a display): from the
//! stored topology display copy when it is valid, else the fallback list
//! the app passes in.

use dagon_format::topology::{TopologyView, HEAD_SIZE, MAX_SIZE};
use dagon_port::Flash;
use heapless::Vec;

use crate::{Addressing, RelayLink};

/// Linked nodes the home screen has room for.
pub const MAX_LINKED: usize = 3;

/// A node reached over one of this node's links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkedNode {
    /// Topology node index (SHOW `node`); this node is 0.
    pub index: u8,
    /// ASCII, NUL padded.
    pub name: [u8; 16],
    pub link: u8,
    pub addr: u8,
}

impl LinkedNode {
    pub fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }
}

pub type LinkedNodes = Vec<LinkedNode, MAX_LINKED>;

/// The hub itself on the home screen.
pub const HUB: LinkedNode = named(0, "hub", 0, 0);

pub const fn named(index: u8, s: &str, link: u8, addr: u8) -> LinkedNode {
    LinkedNode {
        index,
        name: dagon_format::header::target_tag(s),
        link,
        addr,
    }
}

/// The node list: the topology display copy at flash address `topology`,
/// each of its links reached over the first of `links` that carries it,
/// else `fallback`.
pub fn load<F: Flash>(
    flash: &mut F,
    topology: u32,
    links: &[RelayLink],
    fallback: &[LinkedNode],
) -> LinkedNodes {
    from_topology(flash, topology, links)
        .unwrap_or_else(|| fallback.iter().take(MAX_LINKED).copied().collect())
}

fn from_topology<F: Flash>(flash: &mut F, at: u32, links: &[RelayLink]) -> Option<LinkedNodes> {
    let mut raw = [0u8; MAX_SIZE];
    flash.read(at, &mut raw[..HEAD_SIZE]).ok()?;
    let len = u32::from_le_bytes(raw[8..12].try_into().ok()?) as usize;
    if !(HEAD_SIZE..=MAX_SIZE).contains(&len) {
        return None;
    }
    flash.read(at, &mut raw[..len]).ok()?;
    let t = TopologyView::parse(&raw[..len]).ok()?;
    let mut nodes = LinkedNodes::new();
    for l in (0..t.link_count()).map(|i| t.link(i)) {
        let Some(k) = links.iter().find(|k| (k.carries)(&l)) else {
            continue;
        };
        let addr = match k.addressing {
            Addressing::SevenBit => l.i2c_addr,
            Addressing::Fixed(a) => a,
        };
        let to = l.to as usize;
        let tag = (to < t.node_count()).then(|| t.node(to).target_tag)?;
        // The home screen shows the first ones; the rest are still written.
        if nodes
            .push(LinkedNode {
                index: l.to,
                name: short_name(&tag),
                link: k.id,
                addr,
            })
            .is_err()
        {
            break;
        }
    }
    Some(nodes)
}

/// `seg-f401-7seg` -> `seg`.
fn short_name(tag: &[u8; 16]) -> [u8; 16] {
    let end = tag.iter().position(|&b| b == b'-' || b == 0).unwrap_or(16);
    let mut out = [0u8; 16];
    out[..end].copy_from_slice(&tag[..end]);
    out
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use dagon_format::topology::{encode, TopoLink, TopoNode};
    use dagon_port::{FlashError, FlashRegion};
    use std::vec::Vec;

    const AT: u32 = 0x100;
    const BUS: RelayLink = RelayLink {
        id: 7,
        carries: |l| l.iface == 3 && l.bus_id == 0,
        max_payload: 32,
        addressing: Addressing::SevenBit,
    };
    const SECOND_BUS: RelayLink = RelayLink {
        id: 8,
        carries: |l| l.iface == 3 && l.bus_id == 1,
        max_payload: 32,
        addressing: Addressing::SevenBit,
    };
    const STREAM: RelayLink = RelayLink {
        id: 9,
        carries: |l| l.iface == 4,
        max_payload: 64,
        addressing: Addressing::Fixed(0xFF),
    };

    struct Mem(Vec<u8>);

    impl Flash for Mem {
        fn geometry(&self) -> &[FlashRegion] {
            &[]
        }
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            let at = addr as usize;
            let src = self.0.get(at..at + buf.len()).ok_or(FlashError::Range)?;
            buf.copy_from_slice(src);
            Ok(())
        }
        fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
            Err(FlashError::Hw)
        }
        fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
            Err(FlashError::Hw)
        }
    }

    /// A topology at [`AT`]: this node, then one node per entry of
    /// `links`, each reached over a link with that (interface, bus id).
    fn stored(links: &[(u8, u8)]) -> Mem {
        let tags = ["hub", "seg-f401-7seg", "watch-esp32", "other"];
        let nodes: Vec<TopoNode> = (0..=links.len())
            .map(|i| TopoNode {
                index: i as u8,
                target_tag: dagon_format::header::target_tag(tags[i]),
                ..TopoNode::default()
            })
            .collect();
        let links: Vec<TopoLink> = links
            .iter()
            .enumerate()
            .map(|(i, &(iface, bus_id))| TopoLink {
                to: i as u8 + 1,
                iface,
                bus_id,
                i2c_addr: 0x40 + i as u8,
                ..TopoLink::default()
            })
            .collect();
        let mut flash = std::vec![0xFF; AT as usize + MAX_SIZE];
        encode(&nodes, &links, &[], &mut flash[AT as usize..]).unwrap();
        Mem(flash)
    }

    #[test]
    fn topology_links_map_through_the_given_links() {
        let got = load(
            &mut stored(&[(3, 0), (4, 0), (5, 0)]),
            AT,
            &[BUS, STREAM],
            &[],
        );
        assert_eq!(
            got.as_slice(),
            [named(1, "seg", 7, 0x40), named(2, "watch", 9, 0xFF)]
        );
    }

    #[test]
    fn two_links_on_one_interface_reach_their_nodes_over_their_own_link() {
        let links = [BUS, SECOND_BUS, STREAM];
        let got = load(&mut stored(&[(3, 0), (3, 1)]), AT, &links, &[]);
        assert_eq!(
            got.as_slice(),
            [named(1, "seg", 7, 0x40), named(2, "watch", 8, 0x41)]
        );
    }

    #[test]
    fn no_stored_topology_gives_the_fallback() {
        let fallback = [named(1, "a", 7, 0x42); MAX_LINKED + 1];
        let got = load(&mut Mem(std::vec![0xFF; 64]), 0, &[BUS], &fallback);
        assert_eq!(got.as_slice(), &fallback[..MAX_LINKED]);
    }
}
