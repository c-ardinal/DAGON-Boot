//! `dagon new` and `dagon gen`: a build-ready project with one Rust crate
//! per topology node, on top of the board support packages
//! (`dagon-board-<board>` when the node names a board, `dagon-mcu-<mcu>`
//! when it names only an MCU).
//!
//! ```text
//! <project>/
//!   dagon.toml          topology, [dagon] crate source, [build.<id>], [board.<id>]
//!   topology.yaml
//!   nodes/<id>/Cargo.toml, .cargo/config.toml, (ESP) rust-toolchain.toml
//!   nodes/<id>/src/main.rs   start-up, written once (--force-main rewrites)
//!   nodes/<id>/src/app.rs    the developer's code, never touched again
//!   nodes/<id>/src/wiring.rs constants from the topology, rewritten by gen
//! ```
//!
//! `dagon gen` rewrites only wiring.rs and the board-package line of
//! Cargo.toml, and adds missing [build.<id>] tables; everything the
//! developer wrote stays.

use std::path::{Path, PathBuf};

use dagon_topology::schema::{LinkKind, Node, Role, Topology};
use serde::Deserialize;

use crate::config::{read_topology, Config, Opts};
use crate::paths::{contained, write_atomic};

const SAMPLE: &str = include_str!("../../../topology/desk-demo.yaml");
/// The DAGON clone this CLI was built from (default crate source).
const BUILT_FROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// Overrides the DAGON clone used as crate source.
const HOME_ENV: &str = "DAGON_HOME";
const FIRST_VERSION: &str = "0.1.0";

/// Where generated projects take the DAGON crates from (`[dagon]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DagonSource {
    /// Git repository and the release tag it is pinned to.
    pub git: Option<String>,
    pub tag: Option<String>,
    /// A local clone (development); relative to dagon.toml.
    pub path: Option<PathBuf>,
}

/// Board facts for a node chosen by MCU only (`[board.<id>]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardFacts {
    pub flash_bytes: Option<u32>,
}

/// What a node's support package is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Cargo package name.
    pub name: String,
    /// The node names a board (else only an MCU).
    pub board: bool,
}

/// `dagon-board-<board>` or `dagon-mcu-<mcu>`, as a cargo package name.
pub fn package_of(n: &Node) -> Package {
    let clean = |s: &str| s.replace(['.', '_'], "-").to_ascii_lowercase();
    match &n.board {
        Some(b) => Package {
            name: format!("dagon-board-{}", clean(b)),
            board: true,
        },
        None => Package {
            name: format!("dagon-mcu-{}", clean(&n.mcu)),
            board: false,
        },
    }
}

struct Family {
    target: &'static str,
    rustflags: &'static str,
    extra: &'static str,
    toolchain: Option<&'static str>,
}

fn family(mcu: &str) -> Result<Family, String> {
    Ok(match mcu {
        "rp2040" => Family {
            target: "thumbv6m-none-eabi",
            rustflags: r#"["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x"]"#,
            extra: "",
            toolchain: None,
        },
        "stm32f401re" => Family {
            target: "thumbv7em-none-eabihf",
            rustflags: r#"["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x"]"#,
            extra: "runner = \"probe-rs run --chip STM32F401RETx\"\n",
            toolchain: None,
        },
        "esp32s3" => Family {
            target: "xtensa-esp32s3-none-elf",
            rustflags: r#"["-C", "link-arg=-nostartfiles", "-C", "link-arg=-Tlinkall.x", "-C", "llvm-args=--mtext-section-literals"]"#,
            extra: "",
            toolchain: Some("[toolchain]\nchannel = \"esp\"\n"),
        },
        other => return Err(format!("no project template for MCU '{other}' yet")),
    })
}

/// A file written (or kept) by gen, relative to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: String,
    pub action: &'static str,
}

/// The dependency line of `pkg` for the configured source.
fn dependency(pkg: &Package, src: &DagonSource, base: &Path) -> Result<String, String> {
    if let (Some(git), Some(tag)) = (&src.git, &src.tag) {
        return Ok(format!("{} = {{ git = {git:?}, tag = {tag:?} }}", pkg.name));
    }
    let root = match (&src.path, std::env::var_os(HOME_ENV)) {
        (Some(p), _) => base.join(p),
        (None, Some(h)) => PathBuf::from(h),
        (None, None) => PathBuf::from(BUILT_FROM),
    };
    let dir = find_package(&root, &pkg.name).ok_or_else(|| {
        format!(
            "no package '{}' in {} (the board support package may not exist there yet)",
            pkg.name,
            root.display()
        )
    })?;
    let dir = dir.to_string_lossy().replace('\\', "/");
    Ok(format!("{} = {{ path = {dir:?} }}", pkg.name))
}

