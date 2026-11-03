//! The part of a 構成 updating reaches today: the first 親機 and the 子機
//! linked straight to it over I2C or UART, up to eight nodes. Everything
//! else is a valid definition that updating skips (V-23 tells why).

use std::collections::BTreeSet;

use crate::defs::Registry;
use crate::schema::{Constraint, Role, Topology};
use crate::{check_with, Report};

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
    /// Beyond the eight nodes updating covers (the 親機 and its first seven 子機).
    TooMany,
    /// The part updating would reach does not pass the checks on its own,
    /// so nothing is updated.
    CutInvalid,
}

impl Skip {
    /// The name the page and `--json` use.
    pub fn code(self) -> &'static str {
        match self {
            Skip::SecondHub => "second-hub",
            Skip::Deep => "deep",
            Skip::LinkKind => "link-kind",
            Skip::TooMany => "too-many",
            Skip::CutInvalid => "cut-invalid",
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
            Skip::TooMany => "updating reaches the 親機 and its first 7 子機",
            Skip::CutInvalid => {
                "the part updating would reach does not pass `dagon topo check` on its own"
            }
        }
    }
}

/// The part updating reaches, checked on its own.
#[derive(Debug, Clone)]
pub struct Cut {
    /// The updatable part; `None` when it does not pass the checks on its
    /// own (then every node is skipped with [`Skip::CutInvalid`]).
    pub topology: Option<Topology>,
    /// Nodes updating leaves out, in `nodes` order, with why.
    pub skipped: Vec<(String, Skip)>,
    /// `check` on the part (empty issues when it is the whole 構成's).
    pub report: Report,
}

/// [`updatable`], then `check` on the part itself: a part that does not
/// pass is never handed on as updatable.
pub fn cut_with(reg: &Registry, t: &Topology) -> Cut {
    let (sub, skipped) = updatable(t);
    let report = check_with(reg, &sub);
    if report.is_pushable() {
        return Cut {
            topology: Some(sub),
            skipped,
            report,
        };
    }
    let why = |id: &str| {
        skipped
            .iter()
            .find(|(s, _)| s == id)
            .map_or(Skip::CutInvalid, |(_, w)| *w)
    };
    Cut {
        topology: None,
        skipped: t.nodes.iter().map(|n| (n.id.clone(), why(&n.id))).collect(),
        report,
    }
}

