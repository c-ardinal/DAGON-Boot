//! A node's target comes from the topology link joining it to the node the
//! PC is connected to, at either end, and the relay table entry for the
//! connected node's port: `Local` for the connected node itself, and an
//! error naming the nodes for every other case.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use dagon_format::topology::{TopoEnd, TopoLink, NONE};
use dagon_host::{
    local_node, node_targets, Host, HostClock, LocalNode, RouteError, Target, Transport,
};
use dagon_port::{Addressing, LinkKind, RelayLinkDef};
use dagon_proto::payload::Relay;
use dagon_proto::{max_payload, Cmd, Frame};
use dagon_sim::{
    block_on, f401_config, hub_config, sample, watch_config, Layout, SimFlash, SimLink, SimNode,
    SimRelay, NODE_I2C_ADDR,
};
use dagon_topology::defs::{AddressingSpec, McuDef, Registry, RelayLinkSpec, Source};

/// This test's own relay table: two I2C ports and one stream.
const BUS_A: RelayLinkDef = RelayLinkDef {
    id: 7,
    port: 4,
    kind: LinkKind::I2c,
    addressing: Addressing::SevenBit,
    max_payload: 200,
};
const BUS_B: RelayLinkDef = RelayLinkDef {
    id: 9,
    port: 5,
    kind: LinkKind::I2c,
    addressing: Addressing::SevenBit,
    max_payload: 120,
};
const STREAM: RelayLinkDef = RelayLinkDef {
    id: 12,
    port: 6,
    kind: LinkKind::Uart,
    addressing: Addressing::Fixed(0xEE),
    max_payload: 300,
};
const TABLE: [RelayLinkDef; 3] = [BUS_A, BUS_B, STREAM];

fn end(node: u8, port: u8, addr: u8) -> TopoEnd {
    TopoEnd { node, port, addr }
}

fn link(a: TopoEnd, b: TopoEnd) -> TopoLink {
    TopoLink {
        ends: [a, b],
        ..Default::default()
    }
}

fn linked(def: RelayLinkDef, addr: u8) -> Target {
    Target::Linked {
        link: def.id,
        addr,
        max_payload: def.max_payload,
    }
}

#[test]
fn the_connected_node_is_local_without_looking_at_links() {
    let never = std::iter::from_fn(|| -> Option<TopoLink> { panic!("links examined") });
    assert_eq!(Target::of_node(never, 3, 3, &[]), Ok(Target::Local));
}

#[test]
fn the_local_node_may_be_either_end() {
    // Local 0 at the first end, the target at the second.
    let first = [link(end(0, BUS_A.port, NONE), end(1, 2, 0x42))];
    assert_eq!(
        Target::of_node(first, 0, 1, &TABLE),
        Ok(linked(BUS_A, 0x42))
    );
    // Local 1 at the second end, the target at the first.
    let second = [link(end(0, 2, 0x42), end(1, BUS_A.port, NONE))];
    assert_eq!(
        Target::of_node(second, 1, 0, &TABLE),
        Ok(linked(BUS_A, 0x42))
    );
}

#[test]
fn a_fixed_address_link_gives_its_address_in_both_orientations() {
    let a = [link(end(0, STREAM.port, NONE), end(1, 0, NONE))];
    let b = [link(end(0, 0, NONE), end(1, STREAM.port, NONE))];
    assert_eq!(Target::of_node(a, 0, 1, &TABLE), Ok(linked(STREAM, 0xEE)));
    assert_eq!(Target::of_node(b, 1, 0, &TABLE), Ok(linked(STREAM, 0xEE)));
}

#[test]
fn two_ports_of_one_kind_give_their_own_entries() {
    let links = [
        link(end(0, BUS_A.port, NONE), end(1, 2, 0x42)),
        link(end(0, BUS_B.port, NONE), end(2, 2, 0x42)),
    ];
    assert_eq!(
        Target::of_node(links, 0, 1, &TABLE),
        Ok(linked(BUS_A, 0x42))
    );
    assert_eq!(
        Target::of_node(links, 0, 2, &TABLE),
        Ok(linked(BUS_B, 0x42))
    );
}