/// Directory of the cargo package `name` under `root` (target/, .git and
/// node_modules skipped).
pub fn find_package(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let toml = dir.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&toml) {
            let found = toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|v| v.get("package")?.get("name")?.as_str().map(|n| n == name));
            if found == Some(true) {
                return std::fs::canonicalize(&dir).ok().or(Some(dir));
            }
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let n = e.file_name();
            let skip = matches!(n.to_str(), Some("target" | ".git" | "node_modules" | "out"));
            if !skip && e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    None
}

fn cargo_toml(id: &str, pkg: &Package, dep: &str) -> String {
    format!(
        "[package]\nname = \"{id}\"\nversion = \"{FIRST_VERSION}\"\nedition = \"2021\"\npublish = false\n\n\
         # One crate per node: each builds for its own target.\n[workspace]\n\n\
         [features]\n# Links the application for slot B (dagon build builds both).\n\
         slot-b = [\"{name}/slot-b\"]\n\n\
         [dependencies]\n# DAGON board support (rewritten by dagon gen).\n{dep}\n\n\
         [profile.release]\nopt-level = \"s\"\nlto = true\ncodegen-units = 1\ndebug = true\npanic = \"abort\"\n",
        name = pkg.name
    )
}

fn cargo_config(f: &Family) -> String {
    format!(
        "[build]\ntarget = \"{t}\"\n\n[target.{t}]\nrustflags = {r}\n{extra}",
        t = f.target,
        r = f.rustflags,
        extra = f.extra
    ) + if f.target.starts_with("xtensa") {
        "\n[unstable]\nbuild-std = [\"core\"]\n"
    } else {
        ""
    }
}

fn main_rs(pkg: &Package) -> String {
    let krate = pkg.name.replace('-', "_");
    format!(
        "//! Start-up written by `dagon gen`: the board, the DAGON node (link,\n\
         //! update receiver, trial and confirm) and your application in app.rs.\n\
         //! You do not need to change this file.\n\
         #![no_std]\n#![no_main]\n\nmod app;\nmod wiring;\n\n\
         {krate}::dagon_main!(wiring::NODE, app::run, app::self_check);\n"
    )
}

fn app_rs(pkg: &Package) -> String {
    let krate = pkg.name.replace('-', "_");
    format!(
        "//! Your application. DAGON receives updates, runs a new version on\n\
         //! trial and confirms it; this file is never rewritten by dagon gen.\n\n\
         use {krate} as board;\n\n\
         /// Runs for as long as the node is up. Keep feeding `live`: if this\n\
         /// task stops, the watchdog resets the node, and a version on trial\n\
         /// goes back to the previous one.\n\
         pub async fn run(io: board::UserIo, live: board::Lease) {{\n\
         \x20   let _ = io;\n\
         \x20   loop {{\n\
         \x20       live.feed();\n\
         \x20       board::Timer::after_millis(500).await;\n\
         \x20   }}\n\
         }}\n\n\
         /// Whether a version on trial may be confirmed.\n\
         pub fn self_check() -> bool {{\n\
         \x20   true\n\
         }}\n"
    )
}

fn hub_link(t: &Topology, child: &str) -> Option<String> {
    let l = t.links.iter().find(|l| l.to == child)?;
    let ctl = &l.control;
    let opt = |v: Option<u8>| v.map_or("None".into(), |p| format!("Some({p})"));
    Some(match l.kind {
        LinkKind::I2c => {
            let bus = l
                .bus
                .as_ref()
                .and_then(|b| t.buses.iter().find(|x| &x.id == b))?;
            format!(
                "HubLink::I2c {{ sda: {}, scl: {}, speed_hz: {}, addr: {:#04x}, nrst: {}, boot: {} }}",
                bus.sda,
                bus.scl,
                bus.speed_hz.unwrap_or(100_000),
                l.addr.unwrap_or(0),
                opt(ctl.nrst),
                opt(ctl.boot)
            )
        }
        _ => {
            let p = l.pins.as_ref()?;
            format!(
                "HubLink::Uart {{ tx: {}, rx: {}, baud: {} }}",
                p.tx,
                p.rx,
                l.baud.unwrap_or(115_200)
            )
        }
    })
}

