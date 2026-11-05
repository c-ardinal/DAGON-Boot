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
    /// No chain of links up to the first 親機 (unlinked, a loop of 子機, or
    /// no 親機 at all).
    Unreachable,
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
            Skip::Unreachable => "unreachable",
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
            Skip::Unreachable => "no chain of links leads to it from the first 親機",
            Skip::CutInvalid => {
                "the part updating would reach does not pass `dagon topo check` on its own"
            }
        }
    }
}

/// The part updating reaches, checked on its own. Every node of the 構成
/// is in exactly one of `reach` and `skipped`.
#[derive(Debug, Clone)]
pub struct Cut {
    /// The updatable part; `None` when it does not pass the checks on its
    /// own (then every node is skipped with [`Skip::CutInvalid`]).
    pub topology: Option<Topology>,
    /// Ids of the part's nodes in its order: the compiled topology's node
    /// indices, the one list host, CLI and page take reach from.
    pub reach: Vec<String>,
    /// Nodes updating leaves out, in `nodes` order, with why.
    pub skipped: Vec<(String, Skip)>,
    /// `check` on the part.
    pub report: Report,
}

/// [`updatable`], then `check` on the part itself: a part that does not
/// pass is never handed on as updatable.
pub fn cut_with(reg: &Registry, t: &Topology) -> Cut {
    let (sub, skipped) = updatable(t);
    checked(reg, t, sub, skipped)
}

fn checked(reg: &Registry, t: &Topology, sub: Topology, skipped: Vec<(String, Skip)>) -> Cut {
    let report = check_with(reg, &sub);
    if report.is_pushable() {
        return Cut {
            reach: sub.nodes.iter().map(|n| n.id.clone()).collect(),
            topology: Some(sub),
            skipped,
            report,
        };
    }
    Cut {
        topology: None,
        reach: Vec::new(),
        skipped: t
            .nodes
            .iter()
            .map(|n| (n.id.clone(), Skip::CutInvalid))
            .collect(),
        report,
    }
}

/// The updatable part of `t` and the nodes it leaves out, in `nodes` order.
/// The first 親機 is always in it; its direct I2C/UART 子機 follow in
/// `nodes` order up to eight nodes in all. The part keeps `nodes` order, so
/// a 構成 updating reaches whole compiles to the same bytes as before.
/// Every node not in the part is skipped with a reason.
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
            Err(why) => Some(why),
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
        // a constraint naming a skipped node is dropped (lockstep keeps
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

