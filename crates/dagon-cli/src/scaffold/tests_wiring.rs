//! wiring.rs gives every node the nodes its links reach, whatever its role.

use super::*;

/// The sample with a second F401 on the hub's bus: the hub has three
/// links, each other node one.
fn three_links() -> Topology {
    let yaml = SAMPLE
        .replace(
            "\nbuses:",
            "  - id: seg2\n    mcu: stm32f401re\n    board: nucleo-f401re\n    role: node\n    \
             port: self-boot\n    flash_map: f401-default\n    tag: seg2-f401\n\nbuses:",
        )
        .replace(
            "\n  - kind: uart\n",
            "\n  - kind: i2c\n    bus: i2c-main\n    ends:\n      - { node: hub, port: i2c0 }\n      \
             - { node: seg2, port: i2c3, addr: 0x43 }\n  - kind: uart\n",
        );
    dagon_topology::parse_str(&yaml).unwrap()
}

/// The entries of `REACHED` in `wiring`, one line each.
fn reached(wiring: &str) -> Vec<&str> {
    let at = wiring
        .find("pub const REACHED: &[Reached] = &[\n")
        .unwrap_or_else(|| panic!("no REACHED in {wiring}"));
    wiring[at..]
        .lines()
        .skip(1)
        .take_while(|l| *l != "];")
        .map(str::trim)
        .collect()
}

fn wiring_of(t: &Topology, id: &str) -> String {
    let index = t.nodes.iter().position(|n| n.id == id).unwrap();
    wiring_rs(t, index, None, &[], None)
}

#[test]
fn a_node_with_one_link_lists_the_node_it_reaches() {
    let t = three_links();
    let seg = wiring_of(&t, "seg");
    for field in [
        "pub struct Reached {\n",
        "    pub index: u8,\n",
        "    pub name: &'static str,\n",
        "    pub link: &'static str,\n",
        "    pub addr: Option<u8>,\n}",
    ] {
        assert!(seg.contains(field), "{field} in {seg}");
    }
    assert_eq!(
        reached(&seg),
        [r#"Reached { index: 0, name: "hub", link: "i2c3", addr: None },"#]
    );
    assert_eq!(
        reached(&wiring_of(&t, "watch")),
        [r#"Reached { index: 0, name: "hub", link: "uart0", addr: None },"#]
    );
}

#[test]
fn a_node_with_three_links_lists_each_node_it_reaches_in_link_order() {
    let hub = wiring_of(&three_links(), "hub");
    assert_eq!(
        reached(&hub),
        [
            r#"Reached { index: 1, name: "seg", link: "i2c0", addr: Some(0x42) },"#,
            r#"Reached { index: 3, name: "seg2", link: "i2c0", addr: Some(0x43) },"#,
            r#"Reached { index: 2, name: "watch", link: "pio-uart", addr: None },"#,
        ]
    );
    // One list for every node: no list kept for one role only.
    assert!(!hub.contains("CHILDREN"), "{hub}");
}
