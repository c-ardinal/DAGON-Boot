//! A relaying node behind the connected one: the simulated hub with a
//! second hub behind its UART link, and the seg on that one's I2C bus. The
//! host reaches the seg through both, and asks the nearer relaying node
//! about a seg that does not answer.

use super::*;
use dagon_host::Hop;
use dagon_port::LinkKind;
use dagon_proto::max_payload;
use dagon_sim::{hub_config, Layout, SimFlash, SimLink, SimNode, WATCH_UART_PORT};

const V1: Version = Version::new(1, 0, 0);

/// The hop over `relay`'s link on `port` to `addr`.
fn hop(relay: &SimRelay, port: &str, kind: LinkKind, addr: u8) -> Hop {
    let port = relay.port(port, kind);
    let l = relay.links().iter().find(|l| l.port == port).unwrap();
    Hop {
        link: l.id,
        addr,
        max_payload: l.max_payload,
    }
}

/// The connected hub, the hub behind its UART link (with the seg on its
/// I2C bus), all running 1.0.0; and the targets of the two behind.
fn chain() -> (Sim, Target, Target) {
    let sign = |m: &[u8]| *key().sk.sign(m, None);
    let mut mid = demo_hub(*key().pk, 7);
    mid.node.provision_core(sign).unwrap();
    mid.node.provision(&hub_img(0, V1), 0).unwrap();
    let seg = mid.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).unwrap();
    seg.provision_core(sign).unwrap();
    seg.provision(&seg_img(0, V1), 0).unwrap();
    let second = hop(&mid, NODE_I2C_PORT, LinkKind::I2c, NODE_I2C_ADDR);

    let mut hub = SimNode::new(
        hub_config(),
        *key().pk,
        SimFlash::rp2040_4m(8),
        Layout::of("rp2040"),
    );
    hub.provision_core(sign).unwrap();
    hub.provision(&hub_img(0, V1), 0).unwrap();
    let mut relay = SimRelay::new(hub);
    let first = hop(&relay, WATCH_UART_PORT, LinkKind::Uart, 0xFF);
    relay.set_uart_relay(WATCH_UART_PORT, SimLink::new(9), mid);
    let mid_at = Target::through(&[first]).unwrap();
    let seg_at = Target::through(&[first, second]).unwrap();
    (sim_of(relay), mid_at, seg_at)
}

/// Holds the deep seg in reset, or lets it go.
fn hold_seg(sim: &Sim, low: bool) {
    let now = sim.relay.borrow().now_ms();
    seg_of(sim).set_nrst_low(low, now);
}

/// The deep seg, in the simulator.
fn seg_of(sim: &Sim) -> std::cell::RefMut<'_, SimNode> {
    let hops =
        |to: Target| -> Vec<(u8, u8)> { to.hops().iter().map(|h| (h.link, h.addr)).collect() };
    let at = hops(chain().2);
    std::cell::RefMut::map(sim.relay.borrow_mut(), |r| r.node_through(&at).unwrap())
}

#[test]
fn a_node_behind_a_relaying_node_behind_the_hub_answers_the_host() {
    let (sim, mid_at, seg_at) = chain();
    let mut h = Host::new(sim.clone(), sim.clone());
    let mid = block_on(h.hello(mid_at)).unwrap();
    let seg = block_on(h.hello(seg_at)).unwrap();
    assert_ne!(mid.mcu_id, seg.mcu_id, "two nodes answered, not one twice");
    assert_eq!((mid.active_version, seg.active_version), (V1, V1));
    // The seg is told what both tunnels carry: less than its own link does.
    assert!(usize::from(seg.max_payload) <= max_payload::I2C);
    let state = block_on(h.status(seg_at)).unwrap();
    assert_eq!(state.hello.active_version, V1);
}

#[test]
fn a_silent_deep_node_is_told_apart_by_the_records_of_the_node_it_is_linked_to() {
    let (sim, _, seg_at) = chain();
    let mut h = Host::new(sim.clone(), sim.clone());
    // Never answered: the relaying node behind the hub has no record of it.
    hold_seg(&sim, true);
    assert!(block_on(h.hello(seg_at)).is_err());
    assert_eq!(block_on(h.silence(seg_at)).unwrap(), Silence::NeverReached);
    // Answered once, then silent: that node recorded it.
    hold_seg(&sim, false);
    block_on(h.hello(seg_at)).unwrap();
    hold_seg(&sim, true);
    assert!(block_on(h.hello(seg_at)).is_err());
    assert_eq!(block_on(h.silence(seg_at)).unwrap(), Silence::Lost);
}

