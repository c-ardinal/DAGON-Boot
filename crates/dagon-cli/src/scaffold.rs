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
//! The files of a node come from its package's project template: a
//! `template/` folder shipped inside the board (or MCU) package, so the
//! package owns its start-up code, else a copy built into this CLI
//! (crates/dagon-cli/templates/<package>/). Placeholders: `{{node}}`,
//! `{{tag}}`, `{{dep:<package>}}` (the dependency line for that package).
//!
//! `dagon gen` rewrites only wiring.rs and the DAGON package entries of
//! Cargo.toml, and adds missing [build.<id>] tables; everything the
//! developer wrote stays.

use std::path::{Path, PathBuf};

use dagon_topology::schema::{LinkKind, Node, Role, Topology};
use serde::Deserialize;

use crate::config::{read_topology, Config, Opts};
use crate::paths::{contained, write_atomic};

pub const SAMPLE: &str = include_str!("../../../topology/desk-demo.yaml");
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

/// A file written (or kept) by gen, relative to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: String,
    pub action: &'static str,
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
        "//! Generated by `dagon gen` from the topology {topo:?}. Do not edit:\n\
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

/// What a node needs: its board package, or its MCU package when the node
/// names no board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    Board(String),
    Mcu(String),
}

pub fn want_of(n: &Node) -> Want {
    match &n.board {
        Some(b) => Want::Board(b.clone()),
        None => Want::Mcu(n.mcu.clone()),
    }
}

/// The conventional package name: `dagon-board-<board id>` or
/// `dagon-mcu-<mcu id>`, dots and underscores turned into dashes. A package
/// may also declare its id instead (`[package.metadata.dagon] board = ...`
/// or `mcu = ...`), which a local clone is searched for.
pub fn conventional_name(w: &Want) -> String {
    let clean = |s: &str| s.replace(['.', '_'], "-").to_ascii_lowercase();
    match w {
        Want::Board(b) => format!("dagon-board-{}", clean(b)),
        Want::Mcu(m) => format!("dagon-mcu-{}", clean(m)),
    }
}

/// A package found in a local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub dir: PathBuf,
}

/// Package manifests under `root` (target/, .git, node_modules and out/
/// skipped), as (directory, parsed Cargo.toml).
fn manifests(root: &Path) -> Vec<(PathBuf, toml::Value)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) {
            if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                if v.get("package").is_some() {
                    out.push((dir.clone(), v));
                }
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
    out
}

fn package_name(v: &toml::Value) -> Option<&str> {
    v.get("package")?.get("name")?.as_str()
}

/// The package for `w` in the clone at `root`: by its declared id, else by
/// the conventional name.
pub fn find_for(root: &Path, w: &Want) -> Option<Found> {
    let all = manifests(root);
    let (key, id) = match w {
        Want::Board(b) => ("board", b),
        Want::Mcu(m) => ("mcu", m),
    };
    let declared = all.iter().find(|(_, v)| {
        v.get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("dagon"))
            .and_then(|d| d.get(key))
            .and_then(|x| x.as_str())
            == Some(id.as_str())
    });
    let name = conventional_name(w);
    let by_name = || all.iter().find(|(_, v)| package_name(v) == Some(&name));
    let (dir, v) = declared.or_else(by_name)?;
    Some(Found {
        name: package_name(v)?.to_string(),
        dir: std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()),
    })
}

/// The package directory named `name` in the clone at `root`.
pub fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    manifests(root)
        .into_iter()
        .find(|(_, v)| package_name(v) == Some(name))
        .map(|(d, _)| std::fs::canonicalize(&d).unwrap_or(d))
}

fn clone_root(src: &DagonSource, base: &Path) -> Option<PathBuf> {
    if src.git.is_some() && src.tag.is_some() {
        return None;
    }
    Some(match (&src.path, std::env::var_os(HOME_ENV)) {
        (Some(p), _) => base.join(p),
        (None, Some(h)) => PathBuf::from(h),
        (None, None) => PathBuf::from(BUILT_FROM),
    })
}

/// The dependency line for package `name`.
fn dependency(name: &str, src: &DagonSource, base: &Path) -> Result<String, String> {
    match clone_root(src, base) {
        None => Ok(format!(
            "{name} = {{ git = {:?}, tag = {:?} }}",
            src.git.as_deref().unwrap_or_default(),
            src.tag.as_deref().unwrap_or_default()
        )),
        Some(root) => {
            let dir = find_named(&root, name)
                .ok_or_else(|| format!("no package '{name}' in {}", root.display()))?;
            let dir = crate::config::forward_path(&dir);
            Ok(format!("{name} = {{ path = {dir:?} }}"))
        }
    }
}