/// Ok when `id` is the first 親機 or one of its direct I2C/UART 子機; else why not.
fn skip_of(t: &Topology, first: Option<&str>, id: &str) -> Result<(), Skip> {
    let n = t.node(id).ok_or(Skip::Unreachable)?;
    if n.role == Role::Coordinator {
        return if Some(id) == first {
            Ok(())
        } else {
            Err(Skip::SecondHub)
        };
    }
    let up = t.link_to(id).ok_or(Skip::Unreachable)?;
    // The chain must end at a 親機: unlinked, a loop of 子機, or no 親機 at all is unreachable.
    let root = root_of(t, id).ok_or(Skip::Unreachable)?;
    if Some(root) != first {
        return Err(Skip::SecondHub);
    }
    if Some(up.from.as_str()) != first {
        return Err(Skip::Deep);
    }
    if !up.kind.compiles() {
        return Err(Skip::LinkKind);
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

    const SAMPLE: &str = include_str!("../../../topology/sample.yaml");

    #[test]
    fn a_valid_single_hub_topology_is_reached_whole() {
        let t = parse_str(SAMPLE).unwrap();
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

    /// The sample's 親機 and bus with `children` NUCLEO-F401RE 子機 on I2C,
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
        assert!(cut.reach.is_empty());
    }

    #[test]
    fn a_loop_of_children_beside_the_hub_is_unreachable_not_reached() {
        // a -> b -> a: each has its one link (V-05 passes), and no chain ends at a 親機.
        let uart = |from: &str, to: &str| {
            format!("  - from: {from}\n    to: {to}\n    kind: uart\n    iface: hw-uart0\n    pins: {{ tx: 0, rx: 1 }}\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n")
        };
        let yaml = fan(1, false)
            .replace(
                "buses:",
                "  - id: a\n    mcu: rp2040\n    role: node\n    port: self-boot\n    tag: a\n  - id: b\n    mcu: rp2040\n    role: node\n    port: self-boot\n    tag: b\nbuses:",
            )
            .replace("update:", &format!("{}{}update:", uart("a", "b"), uart("b", "a")))
            .replace("order: [n0, hub]", "order: [n0, a, b, hub]");
        let (_, cut) = checked(&yaml);
        assert_eq!(cut.reach, ["hub", "n0"]);
        assert_eq!(
            cut.skipped,
            [
                ("a".to_string(), Skip::Unreachable),
                ("b".to_string(), Skip::Unreachable)
            ]
        );
        assert_partition(&parse_str(&yaml).unwrap(), &cut);
    }

    #[test]
    fn a_topology_that_passes_with_a_part_that_does_not_reports_nothing_updatable() {
        // The part is broken by hand (its 親機 taken out): the checked cut refuses it whole.
        let t = parse_str(&fan(2, false)).unwrap();
        assert!(check_with(Registry::builtin(), &t).is_pushable());
        let (mut sub, skipped) = updatable(&t);
        sub.nodes.retain(|n| n.role != Role::Coordinator);
        let cut = super::checked(Registry::builtin(), &t, sub, skipped);
        assert!(cut.topology.is_none() && cut.reach.is_empty());
        assert_eq!(
            cut.skipped,
            [
                ("hub".to_string(), Skip::CutInvalid),
                ("n0".to_string(), Skip::CutInvalid),
                ("n1".to_string(), Skip::CutInvalid),
            ]
        );
    }

    /// Every node is in exactly one of reach and skipped.
    fn assert_partition(t: &Topology, cut: &Cut) {
        let mut all: Vec<&str> = cut
            .reach
            .iter()
            .map(String::as_str)
            .chain(cut.skipped.iter().map(|(id, _)| id.as_str()))
            .collect();
        all.sort_unstable();
        let mut ids: Vec<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(all, ids);
    }

    /// A generated 構成: `count` nodes (the first `hubs` are 親機), each
    /// 子機 linked from a random node (loops possible), listed shuffled with
    /// h0 kept the first 親機.
    fn generated(next: &mut impl FnMut(usize) -> usize) -> String {
        let count = 2 + next(11);
        let hubs = 1 + next(2);
        let name = |i: usize| {
            if i < hubs {
                format!("h{i}")
            } else {
                format!("n{i}")
            }
        };
        let mut nodes = Vec::new();
        let mut links = Vec::new();
        for i in 0..count {
            let id = name(i);
            if i < hubs {
                nodes.push(format!("  - id: {id}\n    mcu: rp2040\n    board: waveshare-rp2040-touch-lcd-1.28\n    role: coordinator\n    port: self-boot\n    flash_map: rp2040-4m\n    tag: {id}\n"));
                if i > 0 && next(2) == 0 {
                    links.push(format!("  - from: h0\n    to: {id}\n    kind: can\n"));
                }
                continue;
            }
            nodes.push(format!("  - id: {id}\n    mcu: stm32f401re\n    board: nucleo-f401re\n    role: node\n    port: self-boot\n    flash_map: f401-default\n    tag: {id}\n"));
            let p = next(count);
            let parent = if p == i { "h0".to_string() } else { name(p) };
            links.push(if parent == "h0" && next(4) > 0 {
                format!("  - from: h0\n    to: {id}\n    kind: i2c\n    bus: i2c-main\n    addr: {}\n    node_pins: {{ sda: PB4, scl: PA8 }}\n    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\n", 0x10 + i)
            } else if parent.starts_with('h') && next(3) == 0 {
                format!("  - from: {parent}\n    to: {id}\n    kind: can\n")
            } else {
                format!("  - from: {parent}\n    to: {id}\n    kind: uart\n    iface: hw-uart0\n    pins: {{ tx: 0, rx: 1 }}\n    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\n")
            });
        }
        let mut order: Vec<usize> = (0..count).collect();
        for k in (1..count).rev() {
            order.swap(k, next(k + 1));
        }
        let at_h0 = order.iter().position(|&x| x == 0).unwrap();
        let first_hub = order.iter().position(|&x| x < hubs).unwrap();
        order.swap(at_h0, first_hub);
        let listed: String = order.iter().map(|&i| nodes[i].as_str()).collect();
        let ids: Vec<String> = order
            .iter()
            .filter(|&&i| i != 0)
            .map(|&i| name(i))
            .chain(["h0".to_string()])
            .collect();
        format!(
            "dagon: 1\nname: gen\nnodes:\n{listed}buses:\n  - id: i2c-main\n    kind: i2c\n    controller: i2c0\n    sda: 16\n    scl: 17\n    speed_hz: 400000\n    pullups: external\nlinks:\n{}update:\n  order: [{}]\n",
            links.concat(),
            ids.join(", ")
        )
    }

    #[test]
    fn for_generated_topologies_reach_is_the_compiled_order_and_every_node_is_told() {
        use dagon_format::header::target_tag;
        use dagon_format::topology::TopologyView;
        // a small LCG instead of a property-testing crate; 400 shapes.
        let mut seed: u64 = 0x5eed_7090;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n
        };
        let reg = Registry::builtin();
        let mut passed = 0;
        for _ in 0..400 {
            let yaml = generated(&mut next);
            let t = parse_str(&yaml).unwrap();
            if !check_with(reg, &t).is_pushable() {
                continue;
            }
            passed += 1;
            let cut = cut_with(reg, &t);
            assert_partition(&t, &cut);
            assert!(cut.reach.iter().any(|id| id == "h0"), "{yaml}");
            let bin = crate::compile_with(reg, &t).unwrap();
            let v = TopologyView::parse(&bin).unwrap();
            let tags: Vec<_> = (0..v.node_count()).map(|i| v.node(i).target_tag).collect();
            let want: Vec<_> = cut.reach.iter().map(|id| target_tag(id)).collect();
            assert_eq!(tags, want, "{yaml}");
        }
        assert!(
            passed > 100,
            "only {passed} generated topologies passed check"
        );
    }
}