#[test]
fn zero_or_two_joining_links_are_errors_naming_the_nodes() {
    let other = [link(end(1, 2, NONE), end(2, 2, 0x44))];
    let r = Target::of_node(other, 0, 2, &TABLE);
    assert_eq!(r, Err(RouteError::NoLink { local: 0, node: 2 }));
    let text = r.unwrap_err().to_string();
    assert!(text.contains("node 0") && text.contains("node 2"), "{text}");

    let two = [
        link(end(0, BUS_A.port, NONE), end(1, 2, 0x42)),
        link(end(0, BUS_B.port, NONE), end(1, 3, 0x43)),
    ];
    let r = Target::of_node(two, 0, 1, &TABLE);
    assert_eq!(r, Err(RouteError::SeveralLinks { local: 0, node: 1 }));
    let text = r.unwrap_err().to_string();
    assert!(text.contains("node 0") && text.contains("node 1"), "{text}");
}

#[test]
fn a_port_without_an_entry_or_an_end_without_an_address_is_an_error() {
    let unlisted = [link(end(0, 1, NONE), end(1, 2, 0x42))];
    assert_eq!(
        Target::of_node(unlisted, 0, 1, &TABLE),
        Err(RouteError::NoEntry {
            local: 0,
            node: 1,
            port: 1
        })
    );
    let no_addr = [link(end(0, BUS_A.port, NONE), end(1, 2, NONE))];
    assert_eq!(
        Target::of_node(no_addr, 0, 1, &TABLE),
        Err(RouteError::NoAddress { local: 0, node: 1 })
    );
}

const SAMPLE: &str = include_str!("../../../topology/sample.yaml");

/// The sample configuration (hub, seg on I2C, watch on UART), each link's
/// ends written in the order `reversed` asks for.
fn sample(reversed: bool) -> dagon_topology::Topology {
    let mut t = dagon_topology::parse_str(SAMPLE).unwrap();
    if reversed {
        t.links.iter_mut().for_each(|l| l.ends.reverse());
    }
    t
}

/// The RP2040's own table entry for port `name`.
fn rp2040_entry(name: &str) -> RelayLinkDef {
    let m = Registry::builtin().mcu("rp2040").unwrap();
    let port = m.port(name).unwrap().index;
    m.relay_link_defs()
        .into_iter()
        .find(|d| d.port == port)
        .unwrap()
}

#[test]
fn the_yaml_order_of_a_links_ends_does_not_change_the_target() {
    let reg = Registry::builtin();
    let (seg, watch) = (rp2040_entry("i2c0"), rp2040_entry("pio-uart"));
    let want = [
        ("hub".to_string(), Ok(Target::Local)),
        ("seg".to_string(), Ok(linked(seg, 0x42))),
        (
            "watch".to_string(),
            Ok(linked(watch, dagon_proto::payload::link::UART_ADDR)),
        ),
    ];
    for reversed in [false, true] {
        let got = node_targets(reg, &sample(reversed), "hub").unwrap();
        assert_eq!(got, want);
    }
}

#[test]
fn a_connected_node_whose_mcu_has_no_definition_is_a_clear_error() {
    let mut t = sample(false);
    t.nodes[0].mcu = "nova-m4".into();
    let r = local_node(Registry::builtin(), &t, "hub");
    let Err(e) = r else { panic!("{r:?}") };
    assert_eq!(
        e,
        RouteError::NoMcu {
            local: "hub".into(),
            mcu: "nova-m4".into()
        }
    );
    let text = e.to_string();
    assert!(
        text.contains("'hub'") && text.contains("'nova-m4'"),
        "{text}"
    );
}

