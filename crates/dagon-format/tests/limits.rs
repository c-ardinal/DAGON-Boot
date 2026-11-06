//! The node limit, in the formats that carry it.

use dagon_format::manifest::{self, Constraint, MAX_CONSTRAINTS, MAX_ENTRIES};
use dagon_format::topology::{self as tp, TopoLink, TopoNode, TopologyView};
use dagon_format::{bundle, FormatError};

fn nodes(n: usize) -> Vec<TopoNode> {
    (0..n)
        .map(|i| TopoNode {
            index: i as u8,
            role: u8::from(i != 0),
            ..TopoNode::default()
        })
        .collect()
}

fn links(n: usize) -> Vec<TopoLink> {
    (1..n)
        .map(|i| TopoLink {
            from: 0,
            to: i as u8,
            iface: tp::iface::I2C_BUS,
            i2c_addr: 0x10 + i as u8,
            ..TopoLink::default()
        })
        .collect()
}

#[test]
fn a_topology_at_the_limit_encodes_and_decodes() {
    assert_eq!(tp::MAX_NODES, 32);
    assert_eq!(tp::MAX_LINKS, tp::MAX_NODES - 1);
    let (n, l) = (nodes(tp::MAX_NODES), links(tp::MAX_NODES));
    let c = [Constraint::default(); MAX_CONSTRAINTS];
    let mut out = vec![0u8; tp::MAX_SIZE];
    let len = tp::encode(&n, &l, &c, &mut out).unwrap();
    assert_eq!(len, tp::MAX_SIZE);
    let v = TopologyView::parse(&out[..len]).unwrap();
    assert_eq!(
        (v.node_count(), v.link_count()),
        (tp::MAX_NODES, tp::MAX_LINKS)
    );
    assert_eq!(v.node(tp::MAX_NODES - 1).index as usize, tp::MAX_NODES - 1);
    assert_eq!(v.link(tp::MAX_LINKS - 1).to as usize, tp::MAX_NODES - 1);
}

#[test]
fn one_node_over_the_limit_is_refused() {
    let (n, l) = (nodes(tp::MAX_NODES + 1), links(tp::MAX_NODES));
    let mut out = vec![0u8; tp::MAX_SIZE * 2];
    assert_eq!(
        tp::encode(&n, &l, &[], &mut out),
        Err(FormatError::BadField)
    );
}

#[test]
fn a_package_holds_every_image_of_every_node_at_the_limit() {
    assert_eq!(MAX_ENTRIES, tp::MAX_NODES * manifest::IMAGES_PER_NODE);
    assert_eq!(bundle::MAX_ITEMS, MAX_ENTRIES + 1, "and the topology");
}