/// Project template files of a package: `template/` shipped inside the
/// package (its own snippet), else the copy built into this CLI.
fn template_for(name: &str, dir: Option<&Path>) -> Option<Vec<(String, String)>> {
    if let Some(t) = dir.map(|d| d.join("template")).filter(|t| t.is_dir()) {
        let mut files = Vec::new();
        collect(&t, "", &mut files);
        if !files.is_empty() {
            return Some(files);
        }
    }
    let built_in: Vec<(String, String)> = templates::TEMPLATES
        .iter()
        .filter(|(p, _, _)| *p == name)
        .map(|(_, rel, text)| (rel.to_string(), text.to_string()))
        .collect();
    (!built_in.is_empty()).then_some(built_in)
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if e.path().is_dir() {
            collect(&e.path(), &r, out);
        } else if let Ok(text) = std::fs::read_to_string(e.path()) {
            out.push((r, text));
        }
    }
}

mod templates {
    include!(concat!(env!("OUT_DIR"), "/templates.rs"));
}

/// Fills `{{node}}`, `{{tag}}` and `{{dep:<package>}}` in a template file.
fn render(text: &str, n: &Node, src: &DagonSource, base: &Path) -> Result<String, String> {
    // Package lines first, so a tag cannot add placeholders; the tag goes
    // into string literals and comments, escaped as in a Rust string.
    let mut out = text.to_string();
    while let Some(at) = out.find("{{dep:") {
        let end = out[at..]
            .find("}}")
            .map(|e| at + e)
            .ok_or("unclosed {{dep: in a template")?;
        let name = out[at + 6..end].trim().to_string();
        let line = dependency(&name, src, base)?;
        out.replace_range(at..end + 2, &line);
    }
    let tag = n.tag.escape_debug().to_string();
    Ok(out.replace("{{node}}", &n.id).replace("{{tag}}", &tag))
}

/// Whether a dependency key is a DAGON board or MCU package.
fn dagon_package(k: &str) -> bool {
    k.starts_with("dagon-board-") || k.starts_with("dagon-mcu-")
}

/// Every dependency table of a manifest (`dependencies` and
/// `target.<cfg>.dependencies`), with its key path.
fn dep_tables(doc: &mut toml_edit::DocumentMut) -> Vec<(Vec<String>, &mut toml_edit::Table)> {
    let mut out = Vec::new();
    for (k, item) in doc.as_table_mut().iter_mut() {
        if k == "dependencies" {
            if let Some(t) = item.as_table_mut() {
                out.push((vec!["dependencies".to_string()], t));
            }
        } else if k == "target" {
            if let Some(targets) = item.as_table_mut() {
                for (cfg, c) in targets.iter_mut() {
                    if let Some(d) = c
                        .as_table_mut()
                        .and_then(|c| c.get_mut("dependencies"))
                        .and_then(|d| d.as_table_mut())
                    {
                        out.push((
                            vec![
                                "target".into(),
                                cfg.get().to_string(),
                                "dependencies".into(),
                            ],
                            d,
                        ));
                    }
                }
            }
        }
    }
    out
}

type DagonDeps = std::collections::BTreeMap<Vec<String>, Vec<(String, toml_edit::Item)>>;

/// Brings the DAGON package entries of an existing Cargo.toml in line with
/// the freshly rendered template: replaced, added where missing (also a
/// package the template newly needs) and removed where the template has
/// none; everything else stays.
fn refresh_manifest(
    base: &Path,
    rel: &str,
    rendered: &str,
    out: &mut Vec<Written>,
) -> Result<(), String> {
    let path = contained(base, Path::new(rel))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|_| format!("{rel} does not parse"))?;
    let mut want: toml_edit::DocumentMut = rendered
        .parse()
        .map_err(|_| format!("template for {rel} does not parse"))?;
    let mut fresh = DagonDeps::new();
    for (id, t) in dep_tables(&mut want) {
        let deps: Vec<(String, toml_edit::Item)> = t
            .iter()
            .filter(|(k, _)| dagon_package(k))
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        if !deps.is_empty() {
            fresh.insert(id, deps);
        }
    }
    for (id, t) in dep_tables(&mut doc) {
        let keep: Vec<&str> = fresh
            .get(&id)
            .map(|d| d.iter().map(|(k, _)| k.as_str()).collect())
            .unwrap_or_default();
        let stale: Vec<String> = t
            .iter()
            .map(|(k, _)| k.to_string())
            .filter(|k| dagon_package(k) && !keep.contains(&k.as_str()))
            .collect();
        for k in stale {
            t.remove(&k);
        }
    }
    for (id, deps) in fresh {
        let mut t = doc.as_table_mut();
        for seg in &id {
            t = t
                .entry(seg)
                .or_insert_with(|| {
                    let mut n = toml_edit::Table::new();
                    n.set_implicit(true);
                    toml_edit::Item::Table(n)
                })
                .as_table_mut()
                .ok_or_else(|| format!("{rel}: [{}] is not a table", id.join(".")))?;
        }
        for (k, v) in deps {
            t.insert(&k, v);
        }
    }
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