/// The updatable part of `t` and the nodes it leaves out, in `nodes` order.
/// The first 親機 is always in it; its direct I2C/UART 子機 follow in
/// `nodes` order up to eight nodes in all. The part keeps `nodes` order, so
/// a 構成 updating reaches whole compiles to the same bytes as before. A
/// node with no way to the first 親機 (V-05's) is left out without a skip.
pub fn updatable(t: &Topology) -> (Topology, Vec<(String, Skip)>) {
    let first = t
        .nodes
        .iter()
        .find(|n| n.role == Role::Coordinator)
        .map(|n| n.id.as_str());
    // The 親機 is reserved first, so 子機 listed before it never push it out.
    let mut keep: BTreeSet<&str> = first.into_iter().collect();
    let mut skipped = Vec::new();
    for n in &t.nodes {
        let why = match skip_of(t, first, &n.id) {
            Ok(()) if Some(n.id.as_str()) == first => None,
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
    // No 親機 at all (V-03's): nothing is reached, and no reason but that.
    first.ok_or(None)?;
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

    /// The desk demo's 親機 and bus with `children` NUCLEO-F401RE 子機 on I2C,
    /// listed before the 親機 when `hub_last`.
    fn fan(children: usize, hub_last: bool) -> String {
        let hub = "  - id: hub
    mcu: rp2040
    board: waveshare-rp2040-touch-lcd-1.28
    role: coordinator
    port: self-boot
    flash_map: rp2040-4m
    tag: hub
";
        let kids: String = (0..children)
            .map(|i| {
                format!(
                    "  - id: n{i}
    mcu: stm32f401re
    board: nucleo-f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: n{i}
"
                )
            })
            .collect();
        let links: String = (0..children)
            .map(|i| {
                format!(
                    "  - from: hub
    to: n{i}
    kind: i2c
    bus: i2c-main
    addr: {}
    node_pins: {{ sda: PB4, scl: PA8 }}
    control:
      kind: cooperative
      rom_entry: stm32-i2c-rom
",
                    0x10 + i
                )
            })
            .collect();
        let order: Vec<String> = (0..children)
            .map(|i| format!("n{i}"))
            .chain(["hub".into()])
            .collect();
        let nodes = if hub_last {
            format!("{kids}{hub}")
        } else {
            format!("{hub}{kids}")
        };
        format!(
            "dagon: 1
name: fan
nodes:
{nodes}buses:
  - id: i2c-main
    kind: i2c
    controller: i2c0
    sda: 16
    scl: 17
    speed_hz: 400000
    pullups: external
links:
{links}update:
  order: [{}]
",
            order.join(", ")
        )
    }

    fn checked(yaml: &str) -> (Topology, Cut) {
        let t = parse_str(yaml).unwrap();
        let reg = Registry::builtin();
        assert!(
            check_with(reg, &t).is_pushable(),
            "{}",
            check_with(reg, &t).to_json()
        );
        let cut = cut_with(reg, &t);
        (t, cut)
    }

    #[test]
    fn eight_children_listed_before_the_hub_keep_the_hub_and_its_first_seven() {
        let (_, cut) = checked(&fan(8, true));
        assert_eq!(cut.skipped, [("n7".to_string(), Skip::TooMany)]);
        let part = cut.topology.expect("an updatable part");
        let ids: Vec<&str> = part.nodes.iter().map(|n| n.id.as_str()).collect();
        // nodes order is kept: the compiled index of each node is its place here.
        assert_eq!(ids, ["n0", "n1", "n2", "n3", "n4", "n5", "n6", "hub"]);
        assert_eq!(part.links.len(), 7);
        assert_eq!(part.update.order.last().map(String::as_str), Some("hub"));
        let bin = crate::compile_str(&fan(8, true)).unwrap();
        let v = dagon_format::topology::TopologyView::parse(&bin).unwrap();
        assert_eq!((v.node_count(), v.link_count()), (8, 7));
        assert_eq!(v.node(7).role, dagon_format::topology::role::COORDINATOR);
    }

    #[test]
    fn more_than_eight_children_after_the_hub_skip_the_last_ones() {
        let (_, cut) = checked(&fan(9, false));
        assert_eq!(
            cut.skipped,
            [
                ("n7".to_string(), Skip::TooMany),
                ("n8".to_string(), Skip::TooMany)
            ]
        );
        let part = cut.topology.unwrap();
        assert_eq!(part.nodes[0].id, "hub");
        assert_eq!(part.nodes.len(), 8);
        assert!(crate::compile_str(&fan(9, false)).is_ok());
    }

    #[test]
    fn children_listed_before_the_hub_are_reached_whole_in_their_order() {
        let (t, cut) = checked(&fan(3, true));
        assert!(cut.skipped.is_empty());
        let part = cut.topology.unwrap();
        assert_eq!(
            part.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
            t.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_part_that_does_not_pass_on_its_own_is_never_updatable() {
        // No 親機 at all: the part is empty of one, so nothing is updatable and every node says why.
        let t = parse_str(&fan(2, false).replace("role: coordinator", "role: node")).unwrap();
        let cut = cut_with(Registry::builtin(), &t);
        assert!(cut.topology.is_none());
        assert!(cut.report.errors().any(|i| i.rule == "V-03"));
        assert!(
            cut.skipped.iter().all(|(_, w)| *w == Skip::CutInvalid),
            "{:?}",
            cut.skipped
        );
        assert_eq!(cut.skipped.len(), 3);
    }
}