/// wiring.rs for node `n`: constants only, from the topology (and
/// `[board.<id>]` for a node chosen by MCU).
pub fn wiring_rs(t: &Topology, n: &Node, board: Option<&BoardFacts>) -> String {
    let hub = n.role == Role::Coordinator;
    let link = if hub {
        "Link::Hub".to_string()
    } else {
        match t.links.iter().find(|l| l.to == n.id) {
            Some(l) if l.kind == LinkKind::I2c => {
                let speed = l
                    .bus
                    .as_ref()
                    .and_then(|b| t.buses.iter().find(|x| &x.id == b))
                    .and_then(|b| b.speed_hz)
                    .unwrap_or(100_000);
                let pins = l.node_pins.as_ref();
                let s = |v: Option<&String>| v.map_or("None".into(), |p| format!("Some({p:?})"));
                format!(
                    "Link::I2c {{ addr: {:#04x}, speed_hz: {speed}, sda: {}, scl: {} }}",
                    l.addr.unwrap_or(0),
                    s(pins.map(|p| &p.sda)),
                    s(pins.map(|p| &p.scl))
                )
            }
            Some(l) => format!("Link::Uart {{ baud: {} }}", l.baud.unwrap_or(115_200)),
            None => "Link::Hub".to_string(),
        }
    };
    let flash = board
        .and_then(|b| b.flash_bytes)
        .map_or("None".into(), |f| format!("Some({f})"));
    let children: Vec<String> = if hub {
        t.update
            .order
            .iter()
            .filter(|id| **id != n.id)
            .filter_map(|id| {
                hub_link(t, id).map(|l| format!("    Child {{ id: {id:?}, link: {l} }},\n"))
            })
            .collect()
    } else {
        Vec::new()
    };
    format!(
        "//! Generated by `dagon gen` from the topology \"{topo}\". Do not edit:\n\
         //! dagon gen rewrites this file; change the topology instead.\n\n\
         #![allow(dead_code)]\n\n\
         /// How this node reaches the hub (the hub itself: `Link::Hub`).\n\
         #[derive(Clone, Copy, Debug)]\npub enum Link {{\n    Hub,\n    \
         I2c {{ addr: u8, speed_hz: u32, sda: Option<&'static str>, scl: Option<&'static str> }},\n    \
         Uart {{ baud: u32 }},\n}}\n\n\
         /// A link as the hub drives it (hub GPIO numbers).\n\
         #[derive(Clone, Copy, Debug)]\npub enum HubLink {{\n    \
         I2c {{ sda: u8, scl: u8, speed_hz: u32, addr: u8, nrst: Option<u8>, boot: Option<u8> }},\n    \
         Uart {{ tx: u8, rx: u8, baud: u32 }},\n}}\n\n\
         #[derive(Clone, Copy, Debug)]\npub struct Child {{\n    pub id: &'static str,\n    pub link: HubLink,\n}}\n\n\
         #[derive(Clone, Copy, Debug)]\npub struct Node {{\n    pub id: &'static str,\n    \
         pub tag: &'static str,\n    pub hub: bool,\n    pub max_attempts: u8,\n    pub link: Link,\n    \
         /// Flash size given for a board chosen by MCU (else the board package knows it).\n    \
         pub flash_bytes: Option<u32>,\n}}\n\n\
         pub const NODE: Node = Node {{\n    id: {id:?},\n    tag: {tag:?},\n    hub: {hub},\n    \
         max_attempts: {att},\n    link: {link},\n    flash_bytes: {flash},\n}};\n\n\
         /// The hub's children in update order (empty on a child).\n\
         pub const CHILDREN: &[Child] = &[\n{kids}];\n",
        topo = t.name,
        id = n.id,
        tag = n.tag,
        att = n.max_attempts.unwrap_or(3),
        kids = children.concat()
    )
}

fn write(
    base: &Path,
    rel: &str,
    text: &str,
    overwrite: bool,
    out: &mut Vec<Written>,
) -> Result<(), String> {
    let path = contained(base, Path::new(rel))?;
    let exists = path.exists();
    if exists && !overwrite {
        out.push(Written {
            path: rel.into(),
            action: "kept",
        });
        return Ok(());
    }
    let same = exists && std::fs::read_to_string(&path).is_ok_and(|t| t == text);
    if !same {
        write_atomic(&path, text.as_bytes())?;
    }
    out.push(Written {
        path: rel.into(),
        action: match (exists, same) {
            (false, _) => "created",
            (true, true) => "unchanged",
            (true, false) => "rewritten",
        },
    });
    Ok(())
}