#[test]
fn a_relaying_node_held_in_reset_relays_nothing() {
    let (sim, mid_at, seg_at) = chain();
    let mut h = Host::new(sim.clone(), sim.clone());
    block_on(h.hello(seg_at)).unwrap();
    let hold_mid = |low: bool| {
        let at: Vec<(u8, u8)> = mid_at.hops().iter().map(|h| (h.link, h.addr)).collect();
        let mut relay = sim.relay.borrow_mut();
        let now = relay.now_ms();
        relay.node_through(&at).unwrap().set_nrst_low(low, now);
    };
    hold_mid(true);
    assert!(block_on(h.hello(mid_at)).is_err());
    assert!(block_on(h.hello(seg_at)).is_err());
    hold_mid(false);
    block_on(h.hello(seg_at)).unwrap();
}

/// hub - mid - leaf, the links a 構成 gives; the RP2040 relays as a node in
/// the definitions returned.
fn described() -> (dagon_topology::defs::Registry, dagon_topology::Topology) {
    use dagon_topology::defs::{Registry, Source};
    let dir = std::env::temp_dir().join(format!("dagon-host-deep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(
        &toml,
        "format = 1\n[mcu_override.rp2040.relay]\nrelays_as_node = true\n",
    )
    .unwrap();
    let reg = Registry::load(&[Source::Project(&toml)]).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    let rp = |id: &str, role: &str| {
        format!("  - id: {id}\n    mcu: rp2040\n    board: waveshare-rp2040-touch-lcd-1.28\n    role: {role}\n    port: self-boot\n    flash_map: rp2040-4m\n    tag: {id}\n")
    };
    let uart = |near: &str, far: &str, port: &str| {
        format!("  - kind: uart\n    iface: pio-uart\n    ends:\n      - {{ node: {near}, port: pio-uart, pins: {{ tx: 26, rx: 27 }} }}\n      - {{ node: {far}, port: {port} }}\n    baud: 115200\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n")
    };
    let yaml = format!(
        "dagon: 2\nname: chain\nnodes:\n{}{}  - id: leaf\n    mcu: stm32f401re\n    role: node\n    port: self-boot\n    flash_map: f401-default\n    tag: leaf\nlinks:\n{}{}update:\n  order: [leaf, mid, hub]\n",
        rp("hub", "coordinator"),
        rp("mid", "node"),
        uart("hub", "mid", "uart0"),
        uart("mid", "leaf", "usart1"),
    );
    (reg, dagon_topology::parse_str(&yaml).unwrap())
}

#[test]
fn the_definitions_give_the_world_and_the_targets_of_a_node_behind_a_relaying_node() {
    let (reg, t) = described();
    let (mut relay, _) = dagon_sim::world::build(&reg, &t, *key().pk, 3).unwrap();
    let targets = dagon_host::node_targets(&reg, &t, "hub").unwrap();
    let at = |id: &str| {
        let (_, to) = targets.iter().find(|(n, _)| n == id).unwrap();
        to.clone().unwrap()
    };
    for (id, mcu) in [
        ("hub", "rp2040"),
        ("mid", "rp2040"),
        ("leaf", "stm32f401re"),
    ] {
        let hops: Vec<(u8, u8)> = at(id).hops().iter().map(|h| (h.link, h.addr)).collect();
        let node = relay.node_through(&hops).unwrap();
        node.provision_core(|m| *key().sk.sign(m, None)).unwrap();
        node.provision(&img(mcu, id, 0, V1, 0x11), 0).unwrap();
    }
    let sim = sim_of(relay);
    let mut h = Host::new(sim.clone(), sim.clone());
    assert_eq!(at("hub"), Target::Local);
    assert_eq!((at("mid").hops().len(), at("leaf").hops().len()), (1, 2));
    let mid = block_on(h.hello(at("mid"))).unwrap();
    let leaf = block_on(h.hello(at("leaf"))).unwrap();
    assert_ne!(mid.mcu_id, leaf.mcu_id, "two nodes answered, not one twice");
    // The built-in RP2040 relays as a node too: the leaf is in the part
    // updating reaches, through mid.
    let builtin = dagon_topology::defs::Registry::builtin();
    let targets = dagon_host::node_targets(builtin, &t, "hub").unwrap();
    assert!(targets.iter().any(|(n, _)| n == "leaf"));
}
