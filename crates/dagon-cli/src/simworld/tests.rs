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

/// What the simulator has no model for is refused by name.
#[test]
fn a_topology_past_what_the_simulator_models_is_refused_by_name() {
    // A link that does not start at the node the PC is connected to.
    let two_hops = pair_with(1).replace(
        "      - { node: hub, port: i2c0 }\n      - { node: seg2, port: i2c3, addr: 0x43",
        "      - { node: seg, port: i2c1 }\n      - { node: seg2, port: i2c3, addr: 0x43",
    );
    let t = dagon_topology::parse_str(&two_hops).unwrap();
    let e = dagon_sim::world::build(Registry::builtin(), &t, [0; 32], 1)
        .map(|_| ())
        .unwrap_err();
    assert!(
        e.contains("relays one hop from hub") && e.contains("seg and seg2"),
        "{e}"
    );
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