/// Replaces the board-package dependency line in an existing Cargo.toml,
/// keeping the rest.
fn update_dependency(
    base: &Path,
    rel: &str,
    pkg: &Package,
    dep: &str,
    out: &mut Vec<Written>,
) -> Result<(), String> {
    let path = contained(base, Path::new(rel))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|_| format!("{rel} does not parse"))?;
    let want: toml_edit::DocumentMut = format!("[dependencies]\n{dep}\n")
        .parse()
        .map_err(|_| "bad dependency line".to_string())?;
    let deps = doc
        .entry("dependencies")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| format!("{rel}: dependencies is not a table"))?;
    deps.retain(|k, _| {
        !(k.starts_with("dagon-board-") || k.starts_with("dagon-mcu-")) || k == pkg.name
    });
    deps.insert(&pkg.name, want["dependencies"][&pkg.name].clone());
    let new = doc.to_string();
    let changed = new != text;
    if changed {
        write_atomic(&path, new.as_bytes())?;
    }
    out.push(Written {
        path: rel.into(),
        action: if changed { "rewritten" } else { "unchanged" },
    });
    Ok(())
}

/// Generates (or refreshes) the crates of `only` (all nodes when empty).
pub fn gen(
    base: &Path,
    t: &Topology,
    src: &DagonSource,
    boards: &std::collections::BTreeMap<String, BoardFacts>,
    only: &[String],
    force_main: bool,
) -> Result<Vec<Written>, String> {
    let mut out = Vec::new();
    for n in &t.nodes {
        if !only.is_empty() && !only.contains(&n.id) {
            continue;
        }
        let pkg = package_of(n);
        let fam = family(&n.mcu)?;
        let dep = dependency(&pkg, src, base)?;
        let dir = format!("nodes/{}", n.id);
        let cargo = format!("{dir}/Cargo.toml");
        if contained(base, Path::new(&cargo))?.exists() {
            update_dependency(base, &cargo, &pkg, &dep, &mut out)?;
        } else {
            write(
                base,
                &cargo,
                &cargo_toml(&n.id, &pkg, &dep),
                false,
                &mut out,
            )?;
        }
        write(
            base,
            &format!("{dir}/.cargo/config.toml"),
            &cargo_config(&fam),
            false,
            &mut out,
        )?;
        if let Some(tc) = fam.toolchain {
            write(
                base,
                &format!("{dir}/rust-toolchain.toml"),
                tc,
                false,
                &mut out,
            )?;
        }
        write(
            base,
            &format!("{dir}/src/main.rs"),
            &main_rs(&pkg),
            force_main,
            &mut out,
        )?;
        write(
            base,
            &format!("{dir}/src/app.rs"),
            &app_rs(&pkg),
            false,
            &mut out,
        )?;
        let wiring = wiring_rs(t, n, boards.get(&n.id));
        write(
            base,
            &format!("{dir}/src/wiring.rs"),
            &wiring,
            true,
            &mut out,
        )?;
    }
    Ok(out)
}

/// Adds `[build.<id>]` for nodes that have none (comments kept).
pub fn add_build_tables(config: &Path, t: &Topology) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| "dagon.toml does not parse".to_string())?;
    let builds = doc
        .entry("build")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or("build in dagon.toml is not a table")?;
    builds.set_implicit(true);
    let mut added = Vec::new();
    for n in &t.nodes {
        if builds.contains_key(&n.id) {
            continue;
        }
        let mut tbl = toml_edit::Table::new();
        tbl.insert("package", toml_edit::value(format!("nodes/{}", n.id)));
        tbl.insert("slot_b_feature", toml_edit::value("slot-b"));
        tbl.insert("version", toml_edit::value(FIRST_VERSION));
        builds.insert(&n.id, toml_edit::Item::Table(tbl));
        added.push(n.id.clone());
    }
    if !added.is_empty() {
        write_atomic(config, doc.to_string().as_bytes())?;
    }
    Ok(added)
}

/// The `[dagon]` and `[board.<id>]` tables of dagon.toml.
#[derive(Debug, Default, Deserialize)]
struct ProjectTables {
    #[serde(default)]
    dagon: DagonSource,
    #[serde(default)]
    board: std::collections::BTreeMap<String, BoardFacts>,
}

fn project_tables(config: &Path) -> Result<ProjectTables, String> {
    let text = std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", config.display()))
}

