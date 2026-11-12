//! The nodes linked to this node, polled with HELLO (and shown by a
//! display): from the stored topology display copy when it is valid, else
//! the fallback list the app passes in. A linked node is the node at the
//! other end of a topology link that has this node at either end; the
//! topology says nothing else about where this node sits.

use dagon_format::header::target_tag;
use dagon_format::topology::{TopologyView, HEAD_SIZE, MAX_NODES, MAX_SIZE};
use dagon_port::Flash;
use heapless::Vec;

use crate::{Addressing, RelayLink};

/// The index of a node not found in the topology.
pub const NO_INDEX: u8 = u8::MAX;

/// A node reached over one of this node's links, or this node itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkedNode {
    /// Topology node index (SHOW `node`), [`NO_INDEX`] if unknown.
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

/// Every node linked to this one; the topology has at most
/// [`MAX_NODES`] nodes.
pub type LinkedNodes = Vec<LinkedNode, MAX_NODES>;

pub const fn named(index: u8, s: &str, link: u8, addr: u8) -> LinkedNode {
    LinkedNode {
        index,
        name: target_tag(s),
        link,
        addr,
    }
}

/// Who this node is in the topology.
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    /// Its node index, from its definition (`dagon gen` writes it). Only
    /// the index identifies the node: several nodes may run images with
    /// the same target tag.
    pub index: u8,
    /// Its image target tag: only names it.
    pub tag: &'a str,
}

/// This node and the nodes linked to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// This node (`link` and `addr` 0).
    pub own: LinkedNode,
    pub linked: LinkedNodes,
}

/// This node and its linked nodes: from the topology display copy at flash
/// address `topology`, each link reached over the first of `links` that
/// carries it, else `fallback`.
pub fn load<'l, F, I>(
    flash: &mut F,
    topology: u32,
    me: Identity<'_>,
    links: I,
    fallback: &[LinkedNode],
) -> Loaded
where
    F: Flash,
    I: Iterator<Item = &'l RelayLink> + Clone,
{
    let own = LinkedNode {
        index: me.index,
        name: short_name(&target_tag(me.tag)),
        link: 0,
        addr: 0,
    };
    let linked = from_topology(flash, topology, me.index, links)
        .unwrap_or_else(|| fallback.iter().take(MAX_NODES).copied().collect());
    Loaded { own, linked }
}