#[test]
fn the_connected_node_is_the_one_given_not_a_role() {
    // From seg, whose MCU relays over nothing, the hub is not reached.
    let reg = Registry::builtin();
    let got = node_targets(reg, &sample(false), "seg").unwrap();
    assert_eq!(got[1], ("seg".to_string(), Ok(Target::Local)));
    assert!(
        matches!(got[0].1, Err(RouteError::NoEntry { .. })),
        "{:?}",
        got[0]
    );
}

// --- against the simulated relaying node -----------------------------------

/// The RP2040 definition relaying over i2c0, i2c1 and pio-uart with this
/// test's own ids.
fn definition() -> McuDef {
    let spec = |id, port: &str, addressing, max: usize| RelayLinkSpec {
        id,
        port: port.into(),
        addressing,
        max_payload: max as u16,
    };
    let mut def = Registry::builtin().mcu("rp2040").unwrap().clone();
    def.relay_links = vec![
        spec(21, "i2c0", AddressingSpec::SevenBit, max_payload::I2C),
        spec(23, "i2c1", AddressingSpec::SevenBit, max_payload::I2C),
        spec(
            25,
            "pio-uart",
            AddressingSpec::Fixed(0xFF),
            max_payload::UART,
        ),
    ];
    def
}

/// An F401 at [`NODE_I2C_ADDR`] whose HELLO gives `max`, so the test knows
/// which node answered.
fn f401(seed: u64, max: u16) -> SimNode {
    let mut cfg = f401_config();
    cfg.max_payload = max;
    let mut n = SimNode::new(
        cfg,
        sample::PUBKEY,
        SimFlash::stm32f401xe(seed),
        Layout::of("stm32f401re"),
    );
    n.i2c_addr = Some(NODE_I2C_ADDR);
    n.store().provision_core(sample::SEG_CORE_1_0_0).unwrap();
    n.provision(sample::SEG_1_0_0, 0).unwrap();
    n
}

fn watch(seed: u64, max: u16) -> SimNode {
    let mut cfg = watch_config();
    cfg.max_payload = max;
    let mut n = SimNode::new(
        cfg,
        sample::PUBKEY,
        SimFlash::esp32s3_4m(seed),
        Layout::ESP32S3_WATCH,
    );
    n.provision(sample::WATCH_1_0_0, 0).unwrap();
    n
}

/// The relaying node with two F401s at one address on i2c0 and i2c1 and a
/// watch on pio-uart.
fn relay(def: &McuDef) -> SimRelay {
    let node = SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(1),
        Layout::of("rp2040"),
    );
    let mut r = SimRelay::with_definition(node, def);
    r.add_i2c("i2c0", f401(2, 200), false);
    r.add_i2c("i2c1", f401(3, 100), false);
    r.set_uart("pio-uart", SimLink::new(4), watch(5, 300));
    r
}

/// USB to the simulated relaying node, recording the link id of every RELAY.
#[derive(Clone)]
struct Usb {
    relay: Rc<RefCell<SimRelay>>,
    pending: Vec<u8>,
    relayed_over: Rc<RefCell<Vec<u8>>>,
}

