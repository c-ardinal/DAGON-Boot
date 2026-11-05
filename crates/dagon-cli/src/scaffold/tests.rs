//! Projects generated into temporary folders, with a stand-in DAGON clone
//! holding packages named and declared like the board support packages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

/// Today's source of a DAGON developer's project: this clone.
fn dev() -> crate::dagonsrc::NewSource {
    let here = std::fs::canonicalize(crate::dagonsrc::BUILT_FROM).unwrap();
    crate::dagonsrc::NewSource::Clone(crate::config::plain_path(here))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dagon-scaffold-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(&d).unwrap()
}

/// Adds package `name` to the stand-in clone; `meta` is extra manifest text
/// (such as a `[package.metadata.dagon]` table); `template` files go into
/// its `template/` folder.
fn package(clone: &Path, name: &str, meta: &str, template: &[(&str, &str)]) {
    let p = clone.join("packages").join(name);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\n{meta}"),
    )
    .unwrap();
    for (rel, text) in template {
        let f = p.join("template").join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
}

/// The template the M5StopWatch board package ships.
const WATCH_TEMPLATE: &[(&str, &str)] = &[
    (
        ".cargo/config.toml",
        include_str!("../../../../targets/boards/m5stopwatch/template/.cargo/config.toml"),
    ),
    (
        "Cargo.toml.tpl",
        include_str!("../../../../targets/boards/m5stopwatch/template/Cargo.toml.tpl"),
    ),
    (
        "rust-toolchain.toml",
        include_str!("../../../../targets/boards/m5stopwatch/template/rust-toolchain.toml"),
    ),
    (
        "src/app.rs",
        include_str!("../../../../targets/boards/m5stopwatch/template/src/app.rs"),
    ),
    (
        "src/main.rs",
        include_str!("../../../../targets/boards/m5stopwatch/template/src/main.rs"),
    ),
];

// The manifest under its template name, as the board packages ship it.
const F401_TEMPLATE: &[(&str, &str)] = &[
    (
        "Cargo.toml.tpl",
        "[package]\nname = \"{{node}}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{{dep:dagon-board-nucleo-f401re}}\n",
    ),
    ("src/main.rs", "// start-up of {{node}} ({{tag}})\n"),
    ("src/app.rs", "// app of {{node}}\n"),
];

/// A clone with the ESP32-S3 packages (built-in template) and an F401
/// board package that ships its own template; no hub package yet.
fn clone(d: &Path) -> DagonSource {
    let c = d.join("dagon");
    package(&c, "dagon-mcu-esp32s3", "", &[]);
    package(&c, "dagon-board-m5stopwatch", "", WATCH_TEMPLATE);
    package(&c, "dagon-board-nucleo-f401re", "", F401_TEMPLATE);
    DagonSource {
        path: Some(c),
        ..DagonSource::default()
    }
}

fn read(base: &Path, rel: &str) -> String {
    std::fs::read_to_string(base.join(rel)).unwrap()
}

fn sample() -> Topology {
    dagon_topology::parse_str(SAMPLE).unwrap()
}

