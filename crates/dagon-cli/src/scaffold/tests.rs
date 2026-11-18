//! Projects generated into temporary folders, with a stand-in DAGON clone
//! holding packages named and declared like the board support packages.

use std::path::{Path, PathBuf};

use super::*;

/// The source of a DAGON developer's project: this clone.
fn dev() -> crate::dagonsrc::NewSource {
    let here = std::fs::canonicalize(crate::dagonsrc::BUILT_FROM).unwrap();
    crate::dagonsrc::NewSource::Clone(crate::config::plain_path(here))
}

pub(super) fn tmp(name: &str) -> PathBuf {
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
fn gen_writes_each_nodes_index_in_the_compiled_topology() {
    // Two nodes run images with the same tag: only the index tells them apart.
    let d = tmp("index");
    let src = clone(&d);
    let base = project(&d);
    let t =
        dagon_topology::parse_str(&SAMPLE.replace("tag: watch-s3", "tag: seg-f401-7seg")).unwrap();
    gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t,
        &src,
        &[],
        false,
    )
    .unwrap();
    let bin = dagon_topology::compile(&t).unwrap();
    let view = dagon_format::topology::TopologyView::parse(&bin).unwrap();
    let index_of = |id: &str| -> u8 {
        let wiring = read(&base, &format!("nodes/{id}/src/wiring.rs"));
        let index = wiring
            .lines()
            .find_map(|l| l.strip_prefix("    index: ")?.strip_suffix(','));
        index.unwrap_or_else(|| panic!("{wiring}")).parse().unwrap()
    };
    let (seg, watch) = (index_of("seg"), index_of("watch"));
    assert_eq!((seg, watch), (1, 2));
    // The compiled topology's links name the nodes by these indexes: the
    // I2C link to seg and the UART link to watch.
    let ends: Vec<(u8, u8)> = (0..view.link_count())
        .flat_map(|k| view.link(k).ends.map(|e| (e.node, view.link(k).iface)))
        .collect();
    assert!(ends.contains(&(seg, dagon_format::topology::iface::I2C_BUS)));
    assert!(ends.contains(&(watch, dagon_format::topology::iface::PIO_UART)));
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
    let base = project(&d);
    let r = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &t,
        &src,
        &["seg".into()],
        false,
    )
    .unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(read(&base, "nodes/seg/Cargo.toml").contains("dagon-mcu-stm32f401 = { path = "));
    assert!(read(&base, "nodes/seg/src/wiring.rs").contains("flash_bytes: None,"));
}

/// The text of every fenced block of `lang` in a Markdown page.
fn fences(md: &str, lang: &str) -> Vec<String> {
    let open = format!("```{lang}");
    let mut out = Vec::new();
    let mut lines = md.lines();
    while let Some(l) = lines.next() {
        if l.trim_end() == open {
            let body: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "```").collect();
            out.push(body.join("\n") + "\n");
        }
    }
    out
}