impl Transport for Usb {
    type Error = ();
    fn max_payload(&self) -> usize {
        max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        if let Ok((f, _)) = Frame::decode(bytes, max_payload::USB) {
            if f.cmd == Cmd::Relay as u8 {
                let r = Relay::parse(f.payload).unwrap();
                self.relayed_over.borrow_mut().push(r.link);
            }
        }
        self.relay.borrow_mut().usb_write(bytes);
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.pending.extend(self.relay.borrow_mut().usb_read());
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for Usb {
    fn now_ms(&self) -> u64 {
        self.relay.borrow().now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.relay.borrow_mut().advance_ms(ms as u64);
    }
}

fn usb(def: &McuDef) -> Usb {
    Usb {
        relay: Rc::new(RefCell::new(relay(def))),
        pending: Vec::new(),
        relayed_over: Rc::default(),
    }
}

/// Topology node 0 is the relaying node; 1 and 2 sit at one address on
/// its two I2C ports, 3 on its stream (written with the relaying node at
/// either end).
fn sim_links(def: &McuDef) -> Vec<TopoLink> {
    let port = |name: &str| def.port(name).unwrap().index;
    vec![
        link(end(0, port("i2c0"), NONE), end(1, 0, NODE_I2C_ADDR)),
        link(end(0, port("i2c1"), NONE), end(2, 0, NODE_I2C_ADDR)),
        link(end(0, port("pio-uart"), NONE), end(3, 0, NONE)),
    ]
}

#[test]
fn two_nodes_at_one_address_on_two_ports_are_relayed_to_with_their_entries_ids() {
    let def = definition();
    let table = def.relay_link_defs();
    let u = usb(&def);
    let mut h = Host::new(u.clone(), u.clone());
    let links = sim_links(&def);
    let mut seen = Vec::new();
    for node in 1..=3u8 {
        let to = Target::of_node(links.clone(), 0, node, &table).unwrap();
        let hello = block_on(h.hello(to)).unwrap();
        seen.push(hello.max_payload);
        let last = *u.relayed_over.borrow().last().unwrap();
        assert_eq!(last, table[usize::from(node) - 1].id, "node {node}");
    }
    // Each answer came from the node on its own port.
    assert_eq!(seen, [200, 100, 300]);
}

#[test]
fn discover_probes_every_entry_of_the_local_table() {
    let def = definition();
    let table = def.relay_link_defs();
    let u = usb(&def);
    let mut h = Host::new(u.clone(), u.clone());
    h.local = Ok(LocalNode {
        index: 0,
        links: table.clone(),
    });
    let d = block_on(h.discover(&[])).unwrap();
    let found: Vec<_> = d
        .nodes
        .iter()
        .map(|(t, hello)| (*t, hello.max_payload))
        .collect();
    assert_eq!(
        found,
        [
            (linked(table[0], NODE_I2C_ADDR), 200),
            (linked(table[1], NODE_I2C_ADDR), 100),
            (linked(table[2], 0xFF), 300),
        ]
    );
    // Every RELAY went over a listed link, each of them used.
    let used = u.relayed_over.borrow();
    assert!(used.iter().all(|l| table.iter().any(|d| d.id == *l)));
    assert!(table.iter().all(|d| used.contains(&d.id)));
}

#[test]
fn without_a_configuration_discover_reaches_only_the_connected_node() {
    let u = usb(&definition());
    let mut h = Host::new(u.clone(), u.clone());
    let d = block_on(h.discover(&[])).unwrap();
    assert!(d.nodes.is_empty());
    assert!(u.relayed_over.borrow().is_empty());
}

/// The built-in definitions with `overrides` (a project's dagon.toml body).
fn with(name: &str, overrides: &str) -> Registry {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("dagon-host-targets-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(&toml, format!("format = 1\n{overrides}")).unwrap();
    let r = Registry::load(&[Source::Project(&toml)]).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    r
}

#[test]
fn a_projects_override_of_the_table_is_what_routes() {
    let reg = with(
        "override",
        "[[mcu_override.rp2040.relay_link]]\nport = \"i2c0\"\nid = 9\n\
         addressing = \"seven-bit\"\nmax_payload = 64\n",
    );
    let got = node_targets(&reg, &sample(false), "hub").unwrap();
    let entry = reg
        .mcu("rp2040")
        .unwrap()
        .relay_link_defs()
        .into_iter()
        .find(|d| d.max_payload == 64)
        .unwrap();
    assert_ne!(
        entry,
        rp2040_entry("i2c0"),
        "the override is not the built-in entry"
    );
    assert_eq!(got[1], ("seg".to_string(), Ok(linked(entry, 0x42))));
}