fn project(d: &Path) -> PathBuf {
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn sample_uses_each_packages_template_and_skips_what_is_missing() {
    let d = tmp("sample");
    let src = clone(&d);
    let base = project(&d);
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &[],
        false,
    )
    .unwrap();

    // The hub's board package does not exist yet: skipped, with the reason.
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert_eq!(r.skipped[0].0, "hub");
    assert!(r.skipped[0]
        .1
        .contains("dagon-board-waveshare-rp2040-touch-lcd-1-28"));
    assert!(!base.join("nodes/hub").exists());

    // ESP32-S3: the template the board package ships, on the real package API.
    let watch = read(&base, "nodes/watch/Cargo.toml");
    assert!(watch.contains("name = \"watch\""));
    assert!(watch.contains("dagon-board-m5stopwatch = { path = "));
    assert!(watch.contains("dagon-mcu-esp32s3 = { path = "));
    let main = read(&base, "nodes/watch/src/main.rs");
    assert!(main.contains("board::start(") && main.contains("NodeConfig::app(app::dagon())"));
    assert!(read(&base, "nodes/watch/src/app.rs").contains("lease.busy(node::now_ms()"));
    assert!(read(&base, "nodes/watch/rust-toolchain.toml").contains("esp"));
    assert!(read(&base, "nodes/watch/.cargo/config.toml").contains("xtensa-esp32s3-none-elf"));

    // F401: the template the package ships, placeholders filled.
    assert_eq!(
        read(&base, "nodes/seg/src/main.rs"),
        "// start-up of seg (seg-f401-7seg)\n"
    );
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-board-nucleo-f401re = { path = "));

    // wiring.rs from the topology, for every generated node.
    let wiring = read(&base, "nodes/seg/src/wiring.rs");
    assert!(wiring.contains(
        "Link::I2c { addr: 0x42, speed_hz: 400000, sda: Some(\"PB4\"), scl: Some(\"PA8\") }"
    ));
    assert!(read(&base, "nodes/watch/src/wiring.rs").contains("Link::Uart { baud: 115200 }"));
}

#[test]
fn gen_keeps_the_developers_code_and_refreshes_only_generated_parts() {
    let d = tmp("regen");
    let src = clone(&d);
    let base = project(&d);
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &[],
        false,
    )
    .unwrap();

    std::fs::write(base.join("nodes/watch/src/app.rs"), "// mine").unwrap();
    std::fs::write(base.join("nodes/watch/src/main.rs"), "// my main").unwrap();
    let cargo = base.join("nodes/watch/Cargo.toml");
    let own = read(&base, "nodes/watch/Cargo.toml").replace(
        "embassy-time = \"0.5\"",
        "embassy-time = \"0.5\"\nheapless = \"0.8\"",
    );
    // A stale DAGON package entry under a target table is brought in line.
    let own = own + "\n[target.'cfg(target_arch = \"xtensa\")'.dependencies]\ndagon-mcu-esp32s3 = { path = \"stale\" }\n";
    std::fs::write(&cargo, own).unwrap();

    let t2 = dagon_topology::parse_str(&SAMPLE.replace("baud: 115200", "baud: 230400")).unwrap();
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t2,
        &src,
        &BTreeMap::new(),
        &["watch".into()],
        false,
    )
    .unwrap();
    let act = |p: &str| r.written.iter().find(|x| x.path == p).unwrap().action;
    assert_eq!(act("nodes/watch/src/app.rs"), "kept");
    assert_eq!(act("nodes/watch/src/main.rs"), "kept");
    assert_eq!(act("nodes/watch/src/wiring.rs"), "rewritten");
    assert_eq!(act("nodes/watch/Cargo.toml"), "rewritten");
    assert_eq!(read(&base, "nodes/watch/src/app.rs"), "// mine");
    assert!(read(&base, "nodes/watch/src/wiring.rs").contains("baud: 230400"));
    let c = read(&base, "nodes/watch/Cargo.toml");
    assert!(c.contains("heapless = \"0.8\""), "own dependency kept");
    assert!(!c.contains("\"stale\""), "{c}");
    assert!(r.written.iter().all(|x| !x.path.starts_with("nodes/seg")));

    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t2,
        &src,
        &BTreeMap::new(),
        &["watch".into()],
        true,
    )
    .unwrap();
    let act = |p: &str| r.written.iter().find(|x| x.path == p).unwrap().action;
    assert_eq!(act("nodes/watch/src/main.rs"), "rewritten");
    assert_eq!(
        read(&base, "nodes/watch/src/app.rs"),
        "// mine",
        "app.rs even with --force-main"
    );
}

#[test]
fn a_node_chosen_by_mcu_finds_the_package_that_declares_the_mcu() {
    let d = tmp("mcu");
    let c = d.join("dagon");
    let tpl: &[(&str, &str)] = &[(
        "Cargo.toml",
        "[package]\nname = \"{{node}}\"\n[dependencies]\n{{dep:dagon-mcu-stm32f401}}\n",
    )];
    // Package name and topology MCU id differ; the package declares the id.
    package(
        &c,
        "dagon-mcu-stm32f401",
        "[package.metadata.dagon]\nmcu = \"stm32f401re\"\n",
        tpl,
    );
    let src = DagonSource {
        path: Some(c),
        ..DagonSource::default()
    };
    let yaml = SAMPLE.replace("    board: nucleo-f401re\n", "");
    let t = dagon_topology::parse_str(&yaml).unwrap();
    let mut boards = BTreeMap::new();
    boards.insert(
        "seg".to_string(),
        BoardFacts {
            flash_bytes: Some(512 * 1024),
        },
    );
    let base = project(&d);
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t,
        &src,
        &boards,
        &["seg".into()],
        false,
    )
    .unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-mcu-stm32f401 = { path = "));
    assert!(read(&base, "nodes/seg/src/wiring.rs").contains("flash_bytes: Some(524288)"));
}

#[test]
fn a_release_source_pins_the_git_tag_and_reads_its_packages_from_cargos_checkout() {
    let d = tmp("git");
    // A stand-in release repository, tagged, served from a local file URL.
    let c = d.join("release");
    package(&c, "dagon-board-m5stopwatch", "", WATCH_TEMPLATE);
    package(
        &c,
        "dagon-format",
        "[lib]\npath = \"lib.rs\"\n[workspace]\n",
        &[],
    );
    std::fs::write(c.join("packages/dagon-format/lib.rs"), "").unwrap();
    let url = crate::dagonsrc::tests::commit_and_tag(&c, "v0.1.0");
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    let src = DagonSource {
        git: Some(url.clone()),
        tag: Some("v0.1.0".into()),
        path: None,
    };
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["watch".into()],
        false,
    )
    .unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(read(&base, "nodes/watch/Cargo.toml").contains(&format!(
        "dagon-board-m5stopwatch = {{ git = {url:?}, tag = \"v0.1.0\" }}"
    )));

    // A tag the repository does not have: the reason, and nothing written.
    let missing = DagonSource {
        tag: Some("v9.0.0".into()),
        ..src
    };
    let e = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &missing,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    );
    let why = format!("{e:?}");
    assert!(why.contains("cannot get the DAGON source v9.0.0"), "{why}");
}

