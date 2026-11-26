//! A node behind a node that relays: updating reaches it when the MCU of
//! the node between says it relays as a node (`[relay] relays_as_node`),
//! and skips it otherwise.

use dagon_topology::defs::{Registry, Source};
use dagon_topology::{check_with, compile_with, cut_with, parse_str, Skip, Topology};

/// The built-in definitions, the RP2040 relaying as a node or not.
fn registry(name: &str, relays: bool) -> Registry {
    let dir = std::env::temp_dir().join(format!("dagon-deep-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    let body = format!("format = 1\n[mcu_override.rp2040.relay]\nrelays_as_node = {relays}\n");
    std::fs::write(&toml, body).unwrap();
    let r = Registry::load(&[Source::Project(&toml)]).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    r
}

fn node(id: &str, mcu: &str, role: &str) -> String {
    let (port, map) = match mcu {
        "rp2040" => ("self-boot", "rp2040-4m"),
        "stm32f401re" => ("self-boot", "f401-default"),
        _ => ("vendor-stage2", "esp32s3-16m"),
    };
    // The RP2040's board states its crystal, which a UART link needs (V-26).
    let board = if mcu == "rp2040" {
        "    board: waveshare-rp2040-touch-lcd-1.28\n"
    } else {
        ""
    };
    format!("  - id: {id}\n    mcu: {mcu}\n{board}    role: {role}\n    port: {port}\n    flash_map: {map}\n    tag: {id}\n")
}

/// A UART link from `near`'s PIO UART (pins on that end) to `far`.
fn uart(near: &str, far: &str, far_port: &str, flip: bool) -> String {
    let a = format!("{{ node: {near}, port: pio-uart, pins: {{ tx: 26, rx: 27 }} }}");
    let b = format!("{{ node: {far}, port: {far_port} }}");
    let (a, b) = if flip { (b, a) } else { (a, b) };
    format!("  - kind: uart\n    iface: pio-uart\n    ends:\n      - {a}\n      - {b}\n    baud: 115200\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n")
}

/// hub - mid - leaf - far, every node between an RP2040; `end` is the MCU
/// of the last.
fn chain(flip: bool) -> Topology {
    let yaml = format!(
        "dagon: 2\nname: chain\nnodes:\n{}{}{}{}links:\n{}{}{}update:\n  order: [far, leaf, mid, hub]\n",
        node("hub", "rp2040", "coordinator"),
        node("mid", "rp2040", "node"),
        node("leaf", "rp2040", "node"),
        node("far", "stm32f401re", "node"),
        uart("hub", "mid", "uart0", flip),
        uart("mid", "leaf", "uart0", flip),
        uart("leaf", "far", "usart1", flip),
    );
    parse_str(&yaml).unwrap()
}

#[test]
fn nodes_behind_a_node_that_relays_are_reached_whichever_end_is_listed_first() {
    let reg = registry("reached", true);
    for flip in [false, true] {
        let t = chain(flip);
        let report = check_with(&reg, &t);
        assert!(report.is_pushable(), "{:#?}", report.issues);
        assert!(
            report.issues.iter().all(|i| i.rule != "V-23"),
            "{:#?}",
            report.issues
        );
        let cut = cut_with(&reg, &t);
        assert_eq!(cut.skipped, []);
        assert_eq!(cut.reach, ["hub", "mid", "leaf", "far"]);
        assert_eq!(cut.topology.unwrap().links.len(), 3);
        assert!(compile_with(&reg, &t).is_ok());
    }
}

#[test]
fn they_are_skipped_when_the_node_between_does_not_relay() {
    for reg in [&registry("skipped", false), Registry::builtin()] {
        let cut = cut_with(reg, &chain(false));
        assert_eq!(
            cut.skipped,
            [
                ("leaf".to_string(), Skip::Deep),
                ("far".to_string(), Skip::Deep)
            ]
        );
        assert_eq!(cut.reach, ["hub", "mid"]);
        let told = check_with(reg, &chain(false));
        let v23: Vec<&str> = told
            .issues
            .iter()
            .filter(|i| i.rule == "V-23")
            .map(|i| i.path.as_str())
            .collect();
        assert_eq!(v23, ["links[1].ends", "links[2].ends"]);
    }
}

#[test]
fn a_node_behind_one_that_does_not_relay_stays_skipped_behind_one_that_does() {
    // hub - mid - leaf - far with leaf an STM32, whose MCU relays over no link.
    let reg = registry("mixed", true);
    let yaml = format!(
        "dagon: 2\nname: chain\nnodes:\n{}{}{}{}links:\n{}{}{}update:\n  order: [far, leaf, mid, hub]\n",
        node("hub", "rp2040", "coordinator"),
        node("mid", "rp2040", "node"),
        node("leaf", "stm32f401re", "node"),
        node("far", "esp32s3", "node"),
        uart("hub", "mid", "uart0", false),
        uart("mid", "leaf", "usart1", false),
        uart("leaf", "far", "uart0", false).replace("pio-uart, pins: { tx: 26, rx: 27 }", "usart2, pins: { tx: 2, rx: 3 }"),
    );
    let cut = cut_with(&reg, &parse_str(&yaml).unwrap());
    assert_eq!(
        cut.skipped,
        [("far".to_string(), Skip::Deep)],
        "{:#?}",
        cut.report.issues
    );
    assert_eq!(cut.reach, ["hub", "mid", "leaf"]);
}

#[test]
fn a_link_between_two_nodes_behind_the_coordinator_is_no_way_round() {
    // mid and a second child of the hub are linked: neither is behind the other.
    let reg = registry("round", true);
    let mut t = chain(false);
    t.nodes.retain(|n| n.id != "far");
    t.links.retain(|l| l.end("far").is_none());
    t.update.order.retain(|id| id != "far");
    // leaf gets its own link to the hub as well as the one to mid.
    let mut own = t.links[0].clone();
    for e in &mut own.ends {
        if e.node == "mid" {
            e.node = "leaf".into();
        } else {
            e.port = "pio-uart1".into();
            e.pins = Some(dagon_topology::schema::UartPins { tx: 18, rx: 28 });
        }
    }
    t.links.push(own);
    let cut = cut_with(&reg, &t);
    assert_eq!(cut.skipped, [], "{:?}", cut.report.issues);
    // The hub's two links; mid - leaf is not one updating runs over.
    let part = cut.topology.unwrap();
    assert_eq!(part.links.len(), 2);
    assert!(part.links.iter().all(|l| l.end("hub").is_some()));
}
