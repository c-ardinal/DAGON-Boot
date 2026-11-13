//! A request to a linked node goes over the topology link joining it to the
//! node the PC is connected to, by the relay table entry for that node's
//! port; every other case is an error naming the nodes.

use std::path::PathBuf;

use dagon_host::{connected_node, relay_routes, RouteError, Target};
use dagon_port::Addressing;
use dagon_topology::defs::{Registry, Source};
use dagon_topology::Topology;

const NODES: &str = "dagon: 2\nname: routes\nnodes:\n\
  - { id: hub, mcu: rp2040, role: coordinator, port: self-boot, tag: hub }\n\
  - { id: seg, mcu: stm32f401re, role: node, port: self-boot, tag: seg }\n\
  - { id: seg2, mcu: stm32f401re, role: node, port: self-boot, tag: seg2 }\n\
  - { id: watch, mcu: esp32s3, role: node, port: self-boot, tag: watch }\n\
  - { id: far, mcu: stm32f401re, role: node, port: self-boot, tag: far }\n";

const SEG: &str =
    "  - { kind: i2c, ends: [{ node: hub, port: i2c0 }, { node: seg, port: i2c3, addr: 0x42 }] }\n";
const SEG2: &str = "  - { kind: i2c, ends: [{ node: hub, port: i2c1 }, { node: seg2, port: i2c3, addr: 0x43 }] }\n";
const WATCH: &str =
    "  - { kind: uart, ends: [{ node: hub, port: pio-uart }, { node: watch, port: uart0 }] }\n";
/// `far` is linked to `seg` only.
const FAR: &str =
    "  - { kind: i2c, ends: [{ node: seg, port: i2c1 }, { node: far, port: i2c3, addr: 0x44 }] }\n";

fn topo(links: &[&str]) -> Topology {
    dagon_topology::parse_str(&yaml(&links.concat())).unwrap()
}

fn yaml(links: &str) -> String {
    format!("{NODES}links:\n{links}recovery: {{}}\nupdate: {{ order: [] }}\n")
}

/// The built-in definitions with the RP2040 also relaying over i2c1 (id 3).
fn two_i2c_ports() -> Registry {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("dagon-host-routes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(
        &toml,
        "format = 1\n[[mcu_override.rp2040.relay_link]]\nport = \"i2c1\"\nid = 3\n\
         addressing = \"seven-bit\"\nmax_payload = 256\n",
    )
    .unwrap();
    let r = Registry::load(&[Source::Project(&toml)]).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    r
}

fn port(reg: &Registry, name: &str) -> u8 {
    reg.mcu("rp2040").unwrap().port(name).unwrap().index
}

#[test]
fn two_ports_of_one_kind_stay_distinct() {
    let reg = two_i2c_ports();
    let routes = relay_routes(&reg, &topo(&[SEG, SEG2, WATCH]), "hub");
    let seg = routes.to(Target::I2c(0x42)).unwrap();
    let seg2 = routes.to(Target::I2c(0x43)).unwrap();
    assert_eq!(
        (seg.link.id, seg.link.port, seg.addr),
        (1, port(&reg, "i2c0"), 0x42)
    );
    assert_eq!(
        (seg2.link.id, seg2.link.port, seg2.addr),
        (3, port(&reg, "i2c1"), 0x43)
    );
    let watch = routes.to(Target::Uart).unwrap();
    assert_eq!(
        (watch.link.id, watch.link.addressing, watch.addr),
        (2, Addressing::Fixed(0xFF), 0xFF)
    );
}

#[test]
fn a_link_is_found_in_either_orientation() {
    let reg = Registry::builtin();
    let reversed = "  - { kind: i2c, ends: [{ node: seg, port: i2c3, addr: 0x42 }, { node: hub, port: i2c0 }] }\n";
    let a = relay_routes(reg, &topo(&[SEG]), "hub")
        .to(Target::I2c(0x42))
        .unwrap();
    let b = relay_routes(reg, &topo(&[reversed]), "hub")
        .to(Target::I2c(0x42))
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(a.link.id, 1);
}