/// The template files of node `n` (rendered), or why there are none.
fn node_files(n: &Node, src: &DagonSource, base: &Path) -> Result<Vec<(String, String)>, String> {
    let want = want_of(n);
    let (name, dir) = match clone_root(src, base) {
        Some(root) => {
            let f = find_for(&root, &want).ok_or_else(|| {
                format!(
                    "no board support package for {want:?} in {} (looked for {} or a \
                     package declaring it); it may not exist yet",
                    root.display(),
                    conventional_name(&want)
                )
            })?;
            (f.name, Some(f.dir))
        }
        None => (conventional_name(&want), None),
    };
    let files = template_for(&name, dir.as_deref())
        .ok_or_else(|| format!("{name} has no project template yet"))?;
    files
        .into_iter()
        .map(|(rel, text)| Ok((rel, render(&text, n, src, base)?)))
        .collect()
}

/// What `gen` did, and the nodes it could not generate (with the reason).
#[derive(Debug, Default)]
pub struct GenReport {
    pub written: Vec<Written>,
    pub skipped: Vec<(String, String)>,
}

/// Generates (or refreshes) the crates of `only` (all nodes when empty).
/// A node whose package or template is missing is skipped with the reason;
/// the others are still generated.
pub fn gen(
    base: &Path,
    t: &Topology,
    src: &DagonSource,
    boards: &std::collections::BTreeMap<String, BoardFacts>,
    only: &[String],
    force_main: bool,
) -> Result<GenReport, String> {
    let mut r = GenReport::default();
    for n in &t.nodes {
        if !only.is_empty() && !only.contains(&n.id) {
            continue;
        }
        let files = match node_files(n, src, base) {
            Ok(f) => f,
            Err(e) => {
                r.skipped.push((n.id.clone(), e));
                continue;
            }
        };
        let dir = format!("nodes/{}", n.id);
        for (rel, text) in files {
            let path = format!("{dir}/{rel}");
            match rel.as_str() {
                "Cargo.toml" if contained(base, Path::new(&path))?.exists() => {
                    refresh_manifest(base, &path, &text, &mut r.written)?
                }
                "src/app.rs" => write(base, &path, &text, false, &mut r.written)?,
                "src/main.rs" => write(base, &path, &text, force_main, &mut r.written)?,
                "src/wiring.rs" => {}
                _ => write(base, &path, &text, false, &mut r.written)?,
            }
        }
        let wiring = wiring_rs(t, n, boards.get(&n.id));
        write(
            base,
            &format!("{dir}/src/wiring.rs"),
            &wiring,
            true,
            &mut r.written,
        )?;
    }
    Ok(r)
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
        // An MCU without its own slot B link (ESP32-S3: one image runs
        // from either OTA slot) builds once.
        if crate::deploy::has_slot_b_link(&n.mcu) {
            tbl.insert("slot_b_feature", toml_edit::value("slot-b"));
        }
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
    let only: Vec<String> = o.pos.iter().map(|s| s.to_string()).collect();
    for line in gen_project(&config, &only, force)? {
        println!("{line}");
    }
    Ok(())
}

/// `dagon gen` for the project whose dagon.toml is `config`: the lines it
/// reports (written, skipped, added, notes).
pub fn gen_project(config: &Path, only: &[String], force: bool) -> Result<Vec<String>, String> {
    let cfg = Config::load(Some(&config.to_string_lossy()))?;
    let t = read_topology(&contained(&cfg.base, &cfg.topology_path(None)?)?)?;
    let tables = project_tables(config)?;
    if let Some(bad) = only.iter().find(|id| t.node(id).is_none()) {
        return Err(format!("no node '{bad}' in the topology"));
    }
    let report = gen(&cfg.base, &t, &tables.dagon, &tables.board, only, force)?;
    let mut out: Vec<String> = report
        .written
        .iter()
        .map(|w| format!("{:<10} {}", w.action, w.path))
        .collect();
    for (id, why) in &report.skipped {
        out.push(format!("skipped    nodes/{id}: {why}"));
    }
    for id in add_build_tables(config, &t)? {
        out.push(format!("added      [build.{id}] to dagon.toml"));
    }
    for n in &t.nodes {
        if n.board.is_none()
            && tables
                .board
                .get(&n.id)
                .and_then(|b| b.flash_bytes)
                .is_none()
        {
            out.push(format!(
                "note: {} is chosen by MCU; set flash_bytes in [board.{}]",
                n.id, n.id
            ));
        }
    }
    out.push(match cfg.pubkey {
        Some(_) => "next: dagon build (the firmware embeds pubkey from dagon.toml as DAGON_PUBKEY)"
            .to_string(),
        None => "note: set pubkey in dagon.toml; dagon build embeds it as DAGON_PUBKEY".to_string(),
    });
    Ok(out)
}