#[test]
fn new_pins_this_clis_release_unless_a_clone_is_asked_for() {
    let d = tmp("new-release");
    let keys = d.join("keys");
    let release = crate::dagonsrc::NewSource::Release {
        git: crate::dagonsrc::REPO_URL.into(),
        tag: crate::dagonsrc::release_tag(),
    };
    new_project(&d.join("a"), "a", SAMPLE, &keys, &release).unwrap();
    let c = Config::load(Some(&d.join("a/dagon.toml").to_string_lossy())).unwrap();
    assert_eq!(c.dagon.git.as_deref(), Some(crate::dagonsrc::REPO_URL));
    assert_eq!(
        c.dagon.tag,
        Some(format!("cli-v{}", env!("CARGO_PKG_VERSION")))
    );
    assert_eq!(c.dagon.path, None);

    let clone =
        crate::dagonsrc::NewSource::from_options(Some(&d.to_string_lossy()), None, None).unwrap();
    new_project(&d.join("b"), "b", SAMPLE, &keys, &clone).unwrap();
    let c = Config::load(Some(&d.join("b/dagon.toml").to_string_lossy())).unwrap();
    assert!(c.dagon.path.is_some() && c.dagon.git.is_none());

    let other = crate::dagonsrc::NewSource::from_options(None, Some("file:///x"), None).unwrap();
    assert_eq!(
        other,
        crate::dagonsrc::NewSource::Release {
            git: "file:///x".into(),
            tag: crate::dagonsrc::release_tag()
        }
    );
    assert!(crate::dagonsrc::NewSource::from_options(Some("."), Some("g"), None).is_err());
}

