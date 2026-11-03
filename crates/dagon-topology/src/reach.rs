//! The part of a 構成 updating reaches today: the first 親機 and the 子機
//! linked straight to it over I2C or UART, up to eight nodes. Everything
//! else is a valid definition that updating skips (V-23 tells why).

use std::collections::BTreeSet;

use crate::schema::{Constraint, Role, Topology};

/// Nodes the compiled topology can carry (the hub's table size).
pub const MAX_UPDATABLE_NODES: usize = dagon_format::topology::MAX_NODES;

/// Why updating skips a node for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// A second 親機, or a node in a second 親機's tree.
    SecondHub,
    /// Below a 子機 (孫機 and deeper).
    Deep,
    /// Linked to the 親機 by a kind the compiled topology cannot carry (CAN, Ethernet, ...).
    LinkKind,
    /// Beyond the eight nodes updating covers.
    TooMany,
}

impl Skip {
    /// The name the page and `--json` use.
    pub fn code(self) -> &'static str {
        match self {
            Skip::SecondHub => "second-hub",
            Skip::Deep => "deep",
            Skip::LinkKind => "link-kind",
            Skip::TooMany => "too-many",
        }
    }

    /// One calm sentence for the terminal.
    pub fn reason(self) -> &'static str {
        match self {
            Skip::SecondHub => {
                "it is in a second 親機's tree; updating reaches the first 親機 only"
            }
            Skip::Deep => "it is below a 子機; updating reaches the 親機's direct 子機 only",
            Skip::LinkKind => "its link kind is not one updating can use yet (I2C or UART)",
            Skip::TooMany => "updating reaches up to 8 nodes",
        }
    }
}

/// The updatable part of `t` and the nodes it leaves out, in `nodes` order.
/// A node with no way to the first 親機 (V-05's) is left out without a skip.
pub fn updatable(t: &Topology) -> (Topology, Vec<(String, Skip)>) {
    let first = t
        .nodes
        .iter()
        .find(|n| n.role == Role::Coordinator)
        .map(|n| n.id.as_str());
    let mut keep: BTreeSet<&str> = BTreeSet::new();
    let mut skipped = Vec::new();
    for n in &t.nodes {
        let why = match skip_of(t, first, &n.id) {
            Ok(()) if keep.len() >= MAX_UPDATABLE_NODES => Some(Skip::TooMany),
            Ok(()) => {
                keep.insert(&n.id);
                None
            }
            Err(why) => why,
        };
        if let Some(why) = why {
            skipped.push((n.id.clone(), why));
        }
    }
    let kept = |id: &String| keep.contains(id.as_str());
    let sub = Topology {
        nodes: t.nodes.iter().filter(|n| kept(&n.id)).cloned().collect(),
        links: t
            .links
            .iter()
            .filter(|l| kept(&l.from) && kept(&l.to))
            .cloned()
            .collect(),
        recovery: t
            .recovery
            .iter()
            .filter(|(id, _)| kept(id))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        update: crate::schema::Update {
            order: t
                .update
                .order
                .iter()
                .filter(|id| kept(id))
                .cloned()
                .collect(),
        },
        // ponytail: a constraint naming a skipped node is dropped (lockstep keeps
        // its reached ids); multi-hop relay will carry them whole.
        constraints: t
            .constraints
            .iter()
            .filter_map(|c| match c {
                Constraint::Lockstep(ids) => {
                    let ids: Vec<String> = ids.iter().filter(|id| kept(id)).cloned().collect();
                    (!ids.is_empty()).then_some(Constraint::Lockstep(ids))
                }
                other => other
                    .node_refs()
                    .iter()
                    .all(|id| keep.contains(id))
                    .then(|| other.clone()),
            })
            .collect(),
        ..t.clone()
    };
    (sub, skipped)
}

/// Ok when `id` is the first 親機 or one of its direct I2C/UART 子機;
/// Err(None) when it has no way up to a 親機.
fn skip_of(t: &Topology, first: Option<&str>, id: &str) -> Result<(), Option<Skip>> {
    let n = t.node(id).ok_or(None)?;
    if n.role == Role::Coordinator {
        return if Some(id) == first {
            Ok(())
        } else {
            Err(Some(Skip::SecondHub))
        };
    }
    let up = t.link_to(id).ok_or(None)?;
    if root_of(t, id) != first {
        return Err(root_of(t, id).map(|_| Skip::SecondHub));
    }
    if Some(up.from.as_str()) != first {
        return Err(Some(Skip::Deep));
    }
    if !up.kind.compiles() {
        return Err(Some(Skip::LinkKind));
    }
    Ok(())
}

/// The 親機 at the top of `id`'s chain of links, if the chain ends at one.
fn root_of<'t>(t: &'t Topology, id: &str) -> Option<&'t str> {
    let mut seen = BTreeSet::new();
    let mut at = t.node(id)?;
    while at.role != Role::Coordinator {
        if !seen.insert(at.id.as_str()) {
            return None;
        }
        at = t.node(&t.link_to(&at.id)?.from)?;
    }
    Some(at.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_str;

    const DESK: &str = include_str!("../../../topology/desk-demo.yaml");

    #[test]
    fn a_valid_single_hub_topology_is_reached_whole() {
        let t = parse_str(DESK).unwrap();
        let (sub, skipped) = updatable(&t);
        assert!(skipped.is_empty());
        assert_eq!(sub.nodes.len(), t.nodes.len());
        assert_eq!(sub.links.len(), t.links.len());
    }

    #[test]
    fn a_second_hubs_tree_and_deeper_nodes_are_skipped_with_their_reason() {
        let node = |id: &str, role: &str| {
            format!("  - id: {id}\n    mcu: rp2040\n    role: {role}\n    port: self-boot\n    tag: {id}\n")
        };
        let link = |from: &str, to: &str, kind: &str| {
            format!("  - from: {from}\n    to: {to}\n    kind: {kind}\n")
        };
        let yaml = format!(
            "dagon: 1
name: t
nodes:
{}{}{}{}{}{}links:
{}{}{}{}update:
  order: [a, b, c, d, h2, h]
",
            node("h", "coordinator"),
            node("a", "node"),
            node("b", "node"),
            node("h2", "coordinator"),
            node("c", "node"),
            node("d", "node"),
            link("h", "a", "uart"),
            link("h", "b", "can"),
            link("a", "d", "uart"),
            link("h2", "c", "uart"),
        );
        let (sub, skipped) = updatable(&parse_str(&yaml).unwrap());
        assert_eq!(
            skipped,
            [
                ("b".into(), Skip::LinkKind),
                ("h2".into(), Skip::SecondHub),
                ("c".into(), Skip::SecondHub),
                ("d".into(), Skip::Deep),
            ]
        );
        assert_eq!(sub.update.order, ["a", "h"]);
        assert_eq!(sub.links.len(), 1);
    }
}