fn from_topology<'l, F, I>(flash: &mut F, at: u32, this: u8, links: I) -> Option<LinkedNodes>
where
    F: Flash,
    I: Iterator<Item = &'l RelayLink> + Clone,
{
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
        let other = match (l.from == this, l.to == this) {
            (true, false) => l.to,
            (false, true) => l.from,
            _ => continue,
        };
        let Some(k) = links.clone().find(|k| (k.carries)(&l)) else {
            continue;
        };
        let addr = match k.addressing {
            // The topology gives the address of the link's `to` node; a
            // bus target cannot address the node driving its bus.
            Addressing::SevenBit if l.from == this => l.i2c_addr,
            Addressing::SevenBit => continue,
            Addressing::Fixed(a) => a,
        };
        let Some(other_tag) =
            (usize::from(other) < t.node_count()).then(|| t.node(usize::from(other)).target_tag)
        else {
            continue;
        };
        let node = LinkedNode {
            index: other,
            name: short_name(&other_tag),
            link: k.id,
            addr,
        };
        if nodes.push(node).is_err() {
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
    const TAGS: [&str; 7] = [
        "relay-rp2040",
        "seg-f401-7seg",
        "watch-esp32",
        "other",
        "fifth",
        "sixth",
        "seventh",
    ];
    const FIRST: Identity = Identity {
        index: 0,
        tag: "relay-rp2040",
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

    /// A topology at [`AT`] of `count` nodes (tags from [`TAGS`]) and
    /// `links` (from, to, interface, bus id); a link to node `n` has I2C
    /// address 0x40 + n.
    fn topology(count: usize, links: &[(u8, u8, u8, u8)]) -> Mem {
        tagged(&TAGS[..count], links)
    }

    /// [`topology`] with node `i` tagged `tags[i]`.
    fn tagged(tags: &[&str], links: &[(u8, u8, u8, u8)]) -> Mem {
        let nodes: Vec<TopoNode> = tags
            .iter()
            .enumerate()
            .map(|(i, tag)| TopoNode {
                index: i as u8,
                target_tag: target_tag(tag),
                ..TopoNode::default()
            })
            .collect();
        let links: Vec<TopoLink> = links
            .iter()
            .map(|&(from, to, iface, bus_id)| TopoLink {
                from,
                to,
                iface,
                bus_id,
                i2c_addr: 0x40 + to,
                ..TopoLink::default()
            })
            .collect();
        let mut flash = std::vec![0xFF; AT as usize + MAX_SIZE];
        encode(&nodes, &links, &[], &mut flash[AT as usize..]).unwrap();
        Mem(flash)
    }

    /// Node 0 linked to one node per entry of `links` (interface, bus id).
    fn stored(links: &[(u8, u8)]) -> Mem {
        let l: Vec<_> = links
            .iter()
            .enumerate()
            .map(|(i, &(iface, bus))| (0, i as u8 + 1, iface, bus))
            .collect();
        topology(links.len() + 1, &l)
    }

    fn linked<'l>(
        flash: &mut Mem,
        me: Identity,
        links: impl Iterator<Item = &'l RelayLink> + Clone,
    ) -> Vec<LinkedNode> {
        load(flash, AT, me, links, &[]).linked.to_vec()
    }

    #[test]
    fn topology_links_map_through_the_given_links() {
        let mut flash = stored(&[(3, 0), (4, 0), (5, 0)]);
        let got = linked(&mut flash, FIRST, [BUS, STREAM].iter());
        assert_eq!(got, [named(1, "seg", 7, 0x41), named(2, "watch", 9, 0xFF)]);
    }

    #[test]
    fn two_links_on_one_interface_reach_their_nodes_over_their_own_link() {
        let links = [BUS, SECOND_BUS, STREAM];
        let got = linked(&mut stored(&[(3, 0), (3, 1)]), FIRST, links.iter());
        assert_eq!(got, [named(1, "seg", 7, 0x41), named(2, "watch", 8, 0x42)]);
    }

    #[test]
    fn a_topology_link_two_links_carry_goes_over_the_first_in_the_list() {
        let any_bus = RelayLink {
            id: 6,
            carries: |l| l.iface == 3,
            ..BUS
        };
        let got = linked(&mut stored(&[(3, 0)]), FIRST, [any_bus, BUS].iter());
        assert_eq!(got, [named(1, "seg", 6, 0x41)]);
        let got = linked(&mut stored(&[(3, 0)]), FIRST, [BUS, any_bus].iter());
        assert_eq!(got, [named(1, "seg", 7, 0x41)]);
    }

    #[test]
    fn a_node_that_is_not_node_0_takes_only_its_own_links() {
        // 0 -> 1 and 1 -> 3 do not touch node 2 and come first; 0 -> 2
        // reaches it at its `to` end over a stream, 2 -> 4 is its own bus
        // link. (The format lists links by their `to` node.)
        let links = [(0, 1, 3, 0), (0, 2, 4, 0), (1, 3, 3, 0), (2, 4, 3, 0)];
        let me = Identity {
            index: 2,
            tag: "watch-esp32",
        };
        let got = load(&mut topology(5, &links), AT, me, [BUS, STREAM].iter(), &[]);
        assert_eq!(got.own, named(2, "watch", 0, 0));
        assert_eq!(
            got.linked.as_slice(),
            [named(0, "relay", 9, 0xFF), named(4, "fifth", 7, 0x44)]
        );
    }

    #[test]
    fn a_bus_link_with_this_node_as_its_target_reaches_no_node() {
        let me = Identity {
            index: 1,
            tag: "seg-f401-7seg",
        };
        let got = linked(&mut topology(2, &[(0, 1, 3, 0)]), me, [BUS].iter());
        assert_eq!(got, []);
    }

    #[test]
    fn nodes_with_the_same_tag_each_take_the_links_of_their_index() {
        let tags = [
            "seg-f401-7seg",
            "relay-rp2040",
            "relay-rp2040",
            "watch-esp32",
        ];
        let links = [(2, 0, 3, 0), (1, 3, 3, 0)];
        let me = |index| Identity {
            index,
            tag: "relay-rp2040",
        };
        let got = load(&mut tagged(&tags, &links), AT, me(1), [BUS].iter(), &[]);
        assert_eq!(got.own, named(1, "relay", 0, 0));
        assert_eq!(got.linked.as_slice(), [named(3, "watch", 7, 0x43)]);
        let got = load(&mut tagged(&tags, &links), AT, me(2), [BUS].iter(), &[]);
        assert_eq!(got.own, named(2, "relay", 0, 0));
        assert_eq!(got.linked.as_slice(), [named(0, "seg", 7, 0x40)]);
    }

    #[test]
    fn every_linked_node_is_listed_beyond_a_screen_full() {
        let links = [
            (0, 1, 3, 0),
            (0, 2, 3, 0),
            (0, 3, 3, 0),
            (0, 4, 3, 0),
            (0, 5, 4, 0),
        ];
        let got = linked(&mut topology(6, &links), FIRST, [BUS, STREAM].iter());
        assert_eq!(got.len(), 5);
        assert_eq!(got[4], named(5, "sixth", 9, 0xFF));
    }

    #[test]
    fn no_stored_topology_gives_the_fallback() {
        let fallback = [named(1, "a", 7, 0x42); 5];
        let got = load(
            &mut Mem(std::vec![0xFF; 64]),
            0,
            FIRST,
            [BUS].iter(),
            &fallback,
        );
        assert_eq!(got.linked.as_slice(), fallback);
        assert_eq!(got.own, named(0, "relay", 0, 0));
    }
}
