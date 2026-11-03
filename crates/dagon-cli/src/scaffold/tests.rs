//! Projects generated into temporary folders, with a stand-in DAGON clone
//! holding packages named and declared like the board support packages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

const DESK: &str = include_str!("../../../../topology/desk-demo.yaml");

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
    package(&c, "dagon-board-m5stopwatch", "", &[]);
    package(&c, "dagon-board-nucleo-f401re", "", F401_TEMPLATE);
    DagonSource {
        path: Some(c),
        ..DagonSource::default()
    }
}

fn read(base: &Path, rel: &str) -> String {
    std::fs::read_to_string(base.join(rel)).unwrap()
}

fn desk() -> Topology {
    dagon_topology::parse_str(DESK).unwrap()
}

fn project(d: &Path) -> PathBuf {
    let base = d.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn desk_demo_uses_each_packages_template_and_skips_what_is_missing() {
    let d = tmp("desk");
    let src = clone(&d);
    let base = project(&d);
    let r = gen(&base, &desk(), &src, &BTreeMap::new(), &[], false).unwrap();

    // The hub's board package does not exist yet: skipped, with the reason.
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert_eq!(r.skipped[0].0, "hub");
    assert!(r.skipped[0]
        .1
        .contains("dagon-board-waveshare-rp2040-touch-lcd-1-28"));
    assert!(!base.join("nodes/hub").exists());

    // ESP32-S3: the template built into the CLI, on the real package API.
    let watch = read(&base, "nodes/watch/Cargo.toml");
    assert!(watch.contains("name = \"watch\""));
    assert!(watch.contains("dagon-board-m5stopwatch = { path = "));
    assert!(watch.contains("dagon-mcu-esp32s3 = { path = "));
    let main = read(&base, "nodes/watch/src/main.rs");
    assert!(main.contains("board::start(") && main.contains("NodeConfig::app(app::self_check)"));
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
    gen(&base, &desk(), &src, &BTreeMap::new(), &[], false).unwrap();

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

    let t2 = dagon_topology::parse_str(&DESK.replace("baud: 115200", "baud: 230400")).unwrap();
    let r = gen(&base, &t2, &src, &BTreeMap::new(), &["watch".into()], false).unwrap();
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

    let r = gen(&base, &t2, &src, &BTreeMap::new(), &["watch".into()], true).unwrap();
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
    let yaml = DESK.replace("    board: nucleo-f401re\n", "");
    let t = dagon_topology::parse_str(&yaml).unwrap();
    let mut boards = BTreeMap::new();
    boards.insert(
        "seg".to_string(),
        BoardFacts {
            flash_bytes: Some(512 * 1024),
        },
    );
    let base = project(&d);
    let r = gen(&base, &t, &src, &boards, &["seg".into()], false).unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-mcu-stm32f401 = { path = "));
    assert!(read(&base, "nodes/seg/src/wiring.rs").contains("flash_bytes: Some(524288)"));
}

#[test]
fn a_release_source_pins_the_git_tag_and_reads_its_packages_from_cargos_checkout() {
    let d = tmp("git");
    // A stand-in release repository, tagged, served from a local file URL.
    let c = d.join("release");
    package(&c, "dagon-board-m5stopwatch", "", &[]);
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
        &base,
        &desk(),
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
        &base,
        &desk(),
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
    new_project(&d.join("a"), "a", DESK, &keys, &release).unwrap();
    let c = Config::load(Some(&d.join("a/dagon.toml").to_string_lossy())).unwrap();
    assert_eq!(c.dagon.git.as_deref(), Some(crate::dagonsrc::REPO_URL));
    assert_eq!(c.dagon.tag, Some(format!("v{}", env!("CARGO_PKG_VERSION"))));
    assert_eq!(c.dagon.path, None);

    let clone =
        crate::dagonsrc::NewSource::from_options(Some(&d.to_string_lossy()), None, None).unwrap();
    new_project(&d.join("b"), "b", DESK, &keys, &clone).unwrap();
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
    let made = new_project(&proj, "fleet", DESK, &keys, &dev()).unwrap();
    assert!(made.iter().any(|l| l.starts_with("key:")));
    assert!(keys.join(dagon_sign::SECRET_FILE).exists());
    let toml = read(&proj, "dagon.toml");
    assert!(toml.contains("topology = \"topology.yaml\"") && toml.contains("[dagon]"));
    let secret = std::fs::read_to_string(keys.join(dagon_sign::SECRET_FILE)).unwrap();
    assert!(!toml.contains(secret.trim()));
    assert_eq!(read(&proj, "topology.yaml"), DESK);
    assert!(new_project(&proj, "fleet", DESK, &keys, &dev()).is_err());
    let again = new_project(&d.join("fleet2"), "fleet2", DESK, &keys, &dev()).unwrap();
    assert!(!again.iter().any(|l| l.starts_with("key:")));

    let cfg = proj.join("dagon.toml");
    let added = add_build_tables(&cfg, &desk()).unwrap();
    assert_eq!(added, ["hub", "seg", "watch"]);
    let c = Config::load(Some(&cfg.to_string_lossy())).unwrap();
    assert_eq!(c.build["seg"].package, PathBuf::from("nodes/seg"));
    assert_eq!(c.build["seg"].slot_b_feature.as_deref(), Some("slot-b"));
    assert_eq!(
        c.build["watch"].slot_b_feature, None,
        "ESP images serve both slots"
    );
    assert!(add_build_tables(&cfg, &desk()).unwrap().is_empty());
}

#[test]
fn new_writes_claude_files_with_no_paths_or_secrets() {
    let d = tmp("claude");
    let proj = d.join("fleet");
    let keys = d.join("keys");
    let made = new_project(&proj, "fleet", DESK, &keys, &dev()).unwrap();
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
        &base,
        &desk(),
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
        &base,
        &desk(),
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
        &base,
        &desk(),
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
        &base,
        &desk(),
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
    let yaml = DESK
        .replace("name: desk-demo", "name: \"x\\nfn evil() {}\"")
        .replace("tag: seg-f401-7seg", "tag: \"a\\\"); evil(); (\\\"\"");
    let t = dagon_topology::parse_str(&yaml).unwrap();
    gen(&base, &t, &src, &BTreeMap::new(), &["seg".into()], false).unwrap();
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
    let d = tmp(&format!("e2e-{node}"));
    // As deep as a project in a beginner's documents folder can be: slot B
    // (in its own target folder) must still link the host build scripts.
    let dir = d
        .join("documents-of-a-beginner")
        .join("projects-in-a-rather-deep-folder")
        .join("proj");
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let lines = gen_project(&config, &[node.to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
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
fn new_gen_build_rp2040_hub() {
    new_gen_build("hub");
}

#[test]
#[ignore = "builds firmware for the STM32F401"]
fn new_gen_build_f401_node() {
    new_gen_build("seg");
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
    gen(&base, &desk(), &src, &BTreeMap::new(), &nodes, false).unwrap();
    std::fs::write(base.join("nodes/seg/Cargo.toml"), RENAMED_STALE).unwrap();
    gen(&base, &desk(), &src, &BTreeMap::new(), &nodes, false).unwrap();
    let m = read(&base, "nodes/seg/Cargo.toml");
    assert!(!m.contains("v0.0.1") && !m.contains("board = {"), "{m}");
    assert!(m.contains("dagon-board-nucleo-f401re = { path = "), "{m}");
}
