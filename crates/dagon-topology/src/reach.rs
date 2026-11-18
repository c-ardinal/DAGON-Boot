//! The part of a 構成 updating reaches: the first coordinator and the nodes
//! linked straight to it over I2C or UART, whichever end of the link each is
//! listed as. Everything else is a valid definition that updating skips
//! (V-23 tells why).

use std::collections::BTreeSet;

use crate::defs::Registry;
use crate::schema::{Constraint, Node, Role, Topology};
use crate::{check_with, Report};

/// Why updating skips a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// A second coordinator, or a node in a second coordinator's tree.
    SecondHub,
    /// Linked through another node (two or more links from the coordinator).
    Deep,
    /// Linked to the coordinator by a kind the compiled topology cannot carry (CAN, Ethernet, ...).
    LinkKind,
    /// No chain of links up to the first coordinator (unlinked, a loop of nodes with `role: node`, or
    /// no coordinator at all).
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
            Skip::Unreachable => "unreachable",
            Skip::CutInvalid => "cut-invalid",
        }
    }

    /// One calm sentence for the terminal.
    pub fn reason(self) -> &'static str {
        match self {
            Skip::SecondHub => {
                "it is in a second coordinator's tree; updating reaches the first coordinator only"
            }
            Skip::Deep => "it is linked through another node; updating reaches only the nodes linked straight to the coordinator",
            Skip::LinkKind => "its link is of a kind updating does not use (I2C and UART only)",
            Skip::Unreachable => "no chain of links leads to it from the first coordinator",
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

/// The coordinator the updatable part starts from: the node the PC reaches the
/// others through (the first coordinator listed).
pub fn reach_root(t: &Topology) -> Option<&str> {
    t.nodes
        .iter()
        .find(|n| n.role == Role::Coordinator)
        .map(|n| n.id.as_str())
}

/// The updatable part of `t` and the nodes it leaves out, in `nodes` order.
/// The first coordinator is always in it; the nodes linked straight to it over I2C/UART follow in
/// `nodes` order. The part keeps `nodes` order, so
/// a 構成 updating reaches whole compiles to the same bytes as before.
/// Every node not in the part is skipped with a reason.
pub fn updatable(t: &Topology) -> (Topology, Vec<(String, Skip)>) {
    let first = reach_root(t);
    // The coordinator is reserved first, so nodes listed before it never push it out.
    let mut keep: BTreeSet<&str> = first.into_iter().collect();
    let mut skipped = Vec::new();
    for n in &t.nodes {
        let why = match skip_of(t, first, &n.id) {
            Ok(()) if Some(n.id.as_str()) == first => None,
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
        // Only the first coordinator's own links: a link between two reached nodes
        // is not one updating runs over.
        links: t
            .links
            .iter()
            .filter(|l| {
                l.ends.iter().all(|e| kept(&e.node)) && first.is_some_and(|f| l.end(f).is_some())
            })
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

/// Ok when `id` is the first coordinator or a node linked straight to it over I2C/UART; else why not.
fn skip_of(t: &Topology, first: Option<&str>, id: &str) -> Result<(), Skip> {
    let n = t.node(id).ok_or(Skip::Unreachable)?;
    if n.role == Role::Coordinator {
        return if Some(id) == first {
            Ok(())
        } else {
            Err(Skip::SecondHub)
        };
    }
    // No coordinator over any chain of links (unlinked, or only nodes with `role: node`) is unreachable.
    let (hub, hops) = nearest_hub(t, first, n).ok_or(Skip::Unreachable)?;
    if Some(hub) != first {
        return Err(Skip::SecondHub);
    }
    if hops > 1 {
        return Err(Skip::Deep);
    }
    let link = t.link_between(id, hub).ok_or(Skip::Unreachable)?;
    if !link.kind.compiles() {
        return Err(Skip::LinkKind);
    }
    Ok(())
}

/// The coordinator nearest to `from` over links, whichever end each lists first, and
/// how many links away; of two at the same distance the first coordinator.
fn nearest_hub<'t>(
    t: &'t Topology,
    first: Option<&str>,
    from: &'t Node,
) -> Option<(&'t str, usize)> {
    let mut seen = BTreeSet::from([from.id.as_str()]);
    let mut frontier = vec![from.id.as_str()];
    let mut hops = 0;
    while !frontier.is_empty() {
        hops += 1;
        let mut next = Vec::new();
        for at in frontier {
            for (_, l) in t.links_at(at) {
                let Some(n) = l.other(at).and_then(|e| t.node(&e.node)) else {
                    continue;
                };
                if seen.insert(n.id.as_str()) {
                    next.push(n);
                }
            }
        }
        let hubs: Vec<&str> = next
            .iter()
            .filter(|n| n.role == Role::Coordinator)
            .map(|n| n.id.as_str())
            .collect();
        if let Some(&h) = hubs.iter().find(|&&h| Some(h) == first).or(hubs.first()) {
            return Some((h, hops));
        }
        frontier = next.iter().map(|n| n.id.as_str()).collect();
    }
    None
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
        // Each link once with the coordinator or the node nearer the PC listed first, once the other way.
        let link = |a: &str, b: &str, kind: &str, flip: bool| {
            let port = if kind == "uart" { "uart0" } else { "usb" };
            let (a, b) = if flip { (b, a) } else { (a, b) };
            format!("  - kind: {kind}\n    ends:\n      - {{ node: {a}, port: {port} }}\n      - {{ node: {b}, port: {port} }}\n")
        };
        for flip in [false, true] {
            let yaml = format!(
                "dagon: 2
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
                link("h", "a", "uart", flip),
                link("h", "b", "usb-cdc", flip),
                link("a", "d", "uart", flip),
                link("h2", "c", "uart", flip),
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

    #[test]
    fn a_link_between_two_reached_children_is_not_in_the_part() {
        let yaml = fan(2, false).replace(
            "update:",
            "  - kind: i2c\n    bus: i2c-main\n    ends:\n      - { node: n1, port: i2c3, addr: 0x30, node_pins: { sda: PB4, scl: PA8 } }\n      - { node: n0, port: i2c3 }\n    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\nupdate:",
        );
        let (sub, skipped) = updatable(&parse_str(&yaml).unwrap());
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(sub.links.len(), 2);
        assert!(sub.links.iter().all(|l| l.end("hub").is_some()));
    }

    /// The sample's coordinator and bus with `children` NUCLEO-F401RE nodes on I2C,
    /// listed before the coordinator when `hub_last`.
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
                    "  - kind: i2c
    bus: i2c-main
    ends:
      - {{ node: hub, port: i2c0 }}
      - {{ node: n{i}, port: i2c3, addr: {}, node_pins: {{ sda: PB4, scl: PA8 }} }}
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
            "dagon: 2
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
        checked_in(Registry::builtin(), yaml)
    }

    fn checked_in(reg: &Registry, yaml: &str) -> (Topology, Cut) {
        let t = parse_str(yaml).unwrap();
        assert!(
            check_with(reg, &t).is_pushable(),
            "{}",
            check_with(reg, &t).to_json()
        );
        let cut = cut_with(reg, &t);
        (t, cut)
    }

    /// What the check allows, updating reaches: nothing is left out for its count.
    #[test]
    fn a_topology_at_the_limit_is_reached_whole() {
        let children = dagon_format::topology::MAX_NODES - 1;
        for hub_last in [true, false] {
            let (t, cut) = checked(&fan(children, hub_last));
            assert!(cut.skipped.is_empty(), "{:?}", cut.skipped);
            let part = cut.topology.expect("an updatable part");
            assert_eq!(part.nodes.len(), t.nodes.len());
            let bin = crate::compile_str(&fan(children, hub_last)).unwrap();
            let v = dagon_format::topology::TopologyView::parse(&bin).unwrap();
            assert_eq!((v.node_count(), v.link_count()), (children + 1, children));
        }
    }

    #[test]
    fn one_node_over_the_limit_is_an_error_of_the_check() {
        let t = parse_str(&fan(dagon_format::topology::MAX_NODES, false)).unwrap();
        let report = check_with(Registry::builtin(), &t);
        assert!(!report.is_pushable());
        assert!(
            report.issues.iter().any(|i| i.rule == "V-02"),
            "{}",
            report.to_json()
        );
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
        // No coordinator at all: the part is empty of one, so nothing is updatable and every node says why.
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
        // a - b - c - a: each has its links (V-05 passes), and no chain reaches a coordinator.
        let uart = |a: &str, b: &str| {
            format!("  - kind: uart\n    iface: hw-uart0\n    ends:\n      - {{ node: {a}, port: uart0, pins: {{ tx: 0, rx: 1 }} }}\n      - {{ node: {b}, port: uart1 }}\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n")
        };
        let node = |id: &str| {
            format!("  - id: {id}\n    mcu: rp2040\n    role: node\n    port: self-boot\n    tag: {id}\n")
        };
        let yaml = fan(1, false)
            .replace(
                "buses:",
                &format!("{}{}{}buses:", node("a"), node("b"), node("c")),
            )
            .replace(
                "update:",
                &format!(
                    "{}{}{}update:",
                    uart("a", "b"),
                    uart("b", "c"),
                    uart("c", "a")
                ),
            )
            .replace("order: [n0, hub]", "order: [n0, a, b, c, hub]");
        // a, b and c name no board, so their apps run on the RP2040's ring
        // oscillator; a project bounds it (1 %) so their UART links check.
        let dir = std::env::temp_dir().join(format!("dagon-reach-loop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml = dir.join("dagon.toml");
        std::fs::write(
            &toml,
            "format = 1\n[[mcu_override.rp2040.app.clock]]\nsource = \"internal\"\n\
             out_hz = 6_500_000\ntolerance_ppm = 10_000\n",
        )
        .unwrap();
        let reg = Registry::load(&[crate::defs::Source::Project(&toml)]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let (_, cut) = checked_in(&reg, &yaml);
        assert_eq!(cut.reach, ["hub", "n0"]);
        assert_eq!(
            cut.skipped,
            [
                ("a".to_string(), Skip::Unreachable),
                ("b".to_string(), Skip::Unreachable),
                ("c".to_string(), Skip::Unreachable)
            ]
        );
        assert_partition(&parse_str(&yaml).unwrap(), &cut);
    }

    #[test]
    fn a_topology_that_passes_with_a_part_that_does_not_reports_nothing_updatable() {
        // The part is broken by hand (its coordinator taken out): the checked cut refuses it whole.
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

    /// A generated 構成: `count` nodes (the first `hubs` are coordinators), each
    /// other node linked from a random node (loops possible), listed shuffled with
    /// h0 kept the first coordinator.
    fn generated(next: &mut impl FnMut(usize) -> usize) -> String {
        // A link of `kind` (with any lines that follow it) joining two ends
        // (node, port, more fields), listed in either order.
        let link =
            |flip: bool, kind: &str, a: (&str, &str, &str), b: (&str, &str, &str), rest: &str| {
                let end = |(n, p, x): (&str, &str, &str)| {
                    format!("      - {{ node: {n}, port: {p}{x} }}\n")
                };
                let (a, b) = if flip {
                    (end(b), end(a))
                } else {
                    (end(a), end(b))
                };
                format!("  - kind: {kind}\n    ends:\n{a}{b}{rest}")
            };
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
                    links.push(link(
                        next(2) == 0,
                        "usb-cdc",
                        ("h0", "usb", ""),
                        (&id, "usb", ""),
                        "",
                    ));
                }
                continue;
            }
            nodes.push(format!("  - id: {id}\n    mcu: stm32f401re\n    board: nucleo-f401re\n    role: node\n    port: self-boot\n    flash_map: f401-default\n    tag: {id}\n"));
            let p = next(count);
            let parent = if p == i { "h0".to_string() } else { name(p) };
            let (usb, uart) = if parent.starts_with('h') {
                ("usb", "uart0")
            } else {
                ("usb-otg-fs", "usart1")
            };
            let flip = next(2) == 0;
            let control = "    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\n";
            links.push(if parent == "h0" && next(4) > 0 {
                let addr = format!(", addr: {}, node_pins: {{ sda: PB4, scl: PA8 }}", 0x10 + i);
                link(
                    flip,
                    "i2c\n    bus: i2c-main",
                    ("h0", "i2c0", ""),
                    (&id, "i2c3", &addr),
                    control,
                )
            } else if parent.starts_with('h') && next(3) == 0 {
                link(
                    flip,
                    "usb-cdc",
                    (&parent, usb, ""),
                    (&id, "usb-otg-fs", ""),
                    "",
                )
            } else {
                link(
                    flip,
                    "uart\n    iface: hw-uart0",
                    (&parent, uart, ", pins: { tx: 0, rx: 1 }"),
                    (&id, "usart1", ""),
                    control,
                )
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
            "dagon: 2\nname: gen\nnodes:\n{listed}buses:\n  - id: i2c-main\n    kind: i2c\n    controller: i2c0\n    sda: 16\n    scl: 17\n    speed_hz: 400000\n    pullups: external\nlinks:\n{}update:\n  order: [{}]\n",
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