/// A 構成 with no nodes yet, for a project started from scratch.
pub fn empty_topology(name: &str) -> String {
    format!(
        "dagon: 1
name: {name:?}
nodes: []
update:
  order: []
"
    )
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
        "next: cd {} && dagon gen, dagon build (firmware embeds pubkey from dagon.toml as \
         DAGON_PUBKEY), then dagon serve (signing: set DAGON_SIGNING_KEY={})",
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

/// Claude Code instructions and skills for the developer's own Claude Code:
/// fixed text, so a project never carries keys or paths of this PC in them.
const CLAUDE_FILES: [(&str, &str); 4] = [
    ("CLAUDE.md", include_str!("scaffold/claude/CLAUDE.md")),
    (
        ".claude/skills/fix-a-failing-build/SKILL.md",
        include_str!("scaffold/claude/fix-a-failing-build.md"),
    ),
    (
        ".claude/skills/add-a-board-by-mcu/SKILL.md",
        include_str!("scaffold/claude/add-a-board-by-mcu.md"),
    ),
    (
        ".claude/skills/explain-a-topology-problem/SKILL.md",
        include_str!("scaffold/claude/explain-a-topology-problem.md"),
    ),
];

/// Creates the project folder: dagon.toml, topology.yaml, .gitignore, the
/// Claude Code files and,
/// when `keys` holds none, a signing key pair there (outside any git work
/// tree). Refuses a non-empty folder.
pub fn new_project(dir: &Path, name: &str, yaml: &str, keys: &Path) -> Result<Vec<String>, String> {
    if dir.exists() && std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("{} exists and is not empty", dir.display()));
    }
    dagon_topology::parse_str(yaml).map_err(|e| format!("topology: {e}"))?;
    let existed = dir.exists();
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let files = [("topology.yaml", yaml), (".gitignore", "/out/\ntarget/\n")];
    let written = files
        .into_iter()
        .chain(CLAUDE_FILES)
        .try_for_each(|(f, text)| write_atomic(&dir.join(f), text.as_bytes()));
    let made = written.and_then(|_| init_project(dir, name, "topology.yaml", keys));
    let mut made = match made {
        Ok(m) => m,
        Err(e) => {
            // The folder was empty (or not there): what is in it now is this
            // call's, so taking it away lets the next try start clean.
            undo_new(dir, existed);
            return Err(e);
        }
    };
    for (f, _) in files.into_iter().chain(CLAUDE_FILES) {
        made.push(format!("created    {f}"));
    }
    Ok(made)
}

/// Removes what a failed `new_project` wrote into `dir` (empty before):
/// its files and the folders they made, and `dir` itself when it was new.
fn undo_new(dir: &Path, existed: bool) {
    for (f, _) in [
        ("topology.yaml", ""),
        (".gitignore", ""),
        ("dagon.toml", ""),
    ]
    .into_iter()
    .chain(CLAUDE_FILES)
    {
        let p = dir.join(f);
        let _ = std::fs::remove_file(&p);
        // Folders the file needed, innermost first; only while empty.
        let mut up = p.parent();
        while let Some(d) = up.filter(|d| *d != dir) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            up = d.parent();
        }
    }
    if !existed {
        let _ = std::fs::remove_dir(dir);
    }
}

/// dagon.toml for the 構成 `topology` (a file name in `dir`) and, when
/// `keys` holds none, a signing key pair there. Refuses a folder that
/// already has a dagon.toml.
pub fn init_project(
    dir: &Path,
    name: &str,
    topology: &str,
    keys: &Path,
) -> Result<Vec<String>, String> {
    let config = dir.join(crate::config::CONFIG_FILE);
    if config.exists() {
        return Err("the folder already has a dagon.toml".into());
    }
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
    let src = crate::config::forward_path(&src);
    let toml = format!(
        "# DAGON project {name:?}.\ntopology = {topology:?}\n\
         # Public key the firmware and the page verify images with (the secret\n\
         # key stays outside the project: DAGON_SIGNING_KEY).\npubkey = {pubkey:?}\n\n\
         # Where the generated crates take the DAGON board support packages from:\n\
         # a release (git and tag) or, for development, a local clone (path).\n\
         [dagon]\n# git = \"<DAGON repository URL>\"\n# tag = \"<release tag>\"\npath = {src:?}\n"
    );
    write_atomic(&config, toml.as_bytes())?;
    made.push("created    dagon.toml".into());
    Ok(made)
}

#[cfg(test)]
mod tests;