#[test]
fn a_port_with_no_entry_is_an_error_naming_the_nodes() {
    let r = relay_routes(Registry::builtin(), &topo(&[SEG2]), "hub").to(Target::I2c(0x43));
    assert_eq!(
        r,
        Err(RouteError::NoEntry {
            local: "hub".into(),
            node: "seg2".into(),
            port: "i2c1".into()
        })
    );
    let text = r.unwrap_err().to_string();
    assert!(
        text.contains("'hub'") && text.contains("'seg2'") && text.contains("i2c1"),
        "{text}"
    );
}

#[test]
fn a_connected_node_whose_mcu_has_no_definition_is_an_error() {
    let t = dagon_topology::parse_str(&yaml(SEG).replace("mcu: rp2040", "mcu: nova-m4")).unwrap();
    let r = relay_routes(Registry::builtin(), &t, "hub").to(Target::I2c(0x42));
    assert_eq!(
        r,
        Err(RouteError::NoMcu {
            local: "hub".into(),
            mcu: "nova-m4".into()
        })
    );
}

#[test]
fn zero_or_several_joining_links_are_errors_naming_the_nodes() {
    let reg = Registry::builtin();
    let r = relay_routes(reg, &topo(&[SEG, FAR]), "hub").to(Target::I2c(0x44));
    assert_eq!(
        r,
        Err(RouteError::NoLink {
            local: "hub".into(),
            node: "far".into()
        })
    );
    let again = "  - { kind: i2c, ends: [{ node: hub, port: i2c1 }, { node: seg, port: i2c1, addr: 0x42 }] }\n";
    let r = relay_routes(reg, &topo(&[SEG, again]), "hub").to(Target::I2c(0x42));
    assert_eq!(
        r,
        Err(RouteError::SeveralLinks {
            local: "hub".into(),
            node: "seg".into()
        })
    );
}

#[test]
fn routes_follow_the_node_given_as_connected_not_a_role() {
    // From seg, far is on seg's own i2c1 port; seg's MCU relays over nothing.
    let r = relay_routes(Registry::builtin(), &topo(&[SEG, FAR]), "seg").to(Target::I2c(0x44));
    assert_eq!(
        r,
        Err(RouteError::NoEntry {
            local: "seg".into(),
            node: "far".into(),
            port: "i2c1".into()
        })
    );
}

#[test]
fn a_link_of_a_kind_no_target_names_leaves_the_others_alone() {
    let usb = "  - { kind: usb-cdc, ends: [{ node: far, port: usb-otg-fs }, { node: hub, port: usb }] }\n";
    let r = relay_routes(Registry::builtin(), &topo(&[WATCH, usb]), "hub").to(Target::Uart);
    assert_eq!(r.map(|r| r.link.id), Ok(2));
}

#[test]
fn no_configuration_reaches_only_the_connected_node() {
    let r = dagon_host::Routes::default().to(Target::I2c(0x42));
    assert_eq!(r, Err(RouteError::Unknown(Target::I2c(0x42))));
}

#[test]
fn the_connected_node_is_the_one_the_reachable_part_starts_from() {
    assert_eq!(connected_node(&topo(&[SEG])).unwrap(), "hub");
    // A second 親機 is outside the reachable part (skipped as such).
    let two = dagon_topology::parse_str(&yaml(SEG).replace(
        "role: node, port: self-boot, tag: seg }",
        "role: coordinator, port: self-boot, tag: seg }",
    ))
    .unwrap();
    assert_eq!(connected_node(&two).unwrap(), "hub");
    let none =
        dagon_topology::parse_str(&yaml(SEG).replace("role: coordinator", "role: node")).unwrap();
    assert_eq!(
        connected_node(&none),
        Err(RouteError::Connected { nodes: vec![] })
    );
}
