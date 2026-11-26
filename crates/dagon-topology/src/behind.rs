//! The nodes behind a relaying node: what the rules and the part updating
//! reaches both read.

use std::collections::{BTreeMap, BTreeSet};

use crate::defs::Registry;
use crate::schema::{Role, Topology};

/// The relaying nodes a request goes through at most, the coordinator
/// among them: what the host's route holds (`dagon_host::MAX_HOPS`).
pub const MAX_RELAYS: usize = 4;

/// Each node behind a relaying node, with that node: the nodes two or more
/// links from the first coordinator that a chain of I2C/UART links reaches
/// through nodes whose MCU relays as a node. Of two chains the shorter, then
/// the one whose relaying node is listed first. A node with a link of its
/// own to the coordinator is reached over that link or not at all.
pub(crate) fn relayed<'t>(
    reg: &Registry,
    t: &'t Topology,
    first: Option<&'t str>,
) -> BTreeMap<&'t str, &'t str> {
    let mut via = BTreeMap::new();
    let Some(first) = first else { return via };
    let next_to = |at: &'t str| {
        t.links_at(at)
            .filter(|(_, l)| l.kind.compiles())
            .filter_map(move |(_, l)| l.other(at).and_then(|e| t.node(&e.node)))
            .filter(|n| n.role == Role::Node)
    };
    let straight: BTreeSet<&str> = t
        .links_at(first)
        .filter_map(|(_, l)| l.other(first).map(|e| e.node.as_str()))
        .collect();
    let relays = |id: &str| {
        t.node(id)
            .and_then(|n| reg.mcu(&n.mcu))
            .is_some_and(|m| m.relays_as_node())
    };
    let mut frontier: Vec<&str> = t
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| next_to(first).any(|n| n.id == *id))
        .collect();
    // The coordinator is the first relaying node of every chain.
    for _ in 1..MAX_RELAYS {
        let mut next = Vec::new();
        for at in frontier.into_iter().filter(|at| relays(at)) {
            for n in next_to(at) {
                let id = n.id.as_str();
                if !straight.contains(id) && !via.contains_key(id) {
                    via.insert(id, at);
                    next.push(id);
                }
            }
        }
        frontier = next;
    }
    via
}
