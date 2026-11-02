//! Projects generated into temporary folders, with a stand-in DAGON clone
//! that holds packages named like the board support packages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

const DESK: &str = include_str!("../../../../topology/desk-demo.yaml");

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dagon-scaffold-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(&d).unwrap()
}

/// A folder standing in for a DAGON clone, with one package per name.
fn clone_with(dir: &Path, names: &[&str]) {
    for n in names {
        let p = dir.join("boards").join(n);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("Cargo.toml"),
            format!("[package]\nname = \"{n}\"\nversion = \"0.0.0\"\n"),
        )
        .unwrap();
    }
}

fn read(base: &Path, rel: &str) -> String {
    std::fs::read_to_string(base.join(rel)).unwrap()
}

fn desk() -> Topology {
    dagon_topology::parse_str(DESK).unwrap()
}

#[test]
fn desk_demo_gets_one_crate_per_node_on_its_board_package() {
    let d = tmp("desk");
    let src = DagonSource {
        path: Some(d.join("dagon")),
        ..DagonSource::default()
    };
    let t = desk();
    let pkgs: Vec<String> = t.nodes.iter().map(|n| package_of(n).name).collect();
    assert_eq!(
        pkgs,
        [
            "dagon-board-waveshare-rp2040-touch-lcd-1-28",
            "dagon-board-nucleo-f401re",
            "dagon-board-m5stopwatch"
        ]
    );
    let names: Vec<&str> = pkgs.iter().map(String::as_str).collect();
    clone_with(&d.join("dagon"), &names);
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    let w = gen(&base, &t, &src, &BTreeMap::new(), &[], false).unwrap();
    assert!(w.iter().all(|x| x.action == "created"), "{w:?}");

    let seg = read(&base, "nodes/seg/Cargo.toml");
    assert!(seg.contains("dagon-board-nucleo-f401re = { path = "));
    assert!(seg.contains("slot-b = [\"dagon-board-nucleo-f401re/slot-b\"]"));
    assert!(read(&base, "nodes/seg/.cargo/config.toml").contains("thumbv7em-none-eabihf"));
    assert!(read(&base, "nodes/hub/.cargo/config.toml").contains("thumbv6m-none-eabi"));
    assert!(read(&base, "nodes/watch/rust-toolchain.toml").contains("esp"));
    assert!(!base.join("nodes/seg/rust-toolchain.toml").exists());

    let wiring = read(&base, "nodes/seg/src/wiring.rs");
    assert!(wiring.contains("id: \"seg\""));
    assert!(wiring.contains(
        "Link::I2c { addr: 0x42, speed_hz: 400000, sda: Some(\"PB4\"), scl: Some(\"PA8\") }"
    ));
    let hub = read(&base, "nodes/hub/src/wiring.rs");
    assert!(hub.contains("hub: true"));
    assert!(hub.contains("Child { id: \"seg\", link: HubLink::I2c { sda: 16, scl: 17, speed_hz: 400000, addr: 0x42, nrst: Some(18), boot: Some(28) } }"));
    assert!(hub
        .contains("Child { id: \"watch\", link: HubLink::Uart { tx: 26, rx: 27, baud: 115200 } }"));
    assert!(read(&base, "nodes/seg/src/main.rs").contains("dagon_board_nucleo_f401re::dagon_main!"));
}

