use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use dagon_topology::defs::{Registry, Source};

use super::*;
use crate::simfault::{FAULTS, FAULT_HELP};

const SAMPLE: &str = include_str!("../../../../topology/sample.yaml");
const PAIR: &str = include_str!("../../../../topology/sim-pair.yaml");

fn inject(list: &[&str]) -> Vec<Injection> {
    list.iter().map(|s| Injection::parse(s).unwrap()).collect()
}

/// The result and the lines of one `dagon sim` of `yaml` under `reg`.
fn sim(reg: &Registry, yaml: &str, faults: &[&str]) -> (Result<(), String>, Vec<String>) {
    let t = dagon_topology::parse_str(yaml).unwrap();
    let mut lines = Vec::new();
    let r = run(reg, &t, &inject(faults), &mut |l| lines.push(l));
    (r, lines)
}

/// The definitions of a project whose dagon.toml is `text`.
fn project(text: &str) -> Registry {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir: PathBuf =
        std::env::temp_dir().join(format!("dagon-simworld-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(&toml, text).unwrap();
    let reg = Registry::load(&[Source::Project(&toml)]);
    std::fs::remove_dir_all(&dir).unwrap();
    reg.expect("the project's definitions load")
}

/// The pair with `more` F401s beside seg on the same bus, from 0x43 up.
fn pair_with(more: usize) -> String {
    let mut y = PAIR.to_string();
    for i in 0..more {
        let id = format!("seg{}", i + 2);
        y = y
            .replace(
                "buses:\n",
                &format!("  - id: {id}\n    mcu: stm32f401re\n    board: nucleo-f401re\n    role: node\n    port: self-boot\n    flash_map: f401-default\n    max_attempts: 3\n    listen_window_ms: 50\n    tag: {id}-f401\nbuses:\n"),
            )
            .replace(
                "recovery:\n",
                &format!("  - kind: i2c\n    bus: i2c-main\n    ends:\n      - {{ node: hub, port: i2c0 }}\n      - {{ node: {id}, port: i2c3, addr: {:#04x}, node_pins: {{ sda: PB4, scl: PA8 }} }}\n    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\nrecovery:\n", 0x43 + i),
            )
            .replace(
                "  seg: [local, redistribute, rom-forced]\n",
                &format!("  seg: [local, redistribute, rom-forced]\n  {id}: [local, redistribute]\n"),
            )
            .replace("order: [seg, ", &format!("order: [seg, {id}, "));
    }
    y
}

/// The sample (a hub, an I2C node, a UART node): its update, and every
/// fault on every node the fault has a model for.
#[test]
fn every_fault_passes_on_every_node_of_the_sample() {
    let faults = [
        "hub=power-cut",
        "hub=no-confirm",
        "hub=tamper",
        "seg=power-cut",
        "seg=no-confirm",
        "seg=tamper",
        "seg=erased",
        "watch=power-cut",
        "watch=no-confirm",
        "watch=tamper",
    ];
    let (r, lines) = sim(Registry::builtin(), SAMPLE, &faults);
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert_eq!(lines.len(), 2 + faults.len(), "{lines:#?}");
    assert!(lines[0].starts_with("topology sample: hub (rp2040, connected to the PC), seg ("));
    assert!(lines[1].starts_with("update PASS: hub, seg, watch on 1.1.0"));
    for (f, line) in faults.iter().zip(&lines[2..]) {
        let (node, kind) = f.split_once('=').unwrap();
        assert!(line.starts_with(&format!("{node} {kind} PASS: ")), "{line}");
    }
}

/// A topology the simulator ships no world for: three more nodes on the
/// bus, the fault on the last of them.
#[test]
fn a_topology_of_ones_own_runs_and_takes_a_fault_by_node_id() {
    let yaml = pair_with(3);
    let (r, lines) = sim(
        Registry::builtin(),
        &yaml,
        &["seg4=power-cut", "seg3=no-confirm", "seg2=tamper"],
    );
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(lines[0].contains("seg4 (stm32f401re, link 1 address 0x45)"));
    assert!(lines[1].starts_with("update PASS: hub, seg, seg2, seg3, seg4 on 1.1.0"));
    assert!(lines[2].starts_with("seg4 power-cut PASS"), "{lines:#?}");
    assert!(lines[3].starts_with("seg3 no-confirm PASS"), "{lines:#?}");
    assert!(lines[4].starts_with("seg2 tamper PASS"), "{lines:#?}");
}

/// A project's own definitions are the ones simulated: its relay id routes
/// the write, its ROM loader address takes the ROM write, and its boot
/// attempts end the trial.
#[test]
fn the_projects_definitions_are_the_ones_simulated() {
    let reg = project(
        "format = 1\n\n\
         [[mcu_override.rp2040.relay_link]]\nport = \"i2c0\"\nid = 7\n\n\
         [mcu_override.stm32f401re.rom_i2c]\naddr = 0x3B\n\n\
         [mcu_override.stm32f401re.watchdog]\ntrial_attempts = 2\n",
    );
    let seg = reg.mcu("stm32f401re").unwrap();
    assert_eq!(seg.rom_i2c.as_ref().unwrap().addr, 0x3B);
    assert_eq!(seg.watchdog.unwrap().trial_attempts, 2);
    let (r, lines) = sim(&reg, PAIR, &["seg=erased", "seg=no-confirm"]);
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(
        lines[0].contains("seg (stm32f401re, link 7 address 0x42)"),
        "{lines:#?}"
    );
    // The world itself is built on them.
    let t = dagon_topology::parse_str(PAIR).unwrap();
    let w = World::new(&reg, &t).unwrap();
    let (rom, attempts) = w
        .with_node("seg", |n, _| {
            (n.rom_addr, n.store().layout.trial_attempts())
        })
        .unwrap();
    assert_eq!((rom, attempts), (Some(0x3B), 2));
}

/// A fault the simulator has no model for on the node named, and a node
/// the topology lacks, stop before anything runs.
#[test]
fn a_fault_with_no_model_on_its_node_is_refused_before_anything_runs() {
    for (fault, says) in [
        ("watch=erased", "reach seg, not watch"),
        ("nope=tamper", "no node `nope` (it has hub, seg, watch)"),
    ] {
        let (r, lines) = sim(Registry::builtin(), SAMPLE, &[fault]);
        let e = r.unwrap_err();
        assert!(e.starts_with(&format!("--fault {fault}: ")), "{e}");
        assert!(e.contains(says), "{e}");
        assert!(lines.is_empty(), "{lines:?}");
    }
}

/// A fault that never bites fails its run: the write did not stop.
#[test]
fn a_fault_that_does_not_stop_the_write_fails_its_run() {
    let t = dagon_topology::parse_str(PAIR).unwrap();
    let w = World::new(Registry::builtin(), &t).unwrap();
    // The node already runs the release: nothing is written, so no cut.
    w.write_all().unwrap();
    let e = w.power_cut("seg").unwrap_err();
    assert!(e.contains("did not stop at seg"), "{e}");
}

/// The sample closed into a ring by a link between its two nodes: each is
/// still written over its own link from the node the PC is connected to,
/// and the ring's link carries nothing.
#[test]
fn a_ring_is_updated_over_the_links_from_the_connected_node() {
    let ring = SAMPLE.replace(
        "\nrecovery:",
        "  - kind: uart\n    iface: hw-uart0\n    ends:\n      - { node: seg, port: usart1, pins: { tx: 9, rx: 10 } }\n      - { node: watch, port: uart0 }\n    baud: 115200\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n\nrecovery:",
    );
    assert_ne!(ring, SAMPLE);
    let t = dagon_topology::parse_str(&ring).unwrap();
    let cut = dagon_topology::cut_with(Registry::builtin(), &t);
    assert_eq!(cut.reach, ["hub", "seg", "watch"]);
    assert!(cut.skipped.is_empty(), "{:?}", cut.skipped);
    let whole = dagon_topology::check_with(Registry::builtin(), &t);
    assert!(whole.is_pushable(), "{:?}", whole.issues);
    assert!(
        whole
            .issues
            .iter()
            .any(|i| i.rule == "V-23" && i.message.contains("does not use this one")),
        "{:?}",
        whole.issues
    );
    // `dagon sim` runs the part updating reaches, as every command does.
    let part = cut.topology.expect("the part passes the checks");
    // `dagon gen` builds each node for its link from the connected node:
    // no firmware is asked to relay over the ring's link.
    for n in &part.nodes {
        crate::scaffold::hop_placeholders(Registry::builtin(), &part, n)
            .unwrap_or_else(|e| panic!("{}: {e}", n.id));
    }
    let mut lines = Vec::new();
    let r = run(
        Registry::builtin(),
        &part,
        &inject(&["seg=power-cut", "watch=no-confirm"]),
        &mut |l| lines.push(l),
    );
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(
        lines[1].starts_with("update PASS: hub, seg, watch"),
        "{lines:#?}"
    );
}

/// A connected node with two I2C buses (a project's board that brings both
/// controllers to its header): each bus is a relay link of its own, and
/// the node on each is written and recovers over its bus.
#[test]
fn nodes_on_two_i2c_buses_are_each_updated_over_their_own_bus() {
    let reg = project(
        "format = 1\n\n\
         [board_override.\"waveshare-rp2040-touch-lcd-1.28\"]\n\
         header = [{ pin = \"GPIO16\" }, { pin = \"GPIO17\" }, { pin = \"GPIO18\" }, { pin = \"GPIO26\" }, \
         { pin = \"GPIO27\" }, { pin = \"GPIO28\" }, { pin = \"GPIO6\" }, { pin = \"GPIO7\" }]\n\
         i2c = [{ controller = \"i2c0\", sda = \"GPIO16\", scl = \"GPIO17\" }, \
         { controller = \"i2c1\", sda = \"GPIO6\", scl = \"GPIO7\" }]\n",
    );
    let two = pair_with(1)
        .replace(
            "buses:\n",
            "buses:\n  - id: i2c-aux\n    kind: i2c\n    controller: i2c1\n    sda: 6\n    scl: 7\n    speed_hz: 400000\n    pullups: external\n",
        )
        .replace(
            "  - kind: i2c\n    bus: i2c-main\n    ends:\n      - { node: hub, port: i2c0 }\n      - { node: seg2,",
            "  - kind: i2c\n    bus: i2c-aux\n    ends:\n      - { node: hub, port: i2c1 }\n      - { node: seg2,",
        );
    assert!(two.contains("{ node: hub, port: i2c1 }"), "{two}");
    let rp = reg.mcu("rp2040").unwrap();
    let id = |port: &str| {
        let at = rp.port(port).unwrap().index;
        rp.relay_link_defs()
            .iter()
            .find(|l| l.port == at)
            .unwrap()
            .id
    };
    assert_ne!(id("i2c0"), id("i2c1"));
    let (r, lines) = sim(
        &reg,
        &two,
        &["seg=power-cut", "seg2=power-cut", "seg2=no-confirm"],
    );
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(
        lines[0].contains(&format!(
            "seg (stm32f401re, link {} address 0x42)",
            id("i2c0")
        )) && lines[0].contains(&format!(
            "seg2 (stm32f401re, link {} address 0x43)",
            id("i2c1")
        )),
        "{lines:#?}"
    );
}

/// What the simulator has no model for is refused by name.
#[test]
fn a_topology_past_what_the_simulator_models_is_refused_by_name() {
    // A link to a node behind one whose MCU does not relay as a node.
    let two_hops = pair_with(1).replace(
        "      - { node: hub, port: i2c0 }\n      - { node: seg2, port: i2c3, addr: 0x43",
        "      - { node: seg, port: i2c1 }\n      - { node: seg2, port: i2c3, addr: 0x43",
    );
    let t = dagon_topology::parse_str(&two_hops).unwrap();
    let e = dagon_sim::world::build(Registry::builtin(), &t, [0; 32], 1)
        .map(|_| ())
        .unwrap_err();
    assert!(e.contains("no link joins node seg2 to hub"), "{e}");
    // A node no link reaches.
    let (head, tail) = PAIR.split_once("links:\n").unwrap();
    let alone = format!(
        "{head}recovery:\n{}",
        tail.split_once("recovery:\n").unwrap().1
    );
    let t = dagon_topology::parse_str(&alone).unwrap();
    let e = dagon_sim::world::build(Registry::builtin(), &t, [0; 32], 1)
        .map(|_| ())
        .unwrap_err();
    assert!(e.contains("no link joins node seg to hub"), "{e}");
}

#[test]
fn a_fault_is_written_as_node_and_kind() {
    assert_eq!(
        Injection::parse("seg=power-cut"),
        Ok(Injection {
            node: "seg".into(),
            fault: Fault::PowerCut
        })
    );
    for (_, f) in FAULTS {
        assert_eq!(
            Injection::parse(&format!("n={}", f.name())).unwrap().fault,
            f
        );
        assert!(FAULT_HELP.contains(f.name()), "{}", f.name());
    }
    assert!(Injection::parse("seg").unwrap_err().contains("NODE=KIND"));
    assert!(Injection::parse("=tamper").is_err());
    let e = Injection::parse("seg=boom").unwrap_err();
    assert!(e.contains("no fault `boom`") && e.contains("erased"), "{e}");
}

/// hub - mid - leaf over UART links, mid an RP2040 whose definition says it
/// relays as a node: the leaf is written, and recovers, through both.
#[test]
fn a_node_behind_a_node_that_relays_is_updated_through_it() {
    let reg = project("format = 1\n[mcu_override.rp2040.relay]\nrelays_as_node = true\n");
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
    let (r, lines) = sim(&reg, &yaml, &["leaf=power-cut", "leaf=no-confirm"]);
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(
        lines[0].contains("leaf (stm32f401re, link") && lines[0].contains(", then link"),
        "{lines:#?}"
    );
    assert!(
        lines[1].starts_with("update PASS: hub, mid, leaf"),
        "{lines:#?}"
    );
    // The built-in RP2040 relays as a node as well: the leaf is in the run.
    let t = dagon_topology::parse_str(&yaml).unwrap();
    let part = dagon_topology::cut_with(Registry::builtin(), &t)
        .topology
        .unwrap();
    let mut lines = Vec::new();
    let r = run(Registry::builtin(), &part, &[], &mut |l| lines.push(l));
    assert_eq!(r, Ok(()), "{lines:#?}");
    assert!(
        lines[1].starts_with("update PASS: hub, mid, leaf"),
        "{lines:#?}"
    );
}