/// The sample with node `id`'s entry replaced by `entry` (a `- id:` item).
fn with_node(id: &str, entry: &str) -> String {
    let head = format!("  - id: {id}");
    let mut out = String::new();
    let mut skipping = false;
    for l in SAMPLE.lines() {
        if l == head {
            skipping = true;
            out.push_str(entry);
            continue;
        }
        if skipping && (l.starts_with("  - ") || !l.starts_with("    ")) {
            skipping = false;
        }
        if !skipping {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// A new project from the sample, with node `id`'s entry replaced by
/// `entry` and `extra` added to its dagon.toml; `dagon gen` of that node.
fn gen_with(name: &str, id: &str, entry: &str, extra: &str) -> (PathBuf, Vec<String>) {
    let d = tmp(name);
    let dir = d.join("proj");
    new_project(&dir, "proj", &with_node(id, entry), &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let toml = read(&dir, crate::config::CONFIG_FILE) + extra;
    std::fs::write(&config, toml).unwrap();
    let lines = gen_project(&config, &[id.to_string()], false).unwrap_or_else(|e| panic!("{e}"));
    (dir, lines)
}

/// `dagon gen` gives the hub the definition, as the project resolves it,
/// of the node its I2C ROM link reaches, whether or not the hub's end names
/// the reset and boot lines (its board wires them): the relay's build takes
/// that loader's address. The host takes the ROM path to the same node.
#[test]
fn gen_gives_the_relay_its_rom_link_peer_with_or_without_named_lines() {
    use dagon_topology::defs::boot_folder::recovery_rom_addr_rs;
    let moved = "\n[mcu_override.stm32f401re.rom_i2c]\naddr = 0x3B\n";
    let without = SAMPLE.replace(", nrst: 18, boot: 28", "");
    for (name, yaml) in [("rec-lines", SAMPLE), ("rec-nolines", without.as_str())] {
        let d = tmp(name);
        let dir = d.join("proj");
        new_project(&dir, "proj", yaml, &d.join("keys"), &dev()).unwrap();
        let config = dir.join(crate::config::CONFIG_FILE);
        let toml = read(&dir, crate::config::CONFIG_FILE) + moved;
        std::fs::write(&config, toml).unwrap();
        gen_project(&config, &["hub".to_string()], false).unwrap_or_else(|e| panic!("{e}"));
        let rs = recovery_rom_addr_rs(&dir.join("boot/hub"));
        assert!(
            rs.contains("pub const RECOVERY_ROM_ADDR: Option<u8> = Some(0x3b);\n"),
            "{name}: {rs}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[test]
fn the_own_board_guide_runs_through_gen() {
    let pages = [
        (
            "how-to/own-board.md",
            include_str!("../../../../site/src/content/docs/how-to/own-board.md"),
        ),
        (
            "en/how-to/own-board.md",
            include_str!("../../../../site/src/content/docs/en/how-to/own-board.md"),
        ),
    ];
    for (page, md) in pages {
        let (yaml, toml) = (fences(md, "yaml"), fences(md, "toml"));
        assert_eq!((yaml.len(), toml.len()), (1, 1), "{page}");
        let entry: yaml_serde::Value = yaml_serde::from_str(&yaml[0]).unwrap();
        let node = &entry[0];
        let id = node["id"].as_str().unwrap();
        let board = node["board"]
            .as_str()
            .unwrap_or_else(|| panic!("{page}: the node names no board"));
        let defs: toml::Table = toml::from_str(&toml[0]).unwrap();
        let flash = defs["board"][board]["flash_bytes"].as_integer().unwrap();

        let (dir, lines) = gen_with(&format!("guide-{id}"), id, &yaml[0], &toml[0]);
        assert!(
            !lines.iter().any(|l| l.starts_with("skipped")),
            "{page}: {lines:?}"
        );
        let wiring = read(&dir, &format!("nodes/{id}/src/wiring.rs"));
        assert!(
            wiring.contains(&format!("flash_bytes: Some({flash}),")),
            "{page}: {wiring}"
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}

#[test]
fn a_node_chosen_by_mcu_is_told_to_name_a_board_definition() {
    let entry = "  - id: seg\n    mcu: stm32f401re\n    role: node\n    port: self-boot\n    \
                 flash_map: f401-default\n    tag: seg-f401-7seg\n";
    let (dir, lines) = gen_with("by-mcu-note", "seg", entry, "");
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    assert!(read(&dir, "nodes/seg/src/wiring.rs").contains("flash_bytes: None,"));
    assert!(
        lines.contains(
            &"note: seg is chosen by MCU; for its flash size, define its board in \
              [board.<board id>] of dagon.toml and name it in the node's board:"
                .to_string()
        ),
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
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
fn an_overridden_builtin_board_keeps_its_board_package() {
    let d = tmp("override-board");
    let src = clone(&d);
    let base = project(&d);
    std::fs::write(
        base.join("dagon.toml"),
        "format = 1\n[board_override.nucleo-f401re.boot]\nhalt = { pattern = \"code\" }\n",
    )
    .unwrap();
    let reg = dagon_topology::defs::Registry::load(&[dagon_topology::defs::Source::Project(
        &base.join("dagon.toml"),
    )])
    .unwrap();
    // The merged board is the project's text, and still the built-in package's.
    let seg = sample().nodes.into_iter().find(|n| n.id == "seg").unwrap();
    assert_eq!(want_of(&reg, &seg), Want::Board("nucleo-f401re".into()));
    gen(&reg, &base, &sample(), &src, &["seg".into()], false).unwrap();
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

/// wiring.rs carries the clock the app runs on: the MCU's `[[app.clock]]`
/// setup the node's board picks (internal without a board or crystal); an
/// MCU without a DAGON boot loader keeps its HAL's clock and gets none.
#[test]
fn wiring_names_the_clock_the_board_picks_for_the_app() {
    let d = tmp("wiring-clock");
    let src = clone(&d);
    let base = project(&d);
    let reg = dagon_topology::defs::Registry::builtin();
    gen(
        reg,
        &base,
        &sample(),
        &src,
        &["seg".into(), "watch".into()],
        false,
    )
    .unwrap();
    let seg = read(&base, "nodes/seg/src/wiring.rs");
    for want in [
        "pub mod clock {\n",
        "pub const SOURCE: &str = \"internal\";\n",
        "pub const IN_HZ: Option<u32> = Some(16000000);\n",
        "pub const OUT_HZ: u32 = 84000000;\n",
    ] {
        assert!(seg.contains(want), "{want} in {seg}");
    }
    assert!(!read(&base, "nodes/watch/src/wiring.rs").contains("pub mod clock"));
    // The hub on its board's crystal, and the same hub without a board.
    let t = sample();
    let hub = t.nodes.iter().position(|n| n.id == "hub").unwrap();
    let m = reg.mcu("rp2040").unwrap();
    let on = |board| {
        m.clock_for(dagon_topology::defs::ClockStage::App, board)
            .unwrap()
    };
    let ws = reg.board("waveshare-rp2040-touch-lcd-1.28");
    let xtal = wiring_rs(&t, hub, None, &[], Some(on(ws)));
    assert!(
        xtal.contains("pub const SOURCE: &str = \"crystal\";\n"),
        "{xtal}"
    );
    assert!(xtal.contains("(\"usb_fbdiv\", 120)"), "{xtal}");
    let none = wiring_rs(&t, hub, None, &[], Some(on(None)));
    assert!(
        none.contains("pub const SOURCE: &str = \"internal\";\n"),
        "{none}"
    );
    assert!(
        none.contains("pub const PLL: &[(&str, u32)] = &[];\n"),
        "{none}"
    );
    // A project's override of the MCU's [[app.clock]] reaches wiring.rs.
    let toml = d.join("override.toml");
    std::fs::write(
        &toml,
        "format = 1\n[[mcu_override.stm32f401re.app.clock]]\nsource = \"internal\"\nout_hz = 16_000_000\n",
    )
    .unwrap();
    let over =
        dagon_topology::defs::Registry::load(&[dagon_topology::defs::Source::Project(&toml)])
            .unwrap();
    gen(&over, &base, &sample(), &src, &["seg".into()], false).unwrap();
    let seg = read(&base, "nodes/seg/src/wiring.rs");
    assert!(seg.contains("pub const OUT_HZ: u32 = 16000000;\n"), "{seg}");
    assert!(
        seg.contains("pub const IN_HZ: Option<u32> = None;\n"),
        "{seg}"
    );
    let _ = std::fs::remove_dir_all(&d);
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

/// The sample with no board named: its nodes are generated from their MCU
/// packages' templates and run on their MCUs' internal oscillators.
fn sample_by_mcu() -> String {
    SAMPLE
        .lines()
        .filter(|l| !l.trim_start().starts_with("board:"))
        .map(|l| {
            format!(
                "{l}
"
            )
        })
        .collect()
}

/// `topology` (the sample's shape) without the watch and its UART link:
/// the hub and seg over I2C. A UART link needs a clock with a known
/// accuracy on each end (V-26), which a node without a board's crystal
/// does not have.
fn without_uart(topology: &str) -> String {
    let cut = |s: &str, from: &str, to: &str| {
        let a = s.find(from).unwrap_or_else(|| panic!("no {from:?}"));
        let b = a + s[a..].find(to).unwrap_or_else(|| panic!("no {to:?}")) + to.len();
        format!("{}{}", &s[..a], &s[b..])
    };
    let s = cut(topology, "  - id: watch\n", "    tag: watch-s3\n");
    let s = cut(
        &s,
        "  - kind: uart\n",
        "      rom_entry: force-download-boot\n",
    );
    let s = cut(&s, "  watch: [", "]\n");
    s.replacen("order: [seg, watch, hub]", "order: [seg, hub]", 1)
}

/// As [`new_gen_build`] for the sample with no board named, without its
/// UART link: the node is generated from its MCU package's template.
fn new_gen_build_by_mcu(node: &str) {
    new_gen_build_from(
        &without_uart(&sample_by_mcu()),
        node,
        &format!("e2e-mcu-{node}"),
    );
}

fn new_gen_build_from(topology: &str, node: &str, name: &str) {
    if let Err(log) = build_project(topology, node, name, "") {
        panic!("{log}");
    }
}

/// `dagon new` with `topology` (and `config_extra` added to its
/// dagon.toml), `dagon gen`, then `dagon build` of `node`; the build's log
/// when it fails.
fn build_project(topology: &str, node: &str, name: &str, config_extra: &str) -> Result<(), String> {
    let d = tmp(name);
    // As deep as a project in a beginner's documents folder can be: slot B
    // (in its own target folder) must still link the host build scripts.
    let dir = d
        .join("documents-of-a-beginner")
        .join("projects-in-a-rather-deep-folder")
        .join("proj");
    new_project(&dir, "proj", topology, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("{text}\n{config_extra}")).unwrap();
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
    let Ok(built) = r else {
        let _ = std::fs::remove_dir_all(&d);
        return Err(log.join("\n"));
    };
    // Both slots where the board has slot B (all but the ESP32-S3, whose
    // second slot is the vendor's).
    assert_eq!(built[0].slots.len(), if node == "watch" { 1 } else { 2 });
    assert!(dir.join("nodes").join(node).join("Cargo.lock").is_file());
    let _ = std::fs::remove_dir_all(&d);
    Ok(())
}

#[test]
#[ignore = "builds firmware with the ESP toolchain"]
fn new_gen_build_esp32s3_node() {
    new_gen_build("watch");
}

/// The hub from the RP2040's template: without a board it states no
/// crystal, so its app has no USB clock and the build stops naming it; on
/// a board (defined in dagon.toml) that states its 12 MHz crystal it
/// builds.
#[test]
#[ignore = "builds firmware for the RP2040"]
fn new_gen_build_rp2040_hub_by_mcu() {
    let log = build_project(&without_uart(&sample_by_mcu()), "hub", "e2e-mcu-hub", "").unwrap_err();
    assert!(
        log.contains("node hub: its USB uplink needs the 48 MHz USB clock"),
        "{log}"
    );
    // That is the only error: the rest of the template builds.
    assert_eq!(
        log.matches("error[E").count(),
        log.matches("error[E0080]").count(),
        "{log}"
    );
    let on_board = sample_by_mcu().replacen(
        "    mcu: rp2040\n",
        "    mcu: rp2040\n    board: rp2040-with-crystal\n",
        1,
    );
    let board = "[board.rp2040-with-crystal]
format = 3
product = \"An RP2040 board with a 12 MHz crystal\"
mcu = \"rp2040\"
flash_bytes = 0x40_0000
drawing = \"generic\"
header = [{ pin = \"GPIO16\" }, { pin = \"GPIO17\" }, { pin = \"GPIO18\" }, { pin = \"GPIO26\" }, { pin = \"GPIO27\" }, { pin = \"GPIO28\" }]
i2c = [{ controller = \"i2c0\", sda = \"GPIO16\", scl = \"GPIO17\" }]
clock = { source = { crystal_hz = 12_000_000 } }
";
    if let Err(log) = build_project(&on_board, "hub", "e2e-mcu-hub-xtal", board) {
        panic!("{log}");
    }
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
/// in the boot-crates job). With `one_core`, its MCU definition keeps boot
/// core slot a only: slot a alone is built and signed, and slot b is not
/// signed.
fn gen_boot_core_slots_sign(node: &str, one_core: bool) {
    gen_boot_core_slots_sign_from(SAMPLE, node, one_core);
}

/// [`gen_boot_core_slots_sign`] for the project of `topology`.
fn gen_boot_core_slots_sign_from(topology: &str, node: &str, one_core: bool) {
    use dagon_format::header::{image_kind, slot_binding, ImageHeader, HEADER_SIZE};
    let by_mcu = if topology == SAMPLE { "" } else { "-mcu" };
    let d = tmp(&format!("e2e-boot-{node}-{one_core}{by_mcu}"));
    let dir = d.join("proj");
    new_project(&dir, "proj", topology, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    gen_project(&config, &[node.to_string()], false).unwrap();
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let t = dagon_topology::parse_str(topology).unwrap();
    let n = t.node(node).unwrap();
    let reg = if one_core {
        super::tests_slots::with_single(&n.mcu, &["bootcore"]).unwrap()
    } else {
        crate::defs::registry(&cfg).unwrap()
    };
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
    let want: &[char] = if one_core { &['a'] } else { &['a', 'b'] };
    assert_eq!(slots, want, "{node}");
    if one_core {
        let e = crate::commands::image_for_node(
            &reg,
            &t,
            node,
            image_kind::BOOTCORE,
            slot_binding::B,
            &built[0].1,
            Some(&opts),
            &key,
        )
        .unwrap_err();
        assert!(e.contains("has no such slot"), "{e}");
    }
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
    gen_boot_core_slots_sign("seg", false);
}

#[test]
#[ignore = "builds firmware for the STM32F401"]
fn gen_boot_core_slots_sign_f401_one_core() {
    gen_boot_core_slots_sign("seg", true);
}

#[test]
#[ignore = "builds firmware for the RP2040"]
fn gen_boot_core_slots_sign_rp2040() {
    gen_boot_core_slots_sign("hub", false);
}

#[test]
#[ignore = "builds firmware for the RP2040"]
fn gen_boot_core_slots_sign_rp2040_one_core() {
    gen_boot_core_slots_sign("hub", true);
}

/// The RP2040 boot loader of a node whose board states no crystal (no
/// board at all): it builds on the ring oscillator setup, for every boot
/// core slot.
#[test]
#[ignore = "builds firmware for the RP2040"]
fn gen_boot_core_slots_sign_rp2040_without_a_crystal() {
    gen_boot_core_slots_sign_from(&without_uart(&sample_by_mcu()), "hub", false);
}

/// The RP2040 slots a project moves by overriding the built-in definition
/// (each slot elsewhere, the app slots smaller).
const MOVED_RP2040_SLOTS: &str = "
[[mcu_override.rp2040.slot]]
image = \"bootcore\"
binding = \"a\"
at = 0x1001_0000

[[mcu_override.rp2040.slot]]
image = \"bootcore\"
binding = \"b\"
at = 0x1003_0000

[[mcu_override.rp2040.slot]]
image = \"app\"
binding = \"a\"
at = 0x1005_0000
size = 0xC_0000

[[mcu_override.rp2040.slot]]
image = \"app\"
binding = \"b\"
at = 0x1011_0000
size = 0x8_0000

[[mcu_override.rp2040.slot_layout]]
target = \"bootcore\"
slot = \"a\"
start = 0x1001_0000

[[mcu_override.rp2040.slot_layout]]
target = \"bootcore\"
slot = \"b\"
start = 0x1003_0000

[[mcu_override.rp2040.slot_layout]]
target = \"app\"
slot = \"a\"
start = 0x1005_0000
capacity = 0xB_FE00

[[mcu_override.rp2040.slot_layout]]
target = \"app\"
slot = \"b\"
start = 0x1011_0000
capacity = 0x7_FE00

[mcu_override.rp2040.watchdog]
trial_ms = 6_000
confirmed_ms = 8_000
";

/// The constants each `dagon-mcu-*` build generated below `target`
/// (`definition.rs` in its `OUT_DIR`).
fn generated_definitions(target: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![target.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.file_name().is_some_and(|n| n == "definition.rs")
                && path.to_string_lossy().contains("dagon-mcu-")
            {
                out.push(std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    out
}

/// Whether every definition generated below `target` carries all of `want`.
fn every_build_generated(target: &Path, want: &[&str]) {
    let defs = generated_definitions(target);
    assert!(
        !defs.is_empty(),
        "no generated definition below {}",
        target.display()
    );
    for d in &defs {
        for w in want {
            assert!(d.contains(w), "{w} not in {}:\n{d}", target.display());
        }
    }
}

/// A project that moves the RP2040's slots by overriding its definition,
/// with app slots of unequal lengths and unequal trial and confirmed
/// watchdog periods, gets firmware laid out for them, without a code edit:
/// `dagon gen` writes the merged definition into the node's boot folder,
/// the app (both slots), the boot core (both slots) and stage0 build from
/// it, each binary loads at its moved slot, stage0's slot table names the
/// moved boot core slots, and every build generated the overridden values.
/// An `mcu.toml` of another MCU or definition format is refused (real
/// firmware builds: run with --ignored).
#[test]
#[ignore = "builds firmware for the RP2040"]
fn a_project_slot_override_reaches_the_firmware() {
    use dagon_format::header::{image_kind, slot_binding};
    let d = tmp("lay");
    // Short: the boot core's build folders are deep below it.
    let dir = d.join("p");
    // The hub's board: its crystal gives the USB uplink its clock.
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(MOVED_RP2040_SLOTS);
    std::fs::write(&config, text).unwrap();
    let lines = gen_project(&config, &["hub".to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let reg = crate::defs::registry(&cfg).unwrap();
    let mcu_toml = dir.join("boot/hub/mcu.toml");
    assert_eq!(
        std::fs::read_to_string(&mcu_toml).ok(),
        dagon_topology::defs::boot_folder::mcu_file_text(&reg, "rp2040")
    );
    let slot = |kind, binding| *reg.slot("rp2040", kind, binding).unwrap();
    let loads_at = |elf: &[u8], at: u32, what: &str| {
        dagon_sign::image::payload_from_file(elf, at).unwrap_or_else(|e| panic!("{what}: {e}"))
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut log = Vec::new();

    // The app, both slots.
    let built = crate::nodebuild::build_nodes(
        &cfg,
        &["hub".to_string()],
        &mut |l| log.push(l.to_string()),
        &cancel,
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    let apps = &built[0].slots;
    assert_eq!(apps.len(), 2);
    for (letter, path, _) in apps {
        let binding = crate::commands::binding_of(*letter);
        let s = slot(image_kind::APP, binding);
        assert_eq!(
            s.start,
            if *letter == 'a' {
                0x1005_0000
            } else {
                0x1011_0000
            }
        );
        loads_at(
            &std::fs::read(path).unwrap(),
            s.load_addr,
            &format!("app {letter}"),
        );
    }
    let overridden = [
        "pub const APP: [(u32, u32); 2] = [(0x10050000, 0xc0000), (0x10110000, 0x80000)];",
        "pub const TRIAL_WDT_MS: u32 = 6000;",
        "pub const CONFIRMED_WDT_MS: u32 = 8000;",
    ];
    every_build_generated(&dir.join("nodes/hub/target"), &overridden);

    // The boot core, both slots.
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([9; 32]));
    let hex = dagon_sign::to_hex(&key.pk[..]);
    let n = cfg_topology(&cfg).node("hub").unwrap().clone();
    let cores =
        crate::flash::build_core_slots(&cfg, &reg, &n, &hex, &mut |l| log.push(l.into()), &cancel)
            .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    assert_eq!(cores.len(), 2);
    let core_starts: Vec<u32> = [slot_binding::A, slot_binding::B]
        .into_iter()
        .map(|b| slot(image_kind::BOOTCORE, b).start)
        .collect();
    assert_eq!(core_starts, [0x1001_0000, 0x1003_0000]);
    for (letter, elf) in &cores {
        let s = slot(image_kind::BOOTCORE, crate::commands::binding_of(*letter));
        loads_at(elf, s.load_addr, &format!("boot core {letter}"));
    }

    // Stage0: its table of boot core slots is the moved one.
    let parts = crate::flash::build_parts(
        &cfg,
        &reg,
        &n,
        Path::new(crate::dagonsrc::BUILT_FROM),
        &hex,
        crate::flash::Paths::Full,
        &mut |l| log.push(l.into()),
        &cancel,
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    let stage0 = &parts.iter().find(|(name, _)| name == "stage0").unwrap().1;
    let stage0_at = reg.mcu("rp2040").unwrap().layout.stage0.unwrap().at;
    let image = loads_at(&std::fs::read(stage0).unwrap(), stage0_at, "stage0");
    let has_word = |w: u32| image.windows(4).any(|x| x == w.to_le_bytes());
    for at in &core_starts {
        assert!(has_word(*at), "stage0 does not name boot core slot {at:#x}");
    }
    for builtin in [0x1000_8000u32, 0x1002_8000] {
        assert!(!has_word(builtin), "stage0 still names {builtin:#x}");
    }
    every_build_generated(&dir.join("out"), &overridden);

    // A node definition of another MCU or format is refused, even with
    // hashes that match it.
    use dagon_topology::defs::boot_folder::{defs_text, DEFS_FILE, MCU_FILE};
    let good = std::fs::read_to_string(&mcu_toml).unwrap();
    let defs = dir.join("boot/hub").join(DEFS_FILE);
    let good_defs = std::fs::read_to_string(&defs).unwrap();
    // Every line gen wrote, the node definition's hashing `mcu`.
    let rehashed = |mcu: &str| -> String {
        let own = format!("  {MCU_FILE}");
        good_defs
            .lines()
            .map(|l| {
                if l.ends_with(&own) {
                    defs_text(&[(MCU_FILE, mcu)])
                } else {
                    format!("{l}\n")
                }
            })
            .collect()
    };
    for (bad, says) in [
        (
            good.replace("id = \"rp2040\"", "id = \"stm32f401re\""),
            "this package is for MCU 'rp2040'",
        ),
        (
            good.replace("format = 3", "format = 4"),
            "this package reads definition format 3",
        ),
    ] {
        assert_ne!(bad, good);
        std::fs::write(&defs, rehashed(&bad)).unwrap();
        std::fs::write(&mcu_toml, bad).unwrap();
        let mut log = Vec::new();
        let r = crate::nodebuild::build_nodes(
            &cfg,
            &["hub".to_string()],
            &mut |l| log.push(l.to_string()),
            &cancel,
        );
        assert!(r.is_err(), "{says}");
        assert!(log.join("\n").contains(says), "{says}\n{}", log.join("\n"));
    }
    std::fs::write(&mcu_toml, good).unwrap();
    std::fs::write(&defs, good_defs).unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

/// The F401's app slots of unequal lengths (slot B shorter than its sector) and
/// unequal trial and confirmed watchdog periods, by a project override.
const UNEQUAL_F401: &str = "
[[mcu_override.stm32f401re.slot]]
image = \"app\"
binding = \"b\"
size = 0x1_C000

[[mcu_override.stm32f401re.slot_layout]]
target = \"app\"
slot = \"b\"
capacity = 0x1_BE00

[mcu_override.stm32f401re.watchdog]
trial_ms = 9_000
confirmed_ms = 12_000
";

/// The F401 node builds from the project's definition as the RP2040 hub
/// does: both app slots, at their own lengths, and the boot core and
/// stage0 with the overridden watchdog periods (real firmware builds: run
/// with --ignored).
#[test]
#[ignore = "builds firmware for the STM32F401"]
fn a_project_override_of_unequal_f401_slots_reaches_the_firmware() {
    use dagon_format::header::image_kind;
    let d = tmp("f4u");
    let dir = d.join("p");
    let topology = without_uart(&sample_by_mcu());
    new_project(&dir, "proj", &topology, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(UNEQUAL_F401);
    std::fs::write(&config, text).unwrap();
    let lines = gen_project(&config, &["seg".to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let reg = crate::defs::registry(&cfg).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut log = Vec::new();
    let built = crate::nodebuild::build_nodes(
        &cfg,
        &["seg".to_string()],
        &mut |l| log.push(l.to_string()),
        &cancel,
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    for (letter, path, _) in &built[0].slots {
        let s = reg
            .slot(
                "stm32f401re",
                image_kind::APP,
                crate::commands::binding_of(*letter),
            )
            .unwrap();
        dagon_sign::image::payload_from_file(&std::fs::read(path).unwrap(), s.load_addr)
            .unwrap_or_else(|e| panic!("app {letter}: {e}"));
    }
    let overridden = [
        "pub const APP: [(u32, u32); 2] = [(0x8020000, 0x20000), (0x8040000, 0x1c000)];",
        "pub const TRIAL_WDT_MS: u32 = 9000;",
        "pub const CONFIRMED_WDT_MS: u32 = 12000;",
    ];
    every_build_generated(&dir.join("nodes/seg/target"), &overridden);
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([9; 32]));
    let hex = dagon_sign::to_hex(&key.pk[..]);
    let n = cfg_topology(&cfg).node("seg").unwrap().clone();
    crate::flash::build_parts(
        &cfg,
        &reg,
        &n,
        Path::new(crate::dagonsrc::BUILT_FROM),
        &hex,
        crate::flash::Paths::Full,
        &mut |l| log.push(l.into()),
        &cancel,
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    every_build_generated(&dir.join("out"), &overridden);
    let _ = std::fs::remove_dir_all(&d);
}

/// The hub's app slot B, shorter, and the resets of its touch controller
/// and its display swapped (touch on GPIO13, display on GPIO22), by a
/// project's overrides of the RP2040 and of the hub's board.
const HUB_SLOT_AND_PIN: &str = "
[[mcu_override.rp2040.slot]]
image = \"app\"
binding = \"b\"
size = 0x8_0000

[[mcu_override.rp2040.slot_layout]]
target = \"app\"
slot = \"b\"
capacity = 0x7_FE00

[[board_override.\"waveshare-rp2040-touch-lcd-1.28\".pin]]
pin = \"GPIO22\"
name = \"lcd_rst\"

[[board_override.\"waveshare-rp2040-touch-lcd-1.28\".pin]]
pin = \"GPIO13\"
name = \"touch_rst\"
";

/// The files named `name` each build of package `package` generated below
/// `target` (in its `OUT_DIR`).
fn generated_by(target: &Path, package: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![target.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.file_name().is_some_and(|n| n == name)
                && path.to_string_lossy().contains(package)
            {
                out.push(std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    out
}

/// A project that overrides an MCU slot and a board pin gets an app built
/// for both: `dagon gen` writes the merged MCU and board definitions and
/// their hashes into the node's folder; the MCU package builds the
/// shorter slot B and the board package takes the two resets at their
/// swapped pins, which the board's code takes by name. A `board.toml` or `mcu.toml`
/// edited by hand after gen is refused, by `dagon build` before cargo
/// runs and by the packages' builds themselves (real firmware builds: run
/// with --ignored).
#[test]
#[ignore = "builds firmware for the RP2040"]
fn a_project_slot_and_board_pin_override_reaches_the_firmware() {
    use dagon_topology::defs::boot_folder::{BOARD_FILE, DEFS_FILE, MCU_FILE};
    let d = tmp("pin");
    let dir = d.join("p");
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(HUB_SLOT_AND_PIN);
    std::fs::write(&config, text).unwrap();
    let lines = gen_project(&config, &["hub".to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    let boot = dir.join("boot/hub");
    for f in [MCU_FILE, BOARD_FILE, DEFS_FILE] {
        assert!(boot.join(f).is_file(), "{f}: {lines:?}");
    }
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let build = |log: &mut Vec<String>| {
        crate::nodebuild::build_nodes(
            &cfg,
            &["hub".to_string()],
            &mut |l| log.push(l.to_string()),
            &cancel,
        )
    };
    let mut log = Vec::new();
    build(&mut log).unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    let target = dir.join("nodes/hub/target");
    every_build_generated(
        &target,
        &["pub const APP: [(u32, u32); 2] = [(0x10048000, 0x100000), (0x10148000, 0x80000)];"],
    );
    let board_pkg = "dagon-board-waveshare-rp2040-touch-lcd-1-28";
    let pins = generated_by(&target, board_pkg, "pins.rs");
    assert!(!pins.is_empty(), "no pins.rs of {board_pkg}");
    for p in &pins {
        for want in [
            "pub type TouchRst = embassy_rp::peripherals::PIN_13;",
            "pub type LcdRst = embassy_rp::peripherals::PIN_22;",
        ] {
            assert!(p.contains(want), "{want} in\n{p}");
        }
    }

    // Edited by hand after gen: refused by the build command, and by the
    // package builds when cargo is run without it.
    let app = dir.join("nodes/hub");
    let bin = crate::nodebuild::package_name(&app).unwrap();
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([9; 32]));
    let hex = dagon_sign::to_hex(&key.pk[..]);
    for (file, from, to) in [
        (BOARD_FILE, "\"GPIO13\"", "\"GPIO12\""),
        (MCU_FILE, "size = 524288", "size = 589824"),
    ] {
        let path = boot.join(file);
        let good = std::fs::read_to_string(&path).unwrap();
        let bad = good.replacen(from, to, 1);
        assert_ne!(bad, good, "{file}");
        std::fs::write(&path, bad).unwrap();
        let mut log = Vec::new();
        let e = build(&mut log).unwrap_err();
        assert!(e.contains(file) && e.contains(DEFS_FILE), "{file}: {e}");
        assert!(!log.iter().any(|l| l.contains("Compiling")), "{log:?}");
        let mut log = Vec::new();
        let r = crate::nodebuild::cargo_build(
            &app,
            &bin,
            &[],
            None,
            crate::nodebuild::FirmwareEnv {
                pubkey_hex: Some(&hex),
                ..Default::default()
            },
            &mut |l| log.push(l.to_string()),
            &cancel,
        );
        let log = log.join("\n");
        assert!(r.is_err(), "{file}: built\n{log}");
        assert!(
            log.contains(file) && log.contains(DEFS_FILE),
            "{file}:\n{log}"
        );
        std::fs::write(&path, good).unwrap();
    }
    // Without its hashes (an older folder, or one deleted): the packages'
    // own builds refuse it too.
    std::fs::remove_file(boot.join(DEFS_FILE)).unwrap();
    let mut log = Vec::new();
    let r = crate::nodebuild::cargo_build(
        &app,
        &bin,
        &[],
        None,
        crate::nodebuild::FirmwareEnv {
            pubkey_hex: Some(&hex),
            ..Default::default()
        },
        &mut |l| log.push(l.to_string()),
        &cancel,
    );
    let log = log.join("\n");
    assert!(r.is_err(), "built without hashes\n{log}");
    assert!(log.contains("no defs.sha256"), "{log}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The ESP32-S3 node's factory agent, a part of its board package, builds
/// from the project's definition like the app: an override of the
/// watchdog periods reaches it (real firmware builds with the ESP
/// toolchain: run with --ignored).
#[test]
#[ignore = "builds firmware with the ESP toolchain"]
fn a_project_override_reaches_the_esp32s3_factory_agent() {
    let d = tmp("espf");
    let dir = d.join("p");
    new_project(&dir, "proj", SAMPLE, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str("\n[mcu_override.esp32s3.watchdog]\ntrial_ms = 3_000\nconfirmed_ms = 7_000\n");
    std::fs::write(&config, text).unwrap();
    let lines = gen_project(&config, &["watch".to_string()], false).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    assert!(dir.join("boot/watch/mcu.toml").is_file());
    let cfg = Config::load(Some(&config.to_string_lossy())).unwrap();
    let reg = crate::defs::registry(&cfg).unwrap();
    let n = cfg_topology(&cfg).node("watch").unwrap().clone();
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([9; 32]));
    let mut log: Vec<String> = Vec::new();
    let parts = crate::flash::build_parts(
        &cfg,
        &reg,
        &n,
        Path::new(crate::dagonsrc::BUILT_FROM),
        &dagon_sign::to_hex(&key.pk[..]),
        crate::flash::Paths::Full,
        &mut |l| log.push(l.into()),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
    assert!(parts.iter().any(|(name, _)| name == "factory"), "{parts:?}");
    every_build_generated(
        &dir.join("out/flash/target/factory"),
        &[
            "pub const TRIAL_WDT_MS: u32 = 3000;",
            "pub const CONFIRMED_WDT_MS: u32 = 7000;",
        ],
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn cfg_topology(cfg: &Config) -> Topology {
    read_topology(cfg, &cfg.configured_topology().unwrap()).unwrap()
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

/// Every app template (an MCU's or a board's) builds the app from the
/// node's MCU definition `dagon gen` writes into `boot/<id>/`, as the
/// boot crate builds its boot core and stage0 (the same folder).
#[test]
fn every_app_template_builds_from_the_nodes_mcu_definition() {
    let root = Path::new(crate::dagonsrc::BUILT_FROM).join("targets");
    let want =
        "DAGON_BOOT_CRATE = { value = \"../../boot/{{node}}\", relative = true, force = true }";
    let mut seen = 0;
    for group in ["mcu", "boards"] {
        for e in std::fs::read_dir(root.join(group)).unwrap().flatten() {
            let config = e.path().join("template/.cargo/config.toml");
            let Ok(text) = std::fs::read_to_string(&config) else {
                continue;
            };
            seen += 1;
            assert!(text.contains(want), "{}", config.display());
        }
    }
    assert!(seen >= 6, "{seen}");
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
    let pins = t
        .reached_over("seg")
        .and_then(|l| l.end("seg")?.node_pins.clone())
        .unwrap();
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
    // The same links to other nodes, listed with the 親機's end second.
    let to = |mut l: dagon_topology::schema::Link, node: &str| {
        l.ends.reverse();
        l.ends[0].node = node.into();
        l
    };
    let mut i2c2 = to(i2c.clone(), "second");
    i2c2.bus = Some("bus-2".into());
    let uart2 = to(uart.clone(), "third");
    many.links.extend([i2c2, uart2.clone()]);
    let hub = many.node("hub").unwrap().clone();
    let e = wiring_placeholders(reg, &many, &hub).unwrap_err();
    assert!(
        e.contains("one I2C bus")
            && e.contains("bus-2")
            && e.contains("between 'second' and 'hub'"),
        "{e}"
    );
    many.links.retain(|l| l.end("second").is_none());
    let n = &hub;
    let e = wiring_placeholders(reg, &many, n).unwrap_err();
    assert!(
        e.contains("one UART link") && e.contains("between 'third' and 'hub'"),
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
    use dagon_topology::links::{available_in, Dir};
    assert_eq!(kind_variant(LinkKind::WifiTcp), "WifiTcp");
    for (mcu, up, down) in FW_OFFERS {
        let names = |d: Dir| -> Vec<&'static str> {
            let mut v: Vec<&'static str> = available_in(Registry::builtin(), mcu, d)
                .iter()
                .map(|k| k.name())
                .collect();
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
        &["hub".into()],
        false,
    )
    .unwrap_err();
    assert!(
        e.contains("uplink of hub over ble: the firmware of rp2040 does not run this link kind"),
        "{e}"
    );
    // A link a 子機 cannot do: named by its two ends.
    let wifi = dagon_topology::parse_str(&SAMPLE.replacen(
        "  - kind: i2c\n    bus: ",
        "  - kind: wifi-tcp\n    bus: ",
        1,
    ))
    .unwrap();
    let e = gen(
        dagon_topology::defs::Registry::builtin(),
        &base,
        &wifi,
        &src,
        &["seg".into()],
        false,
    )
    .unwrap_err();
    assert!(
        e.contains("link between 'hub' and 'seg' over wifi-tcp: the firmware of"),
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

/// A node's I2C target address is its end's `addr` in the topology, not its
/// board's `[default_link] addr` (0x42 on the NUCLEO-F401RE): `dagon gen`
/// writes it into the app's wiring and the boot core's wiring, and the
/// app's start-up hands it to the board; a node the topology gives no
/// address keeps `None` (its board's default).
#[test]
fn a_non_default_topology_address_reaches_the_app_and_the_boot_core() {
    let d = tmp("addr");
    let dir = d.join("proj");
    let seg = "{ node: seg, port: i2c3, addr: 0x42,";
    assert!(SAMPLE.contains(seg));
    let topology = SAMPLE.replace(seg, "{ node: seg, port: i2c3, addr: 0x45,");
    new_project(&dir, "proj", &topology, &d.join("keys"), &dev()).unwrap();
    let config = dir.join(crate::config::CONFIG_FILE);
    let lines = gen_project(&config, &["seg".into(), "watch".into()], false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
    let want = "pub const LINK_ADDR: Option<u8> = Some(0x45);\n";
    for rel in ["nodes/seg/src/wiring.rs", "nodes/seg/dagon/boot_wiring.rs"] {
        let text = read(&dir, rel);
        assert!(text.contains(want), "{rel}: {text}");
    }
    let main = read(&dir, "nodes/seg/src/main.rs");
    assert!(main.contains("wiring::LINK_ADDR"), "{main}");
    let none = "pub const LINK_ADDR: Option<u8> = None;\n";
    let watch = read(&dir, "nodes/watch/src/wiring.rs");
    assert!(watch.contains(none), "{watch}");
}