/// `dagon gen [<id>...] [--force-main] [--config <file>]`
pub fn gen_command(args: &[&str]) -> Result<(), String> {
    let force = args.contains(&"--force-main");
    let args: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != "--force-main")
        .collect();
    let o = Opts::parse(&args, &["config"])?;
    let config = PathBuf::from(o.get("config").unwrap_or(crate::config::CONFIG_FILE));
    let cfg = Config::load(Some(&config.to_string_lossy()))?;
    let t = read_topology(&contained(&cfg.base, &cfg.topology_path(None)?)?)?;
    let tables = project_tables(&config)?;
    let only: Vec<String> = o.pos.iter().map(|s| s.to_string()).collect();
    if let Some(bad) = only.iter().find(|id| t.node(id).is_none()) {
        return Err(format!("no node '{bad}' in the topology"));
    }
    let written = gen(&cfg.base, &t, &tables.dagon, &tables.board, &only, force)?;
    for w in &written {
        println!("{:<10} {}", w.action, w.path);
    }
    for id in add_build_tables(&config, &t)? {
        println!("added      [build.{id}] to dagon.toml");
    }
    for n in &t.nodes {
        if n.board.is_none()
            && tables
                .board
                .get(&n.id)
                .and_then(|b| b.flash_bytes)
                .is_none()
        {
            println!(
                "note: {} is chosen by MCU; set flash_bytes in [board.{}]",
                n.id, n.id
            );
        }
    }
    Ok(())
}

/// `dagon new <dir> [--from <yaml>] [--keys <dir>]`
pub fn new_command(args: &[&str]) -> Result<(), String> {
    let o = Opts::parse(args, &["from", "keys"])?;
    let [dir] = o.pos[..] else {
        return Err("new needs <dir>".into());
    };
    let dir = PathBuf::from(dir);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .ok_or("new needs a folder name")?;
    let yaml = match o.get("from") {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
        None => SAMPLE.to_string(),
    };
    let keys = match o.get("keys") {
        Some(k) => PathBuf::from(k),
        None => home()?.join(".dagon").join("keys").join(&name),
    };
    let made = new_project(&dir, &name, &yaml, &keys)?;
    for line in &made {
        println!("{line}");
    }
    println!(
        "next: cd {} && dagon gen, then dagon serve (signing: set DAGON_SIGNING_KEY={})",
        dir.display(),
        keys.join(dagon_sign::SECRET_FILE).display()
    );
    Ok(())
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "cannot find the home folder; pass --keys <dir>".into())
}

/// Creates the project folder: dagon.toml, topology.yaml, .gitignore and,
/// when `keys` holds none, a signing key pair there (outside any git work
/// tree). Refuses a non-empty folder.
pub fn new_project(dir: &Path, name: &str, yaml: &str, keys: &Path) -> Result<Vec<String>, String> {
    if dir.exists() && std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("{} exists and is not empty", dir.display()));
    }
    dagon_topology::parse_str(yaml).map_err(|e| format!("topology: {e}"))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut made = Vec::new();
    let secret = keys.join(dagon_sign::SECRET_FILE);
    if !secret.exists() {
        let k = dagon_sign::generate_to_dir(keys).map_err(|e| e.to_string())?;
        made.push(format!(
            "key: {} (key id {})",
            k.public_path.display(),
            dagon_sign::to_hex(&k.key_id)
        ));
    }
    let pubkey = keys
        .join(dagon_sign::PUBLIC_FILE)
        .to_string_lossy()
        .replace('\\', "/");
    let src = std::env::var_os(HOME_ENV).map_or_else(
        || std::fs::canonicalize(BUILT_FROM).unwrap_or_else(|_| PathBuf::from(BUILT_FROM)),
        PathBuf::from,
    );
    let src = src.to_string_lossy().replace('\\', "/");
    let src = src.strip_prefix("//?/").unwrap_or(&src).to_string();
    let toml = format!(
        "# DAGON project \"{name}\".\ntopology = \"topology.yaml\"\n\
         # Public key the firmware and the page verify images with (the secret\n\
         # key stays outside the project: DAGON_SIGNING_KEY).\npubkey = {pubkey:?}\n\n\
         # Where the generated crates take the DAGON board support packages from:\n\
         # a release (git and tag) or, for development, a local clone (path).\n\
         [dagon]\n# git = \"<DAGON repository URL>\"\n# tag = \"<release tag>\"\npath = {src:?}\n"
    );
    let files = [
        ("dagon.toml", toml.as_str()),
        ("topology.yaml", yaml),
        (".gitignore", "/out/\ntarget/\n"),
    ];
    for (f, text) in files {
        write_atomic(&dir.join(f), text.as_bytes())?;
        made.push(format!("created    {f}"));
    }
    Ok(made)
}

#[cfg(test)]
mod tests;