#[test]
fn new_makes_the_project_and_a_key_outside_it() {
    let d = tmp("new");
    let proj = d.join("fleet");
    let keys = d.join("keys");
    let made = new_project(&proj, "fleet", SAMPLE, &keys, &dev()).unwrap();
    assert!(made.iter().any(|l| l.starts_with("key:")));
    assert!(keys.join(dagon_sign::SECRET_FILE).exists());
    let toml = read(&proj, "dagon.toml");
    assert!(toml.contains("topology = \"topology.yaml\"") && toml.contains("[dagon]"));
    let secret = std::fs::read_to_string(keys.join(dagon_sign::SECRET_FILE)).unwrap();
    assert!(!toml.contains(secret.trim()));
    assert_eq!(read(&proj, "topology.yaml"), SAMPLE);
    assert!(new_project(&proj, "fleet", SAMPLE, &keys, &dev()).is_err());
    let again = new_project(&d.join("fleet2"), "fleet2", SAMPLE, &keys, &dev()).unwrap();
    assert!(!again.iter().any(|l| l.starts_with("key:")));

    let cfg = proj.join("dagon.toml");
    let added =
        add_build_tables(dagon_topology::defs::Registry::builtin(), &cfg, &sample()).unwrap();
    assert_eq!(added, ["hub", "seg", "watch"]);
    let c = Config::load(Some(&cfg.to_string_lossy())).unwrap();
    assert_eq!(c.build["seg"].package, PathBuf::from("nodes/seg"));
    assert_eq!(c.build["seg"].slot_b_feature.as_deref(), Some("slot-b"));
    assert_eq!(
        c.build["watch"].slot_b_feature, None,
        "ESP images serve both slots"
    );
    assert!(
        add_build_tables(dagon_topology::defs::Registry::builtin(), &cfg, &sample())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn new_writes_claude_files_with_no_paths_or_secrets() {
    let d = tmp("claude");
    let proj = d.join("fleet");
    let keys = d.join("keys");
    let made = new_project(&proj, "fleet", SAMPLE, &keys, &dev()).unwrap();
    let secret = std::fs::read_to_string(keys.join(dagon_sign::SECRET_FILE)).unwrap();
    let home = home().unwrap().to_string_lossy().replace('\\', "/");
    let rels = [
        "CLAUDE.md",
        ".claude/skills/fix-a-failing-build/SKILL.md",
        ".claude/skills/add-a-board-by-mcu/SKILL.md",
        ".claude/skills/explain-a-topology-problem/SKILL.md",
    ];
    for rel in rels {
        assert!(made.iter().any(|l| l.ends_with(rel)), "{rel} not reported");
        let text = read(&proj, rel);
        let flat = text.replace('\\', "/");
        for bad in [
            d.to_string_lossy().replace('\\', "/"),
            home.clone(),
            secret.trim().to_string(),
        ] {
            assert!(!flat.contains(&bad), "{rel} contains {bad}");
        }
        let b = flat.as_bytes();
        let drive = b
            .windows(3)
            .any(|w| w[0].is_ascii_alphabetic() && w[1] == b':' && w[2] == b'/');
        assert!(!drive, "{rel} names an absolute path");
        for bad in ["/home/", "/Users/", "~/", "sk-ant-", "PRIVATE KEY", "台"] {
            assert!(!flat.contains(bad), "{rel} contains {bad}");
        }
    }
    for rel in &rels[1..] {
        let name = rel.split('/').nth(2).unwrap();
        assert!(read(&proj, rel).starts_with(&format!("---\nname: {name}\n")));
    }
}

#[test]
fn regen_restores_a_deleted_dagon_dependency() {
    let d = tmp("regen-del");
    let src = clone(&d);
    let base = project(&d);
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    let cargo = base.join("nodes/seg/Cargo.toml");
    let without: String = read(&base, "nodes/seg/Cargo.toml")
        .lines()
        .filter(|l| !l.starts_with("dagon-board-nucleo-f401re"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&cargo, without).unwrap();
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    assert!(r
        .written
        .iter()
        .any(|w| w.path == "nodes/seg/Cargo.toml" && w.action == "rewritten"));
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-board-nucleo-f401re = { path = "));
}

#[test]
fn regen_adds_a_dagon_package_the_template_newly_needs() {
    let d = tmp("regen-new");
    let src = clone(&d);
    let base = project(&d);
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    // The board package's template now also needs the MCU package, in a
    // target table the existing manifest does not have.
    let c = d.join("dagon");
    package(&c, "dagon-mcu-stm32f401re", "", &[]);
    std::fs::write(
        c.join("packages/dagon-board-nucleo-f401re/template/Cargo.toml.tpl"),
        "[package]\nname = \"{{node}}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{{dep:dagon-board-nucleo-f401re}}\n\n[target.'cfg(target_os = \"none\")'.dependencies]\n{{dep:dagon-mcu-stm32f401re}}\n",
    )
    .unwrap();
    // The lock of the old dependencies goes with them.
    std::fs::write(base.join("nodes/seg/Cargo.lock"), "# old\n").unwrap();
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    assert!(!base.join("nodes/seg/Cargo.lock").exists());
    assert!(r
        .written
        .iter()
        .any(|w| w.path == "nodes/seg/Cargo.lock" && w.action == "removed"));
    let m = read(&base, "nodes/seg/Cargo.toml");
    let doc: toml_edit::DocumentMut = m.parse().unwrap();
    assert!(
        doc["dependencies"]
            .get("dagon-board-nucleo-f401re")
            .is_some(),
        "{m}"
    );
    assert!(
        doc["target"]["cfg(target_os = \"none\")"]["dependencies"]
            .get("dagon-mcu-stm32f401re")
            .is_some(),
        "{m}"
    );
}

#[test]
fn topology_names_and_tags_cannot_break_out_of_generated_code() {
    let d = tmp("inject");
    let src = clone(&d);
    let base = project(&d);
    let yaml = SAMPLE
        .replace("name: sample", "name: \"x\\nfn evil() {}\"")
        .replace("tag: seg-f401-7seg", "tag: \"a\\\"); evil(); (\\\"\"");
    let t = dagon_topology::parse_str(&yaml).unwrap();
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t,
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    let wiring = read(&base, "nodes/seg/src/wiring.rs");
    assert!(
        !wiring.lines().any(|l| l.starts_with("fn evil")),
        "{wiring}"
    );
    assert!(
        wiring.starts_with("//! Generated by `dagon gen` from the topology \"x\\nfn evil() {}\".")
    );
    let main = read(&base, "nodes/seg/src/main.rs");
    assert!(main.contains(r#"a\"); evil(); (\""#), "{main}");
}

/// `dagon new` with the sample, `dagon gen`, then `dagon build` of `node`
/// against this DAGON clone's board support packages (real firmware
/// builds: run with --ignored).
fn new_gen_build(node: &str) {
    new_gen_build_from(SAMPLE, node, &format!("e2e-{node}"));
}

/// As [`new_gen_build`] for the sample with no board named: the node is
/// generated from its MCU package's template.
fn new_gen_build_by_mcu(node: &str) {
    let text: String = SAMPLE
        .lines()
        .filter(|l| !l.trim_start().starts_with("board:"))
        .map(|l| {
            format!(
                "{l}
"
            )
        })
        .collect();
    new_gen_build_from(&text, node, &format!("e2e-mcu-{node}"));
}

fn new_gen_build_from(topology: &str, node: &str, name: &str) {
    let d = tmp(name);
    // As deep as a project in a beginner's documents folder can be: slot B
    // (in its own target folder) must still link the host build scripts.
    let dir = d
        .join("documents-of-a-beginner")
        .join("projects-in-a-rather-deep-folder")
        .join("proj");
    new_project(&dir, "proj", topology, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let lines = gen_project(&config, &[node.to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    // Every placeholder rendered ({{uplink}} and {{downlinks}} among them):
    // the build below then checks the paths against the firmware packages.
    let main = std::fs::read_to_string(dir.join("nodes").join(node).join("src/main.rs")).unwrap();
    assert!(!main.contains("{{"), "{main}");
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let mut log = Vec::new();
    let r = crate::nodebuild::build_nodes(
        &cfg,
        &[node.to_string()],
        &mut |l| log.push(l.to_string()),
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert!(r.is_ok(), "{}", log.join("\n"));
    // Both slots where the board has slot B (all but the ESP32-S3, whose
    // second slot is the vendor's).
    let slots = r.unwrap()[0].slots.len();
    assert_eq!(slots, if node == "watch" { 1 } else { 2 });
    assert!(dir.join("nodes").join(node).join("Cargo.lock").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
#[ignore = "builds firmware with the ESP toolchain"]
fn new_gen_build_esp32s3_node() {
    new_gen_build("watch");
}

#[test]
#[ignore = "builds firmware for the RP2040"]
fn new_gen_build_rp2040_hub_by_mcu() {
    new_gen_build_by_mcu("hub");
}

#[test]
#[ignore = "builds firmware for the STM32F401RE"]
fn new_gen_build_f401_node_by_mcu() {
    new_gen_build_by_mcu("seg");
}

#[test]
#[ignore = "builds firmware for the RP2040"]
fn new_gen_build_rp2040_hub() {
    new_gen_build("hub");
}

#[test]
#[ignore = "builds firmware for the STM32F401"]
fn new_gen_build_f401_node() {
    new_gen_build("seg");
}

/// `dagon new` and `dagon gen`, then `node`'s generated boot crate built
/// for every boot core slot its MCU declares and signed for that slot as a
/// release signs it: the signer refuses an ELF whose lowest load address
/// is not the slot's payload address (real firmware builds; CI runs them
/// in the boot-crates job).
fn gen_boot_core_slots_sign(node: &str) {
    use dagon_format::header::{image_kind, slot_binding, ImageHeader, HEADER_SIZE};
    let d = tmp(&format!("e2e-boot-{node}"));
    let dir = d.join("proj");
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    gen_project(&config, &[node.to_string()], false).unwrap();
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let reg = crate::defs::registry(&cfg).unwrap();
    let t = dagon_topology::parse_str(SAMPLE).unwrap();
    let n = t.node(node).unwrap();
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([9; 32]));
    let mut log = Vec::new();
    let built = crate::flash::build_core_slots(
        &cfg,
        &reg,
        n,
        &dagon_sign::to_hex(&key.pk[..]),
        &mut |l| log.push(l.to_string()),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    // As deploy signs a release (deploy::prepare).
    let opts = crate::commands::SignOpts {
        version: dagon_format::Version::new(1, 0, 0),
        release_seq: 1,
        min_bootcore: dagon_format::Version::default(),
        proto: 1,
        bits: None,
    };
    let slots: Vec<char> = built.iter().map(|(s, _)| *s).collect();
    assert_eq!(slots, ['a', 'b'], "{node}");
    for (slot, elf) in &built {
        let binding = if *slot == 'b' {
            slot_binding::B
        } else {
            slot_binding::A
        };
        let img = crate::commands::image_for_node(
            &reg,
            &t,
            node,
            image_kind::BOOTCORE,
            binding,
            elf,
            Some(&opts),
            &key,
        )
        .unwrap_or_else(|e| panic!("{node} slot {slot}: {e}"));
        let h = ImageHeader::parse(img[..HEADER_SIZE].try_into().unwrap()).unwrap();
        let s = dagon_sign::image::slot_with(&reg, &n.mcu, image_kind::BOOTCORE, binding).unwrap();
        assert_eq!(
            (h.load_addr, h.slot_binding),
            (s.load_addr, binding),
            "{node} slot {slot}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
#[ignore = "builds firmware for the STM32F401"]
fn gen_boot_core_slots_sign_f401() {
    gen_boot_core_slots_sign("seg");
}

#[test]
#[ignore = "builds firmware for the RP2040"]
fn gen_boot_core_slots_sign_rp2040() {
    gen_boot_core_slots_sign("hub");
}

#[test]
fn a_new_project_whose_key_cannot_be_made_leaves_nothing_behind() {
    let d = tmp("new-undo");
    // Keys inside a git work tree are refused: key generation fails after
    // the project's files were written.
    std::fs::create_dir_all(d.join("repo/.git")).unwrap();
    let dir = d.join("proj");
    let e = new_project(&dir, "proj", SAMPLE, &d.join("repo/keys"), &dev()).unwrap_err();
    assert!(!dir.exists(), "{e}");
    // An empty folder given by the user stays, empty.
    std::fs::create_dir_all(&dir).unwrap();
    assert!(new_project(&dir, "proj", SAMPLE, &d.join("repo/keys"), &dev()).is_err());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    // The retry with a usable key folder works.
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    assert!(dir.join("dagon.toml").is_file() && dir.join("CLAUDE.md").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

const RENAMED_STALE: &str = "[package]\nname = \"seg\"\nversion = \"0.1.0\"\n\n[dependencies]\nboard = { package = \"dagon-board-nucleo-f401re\", git = \"https://example.invalid/dagon\", tag = \"v0.0.1\" }\n";

#[test]
fn a_renamed_dagon_dependency_with_a_stale_tag_is_refused() {
    let d = tmp("renamed-check");
    std::fs::write(d.join("Cargo.toml"), RENAMED_STALE).unwrap();
    let src = DagonSource {
        git: Some("https://example.invalid/dagon".into()),
        tag: Some("v0.2.0".into()),
        path: None,
    };
    let why = source_mismatch(&d, Path::new("Cargo.toml"), &src)
        .unwrap()
        .expect("stale tag must be refused");
    assert!(why.contains("board") && why.contains("v0.0.1"), "{why}");
}

#[test]
fn gen_refresh_replaces_a_renamed_dagon_dependency() {
    let d = tmp("renamed-gen");
    let src = clone(&d);
    let base = project(&d);
    let nodes = ["seg".to_string()];
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &nodes,
        false,
    )
    .unwrap();
    std::fs::write(base.join("nodes/seg/Cargo.toml"), RENAMED_STALE).unwrap();
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &nodes,
        false,
    )
    .unwrap();
    let m = read(&base, "nodes/seg/Cargo.toml");
    assert!(!m.contains("v0.0.1") && !m.contains("board = {"), "{m}");
    assert!(m.contains("dagon-board-nucleo-f401re = { path = "), "{m}");
}

#[test]
fn hop_placeholders_name_each_nodes_link_kinds() {
    let t = sample();
    let hub = t.node("hub").unwrap();
    let (up, down) = hop_placeholders(dagon_topology::defs::Registry::builtin(), &t, hub).unwrap();
    assert_eq!(up, "dagon_mcu_rp2040::links::Kind::UsbCdc");
    assert_eq!(
        down,
        "dagon_mcu_rp2040::links::Kind::I2c, dagon_mcu_rp2040::links::Kind::Uart"
    );
    let (up, down) = hop_placeholders(
        dagon_topology::defs::Registry::builtin(),
        &t,
        t.node("seg").unwrap(),
    )
    .unwrap();
    assert_eq!(
        (up.as_str(), down.as_str()),
        ("dagon_mcu_stm32f401re::links::Kind::I2c", "")
    );
    let (up, _) = hop_placeholders(
        dagon_topology::defs::Registry::builtin(),
        &t,
        t.node("watch").unwrap(),
    )
    .unwrap();
    assert_eq!(up, "dagon_mcu_esp32s3::links::Kind::Uart");
}

/// An MCU package's template (a node without a board package) takes its
/// link wiring from the 構成 and the MCU definition, never a board's.
#[test]
fn mcu_templates_wire_the_links_the_topology_names() {
    let reg = dagon_topology::defs::Registry::builtin();
    let t = sample();
    let src = DagonSource::default();
    let tpl = |mcu: &str| {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../targets/mcu")
                .join(mcu)
                .join("template/src/board.rs"),
        )
        .unwrap()
    };
    let render_for = |id: &str, text: &str| {
        let n = t.node(id).unwrap();
        let hops = hop_placeholders(reg, &t, n).unwrap();
        render(
            text,
            n,
            &hops,
            &wiring_placeholders(reg, &t, n),
            &src,
            Path::new("."),
        )
        .unwrap()
    };
    let hub = render_for("hub", &tpl("rp2040"));
    assert!(!hub.contains("{{"), "{hub}");
    let bus = t.buses.first().unwrap();
    assert!(hub.contains(&format!("p.PIN_{}", bus.sda)), "{hub}");
    assert!(hub.contains("#[cfg(all())]"), "{hub}");
    let seg = render_for("seg", &tpl("stm32f401re"));
    assert!(!seg.contains("{{"), "{seg}");
    let pins = t.link_to("seg").and_then(|l| l.node_pins.clone()).unwrap();
    assert!(seg.contains(&format!("sda: p.{}", pins.sda)), "{seg}");
    // Its DMA streams come from the MCU definition's request mapping
    // (I2C3: tx stream 4, rx stream 1), not from the template.
    assert!(
        seg.contains("use embassy_stm32::peripherals::{DMA1_CH4, DMA1_CH1};"),
        "{seg}"
    );
    assert!(
        seg.contains("DMA1_STREAM1 => dma::InterruptHandler<peripherals::DMA1_CH1>;"),
        "{seg}"
    );
    assert!(seg.contains("rx_dma: p.DMA1_CH1,"), "{seg}");
    let tpl_text = tpl("stm32f401re");
    assert!(
        !tpl_text.contains("DMA1_CH"),
        "the template names no stream"
    );
    // Without a UART downlink its code is left out and still parses.
    let mut no_uart = t.clone();
    no_uart.links.retain(|l| l.kind != LinkKind::Uart);
    let n = no_uart.node("hub").unwrap();
    let w = wiring_placeholders(reg, &no_uart, n).unwrap();
    let get = |k: &str| w.iter().find(|(p, _)| p == k).unwrap().1.clone();
    assert_eq!(get("{{if_down_uart}}"), "any()");
    assert_eq!(get("{{down_uart_tx}}"), "none");
    // Two I2C buses and two UART links: refused by name, never one picked.
    let mut many = t.clone();
    let mut bus2 = many.buses[0].clone();
    bus2.id = "bus-2".into();
    many.buses.push(bus2);
    let i2c = many
        .links
        .iter()
        .find(|l| l.kind == LinkKind::I2c)
        .unwrap()
        .clone();
    let uart = many
        .links
        .iter()
        .find(|l| l.kind == LinkKind::Uart)
        .unwrap()
        .clone();
    let mut i2c2 = i2c.clone();
    i2c2.to = "second".into();
    i2c2.bus = Some("bus-2".into());
    let mut uart2 = uart.clone();
    uart2.to = "third".into();
    many.links.extend([i2c2, uart2.clone()]);
    let hub = many.node("hub").unwrap().clone();
    let e = wiring_placeholders(reg, &many, &hub).unwrap_err();
    assert!(
        e.contains("one I2C bus") && e.contains("bus-2") && e.contains("hub -> second"),
        "{e}"
    );
    many.links.retain(|l| l.to != "second");
    let n = &hub;
    let e = wiring_placeholders(reg, &many, n).unwrap_err();
    assert!(
        e.contains("one UART link") && e.contains("hub -> third"),
        "{e}"
    );
    // A template that does not use the wiring is rendered all the same.
    let hops = hop_placeholders(reg, &many, n).unwrap();
    let w = wiring_placeholders(reg, &many, n);
    assert!(render("plain {{node}}", n, &hops, &w, &src, Path::new(".")).is_ok());
    assert!(render("{{down_i2c}}", n, &hops, &w, &src, Path::new(".")).is_err());
    // No template of an MCU package names a board fact of its own.
    for mcu in ["rp2040", "stm32f401re"] {
        for fact in [
            "PA5",
            "PIN_25",
            "512 * 1024",
            "4 * 1024 * 1024",
            "PB4",
            "PIN_16",
        ] {
            assert!(!tpl(mcu).contains(fact), "{mcu}: {fact}");
        }
    }
}

/// The firmware's generator contract (targets/mcu/<mcu>/README.md, "Links"):
/// `{{uplink}}` / `{{downlinks}}` are `dagon_mcu_<mcu>::links::Kind::<Variant>`
/// (both enums come from `dagon_format::with_link_kinds!`), and the kinds
/// each MCU offers up and down (its table's "Yes" cells).
const FW_OFFERS: [(&str, &[&str], &[&str]); 3] = [
    ("rp2040", &["usb-cdc"], &["uart", "i2c"]),
    ("stm32f401re", &["i2c"], &[]),
    ("esp32s3", &["usb-cdc", "uart"], &[]),
];

#[test]
fn the_hop_paths_are_exactly_the_firmware_links_contract() {
    use dagon_topology::links::{available, Dir};
    assert_eq!(kind_variant(LinkKind::WifiTcp), "WifiTcp");
    for (mcu, up, down) in FW_OFFERS {
        let names = |d: Dir| -> Vec<&'static str> {
            let mut v: Vec<&'static str> = available(mcu, d).iter().map(|k| k.name()).collect();
            v.sort_unstable();
            v
        };
        let (mut up, mut down) = (up.to_vec(), down.to_vec());
        up.sort_unstable();
        down.sort_unstable();
        assert_eq!(names(Dir::Up), up, "{mcu} up");
        assert_eq!(names(Dir::Down), down, "{mcu} down");
    }
    // Every node of the sample, rendered: the exact paths of the contract.
    let t = sample();
    for (id, up, down) in [
        (
            "hub",
            "dagon_mcu_rp2040::links::Kind::UsbCdc",
            "dagon_mcu_rp2040::links::Kind::I2c, dagon_mcu_rp2040::links::Kind::Uart",
        ),
        ("seg", "dagon_mcu_stm32f401re::links::Kind::I2c", ""),
        ("watch", "dagon_mcu_esp32s3::links::Kind::Uart", ""),
    ] {
        let got = hop_placeholders(
            dagon_topology::defs::Registry::builtin(),
            &t,
            t.node(id).unwrap(),
        )
        .unwrap();
        assert_eq!((got.0.as_str(), got.1.as_str()), (up, down), "{id}");
    }
}

#[test]
fn templates_get_the_hops_and_gen_refuses_one_not_available_yet() {
    let d = tmp("hops");
    let c = d.join("dagon");
    let tpl: &[(&str, &str)] = &[
        (
            "Cargo.toml.tpl",
            "[package]\nname = \"{{node}}\"\n[dependencies]\n{{dep:dagon-board-nucleo-f401re}}\n",
        ),
        (
            "src/main.rs",
            "const UPLINK: Kind = {{uplink}};\nconst DOWNLINKS: &[Kind] = &[{{downlinks}}];\n",
        ),
    ];
    package(&c, "dagon-board-nucleo-f401re", "", tpl);
    let src = DagonSource {
        path: Some(c),
        ..DagonSource::default()
    };
    let base = project(&d);
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap();
    assert_eq!(
        read(&base, "nodes/seg/src/main.rs"),
        "const UPLINK: Kind = dagon_mcu_stm32f401re::links::Kind::I2c;\nconst DOWNLINKS: &[Kind] = &[];\n"
    );
    // The 親機 over BLE: gen stops, naming the hop.
    let ble = dagon_topology::parse_str(&SAMPLE.replacen(
        "    tag: hub-rp2040-lcd\n",
        "    tag: hub-rp2040-lcd\n    uplink: { kind: ble, host: browser }\n",
        1,
    ))
    .unwrap();
    let e = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &ble,
        &src,
        &BTreeMap::new(),
        &["hub".into()],
        false,
    )
    .unwrap_err();
    assert!(
        e.contains("uplink of hub over ble: not available yet on rp2040"),
        "{e}"
    );
    // A link a 子機 cannot do: named by its two ends.
    let wifi = dagon_topology::parse_str(&SAMPLE.replacen(
        "    kind: i2c\n    bus: ",
        "    kind: wifi-tcp\n    bus: ",
        1,
    ))
    .unwrap();
    let e = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &wifi,
        &src,
        &BTreeMap::new(),
        &["seg".into()],
        false,
    )
    .unwrap_err();
    assert!(
        e.contains("link hub -> seg over wifi-tcp: not available yet"),
        "{e}"
    );
}

#[test]
fn write_refuses_a_symlink_at_the_final_path() {
    let base = std::env::temp_dir().join(format!("dagon-write-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("nodes/n")).unwrap();
    std::fs::write(
        base.join("user.rs"),
        "mine
",
    )
    .unwrap();
    #[cfg(unix)]
    let made =
        std::os::unix::fs::symlink(base.join("user.rs"), base.join("nodes/n/wiring.rs")).is_ok();
    // Windows creates file symlinks only with the privilege (or developer mode).
    #[cfg(windows)]
    let made =
        std::os::windows::fs::symlink_file(base.join("user.rs"), base.join("nodes/n/wiring.rs"))
            .is_ok();
    if made {
        let mut out = Vec::new();
        let e = super::write(
            &base,
            "nodes/n/wiring.rs",
            "generated
",
            true,
            &mut out,
        )
        .unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        assert!(out.is_empty());
        assert_eq!(
            std::fs::read_to_string(base.join("user.rs")).unwrap(),
            "mine
"
        );
        assert!(std::fs::symlink_metadata(base.join("nodes/n/wiring.rs"))
            .unwrap()
            .file_type()
            .is_symlink());
    } else {
        eprintln!("symlinks are not allowed here: the symlink case did not run");
    }
    // A junction (Windows: no privilege needed) at the final path is a link too.
    #[cfg(windows)]
    {
        std::fs::create_dir_all(base.join("src")).unwrap();
        let p = |r: &str| base.join(r).components().collect::<std::path::PathBuf>();
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(p("nodes/n/main.rs"))
            .arg(p("src"))
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J");
        let mut out = Vec::new();
        let e = super::write(
            &base,
            "nodes/n/main.rs",
            "generated
",
            true,
            &mut out,
        )
        .unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        assert_eq!(std::fs::read_dir(base.join("src")).unwrap().count(), 0);
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn gen_skips_a_node_folder_reached_through_a_junction() {
    if !cfg!(windows) {
        return; // the link cases themselves are in genfs
    }
    let d = tmp("node-junction");
    let src = clone(&d);
    let base = project(&d);
    std::fs::create_dir_all(base.join("elsewhere")).unwrap();
    std::fs::write(
        base.join("elsewhere/Cargo.lock"),
        "user
",
    )
    .unwrap();
    std::fs::create_dir_all(base.join("nodes")).unwrap();
    let p = |r: &str| base.join(r).components().collect::<PathBuf>();
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(p("nodes/watch"))
        .arg(p("elsewhere"))
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "mklink /J");
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &sample(),
        &src,
        &BTreeMap::new(),
        &["watch".into()],
        false,
    )
    .unwrap();
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert!(
        r.skipped[0].1.contains("nodes/watch is a link"),
        "{:?}",
        r.skipped
    );
    assert!(r.written.is_empty());
    assert_eq!(
        std::fs::read_dir(base.join("elsewhere")).unwrap().count(),
        1
    );
    // gen ends non-zero, naming the node and the reason.
    let lines = vec![format!("skipped    nodes/watch: {}", r.skipped[0].1)];
    assert!(super::outcome(&lines).unwrap_err().contains("is a link"));
}