#[test]
fn gen_keeps_the_developers_code_and_rewrites_only_generated_parts() {
    let d = tmp("regen");
    let t = desk();
    let names: Vec<String> = t.nodes.iter().map(|n| package_of(n).name).collect();
    clone_with(
        &d.join("dagon"),
        &names.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let src = DagonSource {
        path: Some(d.join("dagon")),
        ..DagonSource::default()
    };
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    gen(&base, &t, &src, &BTreeMap::new(), &[], false).unwrap();

    std::fs::write(base.join("nodes/seg/src/app.rs"), "// mine").unwrap();
    std::fs::write(base.join("nodes/seg/src/main.rs"), "// my main").unwrap();
    let cargo = base.join("nodes/seg/Cargo.toml");
    let mut c = read(&base, "nodes/seg/Cargo.toml");
    c.push_str("heapless = \"0.8\"\n");
    std::fs::write(&cargo, c).unwrap();
    std::fs::write(base.join("nodes/seg/src/wiring.rs"), "// stale").unwrap();

    // The seg link moves to 0x43 in the topology.
    let t2 = dagon_topology::parse_str(&DESK.replace("addr: 0x42", "addr: 0x43")).unwrap();
    let w = gen(&base, &t2, &src, &BTreeMap::new(), &["seg".into()], false).unwrap();
    let act = |p: &str| w.iter().find(|x| x.path == p).unwrap().action;
    assert_eq!(act("nodes/seg/src/app.rs"), "kept");
    assert_eq!(act("nodes/seg/src/main.rs"), "kept");
    assert_eq!(act("nodes/seg/src/wiring.rs"), "rewritten");
    assert_eq!(read(&base, "nodes/seg/src/app.rs"), "// mine");
    assert_eq!(read(&base, "nodes/seg/src/main.rs"), "// my main");
    assert!(read(&base, "nodes/seg/src/wiring.rs").contains("addr: 0x43"));
    assert!(
        read(&base, "nodes/seg/Cargo.toml").contains("heapless = \"0.8\""),
        "own deps kept"
    );
    assert!(
        w.iter().all(|x| !x.path.starts_with("nodes/hub")),
        "only the named node"
    );

    let w = gen(&base, &t2, &src, &BTreeMap::new(), &["seg".into()], true).unwrap();
    assert_eq!(
        w.iter()
            .find(|x| x.path == "nodes/seg/src/main.rs")
            .unwrap()
            .action,
        "rewritten"
    );
    assert_eq!(
        read(&base, "nodes/seg/src/app.rs"),
        "// mine",
        "app.rs even with --force-main"
    );
}

#[test]
fn a_node_chosen_by_mcu_uses_the_mcu_package_and_its_board_facts() {
    let d = tmp("mcu");
    let yaml = DESK.replace("    board: nucleo-f401re\n", "");
    let t = dagon_topology::parse_str(&yaml).unwrap();
    let seg = t.node("seg").unwrap();
    assert_eq!(package_of(seg).name, "dagon-mcu-stm32f401re");
    clone_with(&d.join("dagon"), &["dagon-mcu-stm32f401re"]);
    let src = DagonSource {
        path: Some(d.join("dagon")),
        ..DagonSource::default()
    };
    let mut boards = BTreeMap::new();
    boards.insert(
        "seg".to_string(),
        BoardFacts {
            flash_bytes: Some(512 * 1024),
        },
    );
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    gen(&base, &t, &src, &boards, &["seg".into()], false).unwrap();
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-mcu-stm32f401re = { path = "));
    assert!(read(&base, "nodes/seg/src/wiring.rs").contains("flash_bytes: Some(524288)"));
}

#[test]
fn a_release_source_pins_the_git_tag() {
    let d = tmp("git");
    let src = DagonSource {
        git: Some("https://example.invalid/dagon.git".into()),
        tag: Some("v0.1.0".into()),
        path: None,
    };
    let t = desk();
    gen(&d, &t, &src, &BTreeMap::new(), &["seg".into()], false).unwrap();
    assert!(read(&d, "nodes/seg/Cargo.toml").contains(
        "dagon-board-nucleo-f401re = { git = \"https://example.invalid/dagon.git\", tag = \"v0.1.0\" }"
    ));
}

#[test]
fn a_missing_board_package_is_reported() {
    let d = tmp("missing");
    let src = DagonSource {
        path: Some(d.join("empty")),
        ..DagonSource::default()
    };
    std::fs::create_dir_all(d.join("empty")).unwrap();
    let e = gen(&d, &desk(), &src, &BTreeMap::new(), &["seg".into()], false).unwrap_err();
    assert!(e.contains("no package 'dagon-board-nucleo-f401re'"), "{e}");
}

#[test]
fn new_makes_the_project_and_a_key_outside_it() {
    let d = tmp("new");
    let proj = d.join("fleet");
    let keys = d.join("keys");
    let made = new_project(&proj, "fleet", DESK, &keys).unwrap();
    assert!(made.iter().any(|l| l.starts_with("key:")));
    assert!(keys.join(dagon_sign::SECRET_FILE).exists());
    let toml = read(&proj, "dagon.toml");
    assert!(toml.contains("topology = \"topology.yaml\"") && toml.contains("[dagon]"));
    assert!(!toml.contains(&std::fs::read_to_string(keys.join(dagon_sign::SECRET_FILE)).unwrap()));
    assert_eq!(read(&proj, "topology.yaml"), DESK);
    // Not over an existing project; the key is kept on a second project.
    assert!(new_project(&proj, "fleet", DESK, &keys).is_err());
    let again = new_project(&d.join("fleet2"), "fleet2", DESK, &keys).unwrap();
    assert!(!again.iter().any(|l| l.starts_with("key:")));

    let cfg = proj.join("dagon.toml");
    let added = add_build_tables(&cfg, &desk()).unwrap();
    assert_eq!(added, ["hub", "seg", "watch"]);
    let c = Config::load(Some(&cfg.to_string_lossy())).unwrap();
    assert_eq!(c.build["seg"].package, PathBuf::from("nodes/seg"));
    assert!(add_build_tables(&cfg, &desk()).unwrap().is_empty());
}
