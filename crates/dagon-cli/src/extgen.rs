//! The extension and protection files `dagon gen` writes into
//! `nodes/<id>/dagon/`: `boot_wiring.rs` (MCUs with a DAGON boot stage),
//! one linker fragment per image slot (`memory-<image>-<slot>.x`, MCUs that
//! define their RAM) and the readable `protection.toml`; and, for an MCU
//! with a DAGON boot stage, the node's boot crate in `boot/<id>/` (its own
//! folder: cargo would merge a node crate's `.cargo/config.toml`, made for
//! the app, into a crate below it).
//!
//! dagon.toml:
//!
//! ```toml
//! [extensions]
//! packages = ["extensions/*"]   # folders holding dagon-extension.toml
//! [extensions.nodes.<id>]
//! boot = ["uds-transport"]      # the boot core's chain, in order
//! app = ["uds-transport"]       # the app's (and a hub's relay) chain
//! trusted = [{ id = "hsm-verify", sha256 = "<64 hex digits>" }]
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dagon_extension::node::{self, Ext, Image, ImagePlan};
use dagon_extension::{gen, Known, Manifest, MANIFEST_FILE};
use dagon_topology::defs::boot_folder::source_sha256;
use dagon_topology::defs::{ImageKind, McuDef, PortKind, Registry};
use dagon_topology::schema::{Node, Role};
use serde::Deserialize;

use crate::paths::contained;

/// The folder of a node holding only what `dagon gen` writes here.
const GENERATED: &str = "dagon";
/// The project folder holding each node's boot crate (`boot/<id>/`), only
/// what `dagon gen` writes there.
const BOOT_CRATES: &str = "boot";
use crate::scaffold::{write, Written};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trusted {
    pub id: String,
    pub sha256: String,
}

/// One node's chains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeExtensions {
    #[serde(default)]
    pub boot: Vec<String>,
    #[serde(default)]
    pub app: Vec<String>,
    #[serde(default)]
    pub trusted: Vec<Trusted>,
}

/// `[extensions]` of dagon.toml.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extensions {
    /// Folders (or `folder/*`) relative to dagon.toml.
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeExtensions>,
}

/// A manifest found in the project, and the crate that provides it.
struct Found {
    manifest: Manifest,
    /// Package name and library name.
    package: String,
    krate: String,
    /// The package folder.
    dir: PathBuf,
}

/// Package folders `patterns` name: `dir` or `dir/*` (its subfolders).
fn folders(base: &Path, patterns: &[String]) -> Result<Vec<std::path::PathBuf>, String> {
    let mut out = Vec::new();
    for p in patterns {
        match p.strip_suffix("/*") {
            Some(dir) => {
                let dir = contained(base, Path::new(dir))?;
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut found: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                found.sort();
                out.extend(found);
            }
            None => out.push(contained(base, Path::new(p))?),
        }
    }
    Ok(out)
}

/// The package name and library name of the crate in `dir` (`[lib]
/// name`, else the package name with `-` as `_`).
fn names(dir: &Path) -> Result<(String, String), String> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let t: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let package = t
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{}: no package name", path.display()))?;
    let lib = t
        .get("lib")
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(package);
    Ok((package.to_string(), gen::lib_name(lib)))
}

fn discover(base: &Path, cfg: &Extensions, known: &Known) -> Result<Vec<Found>, String> {
    let mut out: Vec<Found> = Vec::new();
    for dir in folders(base, &cfg.packages)? {
        let path = dir.join(MANIFEST_FILE);
        if !path.is_file() {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest = Manifest::parse(&text, &path.display().to_string(), known)
            .map_err(|e| e.to_string())?;
        if out.iter().any(|f| f.manifest.id == manifest.id) {
            return Err(format!(
                "extension {} is defined twice in the project",
                manifest.id
            ));
        }
        let (package, krate) = names(&dir)?;
        out.push(Found {
            package,
            krate,
            manifest,
            dir,
        });
    }
    Ok(out)
}

fn chain<'a>(
    found: &'a [Found],
    ids: &[String],
    trusted: &[Trusted],
) -> Result<Vec<Ext<'a>>, String> {
    ids.iter()
        .map(|id| {
            let f = found.iter().find(|f| &f.manifest.id == id).ok_or_else(|| {
                format!("extension {id} is in no folder of [extensions] packages")
            })?;
            let sha256 = trusted
                .iter()
                .find(|t| &t.id == id)
                .map(|t| t.sha256.to_lowercase());
            if let Some(want) = &sha256 {
                let got = source_sha256(&f.dir)?;
                if &got != want {
                    return Err(format!(
                        "trusted extension {id}: its source hashes to {got}; dagon.toml declares {want}                          (declare the new hash once this source is the one to trust)"
                    ));
                }
            }
            Ok(Ext {
                manifest: &f.manifest,
                krate: f.krate.clone(),
                sha256,
            })
        })
        .collect()
}

/// The boot core runs the extensions of its chain privileged: only those
/// whose manifest says `trusted = true` and that dagon.toml declares
/// trusted with their source hash.
fn boot_trusted(exts: &[Ext<'_>], node: &str) -> Result<(), String> {
    match exts
        .iter()
        .find(|e| !e.manifest.trusted || e.sha256.is_none())
    {
        Some(e) => Err(format!(
            "boot extension {} is not trusted: the boot core runs an extension only when its manifest says \
             trusted = true and [extensions.nodes.{node}] trusted lists it with its source SHA-256",
            e.manifest.id
        )),
        None => Ok(()),
    }
}

/// The boot crate of node `n` (`nodes/<id>/dagon/`): its MCU's boot core
/// template with the node's identity, its trusted extensions as
/// dependencies and as `[package.metadata.dagon.boot]` (read by the boot
/// stages' builds, `dagon_topology::defs::boot_folder`).
fn boot_crate(
    n: &Node,
    board: Option<&str>,
    exts: &[Ext<'_>],
    found: &[Found],
    p: &ProjectRef<'_>,
) -> Result<Files, String> {
    let root = crate::dagonsrc::root(p.src, p.base)?;
    let template = crate::scaffold::boot_template(&root, &n.mcu)
        .ok_or_else(|| format!("the DAGON source has no boot core template for {}", n.mcu))?;
    let up = "../../";
    let mut deps = Vec::new();
    let mut meta = format!(
        "[package.metadata.dagon.boot]\nnode = {:?}\nmcu = {:?}\n{}project = \"{up}{}\"\n",
        n.id,
        n.mcu,
        board.map_or(String::new(), |b| format!("board = {b:?}\n")),
        p.config
    );
    for e in exts {
        let f = found
            .iter()
            .find(|f| f.manifest.id == e.manifest.id)
            .expect("chain() found it");
        let base =
            std::fs::canonicalize(p.base).map_err(|e| format!("{}: {e}", p.base.display()))?;
        let rel = f
            .dir
            .strip_prefix(&base)
            .or_else(|_| f.dir.strip_prefix(p.base))
            .map_err(|_| format!("{}: outside the project", f.dir.display()))?;
        let rel = format!("{up}{}", crate::config::forward_path(rel));
        deps.push(format!("{} = {{ path = {rel:?} }}", f.package));
        meta.push_str(&format!(
            "\n[[package.metadata.dagon.boot.trusted]]\nid = {:?}\npath = {rel:?}\nsha256 = {:?}\n",
            e.manifest.id,
            e.sha256.as_deref().unwrap_or_default()
        ));
    }
    let tag = n.tag.escape_debug().to_string();
    template
        .into_iter()
        .map(|(rel, text)| {
            let text = crate::scaffold::render_deps(&text, p.src, p.base)?
                .replace("{{boot_extensions}}", &deps.join("\n"))
                .replace("{{boot_metadata}}", meta.trim_end())
                .replace("{{node}}", &n.id)
                .replace("{{tag}}", &tag);
            Ok((format!("{BOOT_CRATES}/{}/{rel}", n.id), text))
        })
        .collect()
}

/// Where the project's DAGON crates come from and where it is.
pub struct ProjectRef<'a> {
    pub base: &'a Path,
    /// dagon.toml's file name in `base`.
    pub config: &'a str,
    pub src: &'a crate::scaffold::DagonSource,
}

fn has_boot_stage(def: &McuDef) -> bool {
    def.port == PortKind::SelfBoot && def.slots.iter().any(|s| s.image == ImageKind::Bootcore)
}

/// Files as (project-relative path, text).
type Files = Vec<(String, String)>;

/// The files of node `n`, and notes for the user; `Err` names why its set
/// of extensions cannot be generated.
fn plan_files(
    reg: &Registry,
    n: &Node,
    mine: &NodeExtensions,
    found: &[Found],
    known: &Known,
    p: &ProjectRef<'_>,
) -> Result<(Files, Vec<String>), String> {
    let def = reg
        .mcu(&n.mcu)
        .ok_or_else(|| format!("unknown MCU {}", n.mcu))?;
    let board = n.board.as_deref().and_then(|b| reg.board(b));
    let mut plans: Vec<ImagePlan<'_>> = Vec::new();
    let mut files = Vec::new();
    if has_boot_stage(def) {
        let exts = chain(found, &mine.boot, &mine.trusted)?;
        boot_trusted(&exts, &n.id)?;
        files.extend(boot_crate(n, n.board.as_deref(), &exts, found, p)?);
        plans.push(node::plan(def, board, Image::Bootcore, exts, known)?);
    } else if !mine.boot.is_empty() {
        return Err(format!(
            "{} has no DAGON boot stage (its vendor's second stage starts the app), so boot extensions cannot run there",
            def.id
        ));
    }
    let relay = n.role == Role::Coordinator;
    plans.push(node::plan(
        def,
        board,
        Image::App { relay },
        chain(found, &mine.app, &mine.trusted)?,
        known,
    )?);
    let dir = format!("nodes/{}/{GENERATED}", n.id);
    let mut notes = Vec::new();
    for p in &plans {
        if p.image == Image::Bootcore {
            files.push((
                format!("{dir}/boot_wiring.rs"),
                gen::boot_wiring(&n.id, def, board, p, known)?,
            ));
        }
        for (slot, space) in &p.memory {
            let slot_name = format!("{slot:?}").to_lowercase();
            files.push((
                format!("{dir}/memory-{}-{slot_name}.x", p.image.name()),
                gen::linker_fragment(&n.id, def, p, *slot, space),
            ));
        }
        notes.extend(p.notes.iter().map(|x| format!("note: {}: {x}", n.id)));
    }
    files.push((
        format!("{dir}/protection.toml"),
        gen::protection_map(&n.id, def, &plans),
    ));
    Ok((files, notes))
}

/// The mark every file `dagon gen` writes here carries in its first line.
const MARK: &str = "Generated by `dagon gen`";

/// Whether `path` starts with gen's mark (only such files are removed).
fn generated(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().next().map(|l| l.contains(MARK)))
        .unwrap_or(false)
}

/// Removes every file in node `id`'s generated folder that this run did
/// not write (`keep`, project-relative paths) and that carries gen's mark,
/// so no build reads a map, fragment or wiring an earlier run left;
/// returns the lines reporting each removal and each unmarked file kept.
/// Callers have checked the folder is no link ([`linked`]).
fn prune(base: &Path, rel: &str, keep: &[String]) -> Result<Vec<String>, String> {
    let dir = contained(base, Path::new(&rel))?;
    let mut names = Vec::new();
    files_in(&dir, "", &mut names);
    names.retain(|n| !keep.contains(&format!("{rel}/{n}")));
    names.sort();
    let mut out = Vec::new();
    for name in names {
        if !generated(&dir.join(&name)) {
            out.push(format!(
                "{:<10} {rel}/{name} (not written by dagon gen)",
                "kept"
            ));
            continue;
        }
        crate::genfs::remove(base, &crate::genfs::Rel::new(format!("{rel}/{name}"))?)?;
        out.push(format!("{:<10} {rel}/{name}", "removed"));
    }
    Ok(out)
}

/// The files under `dir` (`rel/`-prefixed, `/`-separated), leaving out
/// the boot crate's build output (`target/`) and the lock file cargo
/// writes (`Cargo.lock`, not gen's: it carries no mark and is kept).
fn files_in(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let r = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        match e.file_type() {
            Ok(t) if t.is_file() && name != "Cargo.lock" => out.push(r),
            Ok(t) if t.is_dir() && name != "target" => files_in(&e.path(), &r, out),
            _ => {}
        }
    }
}

/// Writes the extension files of the nodes `only` names (all when
/// empty) and removes whatever else their generated folders hold; nodes
/// in `failed` (their crates were not generated) keep no generated files.
/// Returns gen's report lines (written, skipped with the refusal, removed,
/// notes).
pub fn write_nodes(
    reg: &Registry,
    p: &ProjectRef<'_>,
    nodes: &[Node],
    only: &[String],
    failed: &[String],
    cfg: &Extensions,
) -> Result<Vec<String>, String> {
    if let Some(id) = cfg
        .nodes
        .keys()
        .find(|id| !nodes.iter().any(|n| &n.id == *id))
    {
        return Err(format!(
            "[extensions.nodes.{id}]: no such node in the topology"
        ));
    }
    let base = p.base;
    let known = Known::dagon();
    let found = discover(base, cfg, &known)?;
    let none = NodeExtensions::default();
    let mut lines = Vec::new();
    for n in nodes
        .iter()
        .filter(|n| only.is_empty() || only.contains(&n.id))
    {
        let mine = cfg.nodes.get(&n.id).unwrap_or(&none);
        let folders = [
            format!("nodes/{}/{GENERATED}", n.id),
            format!("{BOOT_CRATES}/{}", n.id),
        ];
        let mut linked = None;
        for f in &folders {
            if let Some(link) = crate::genfs::link_in(base, &crate::genfs::Rel::new(f)?) {
                linked.get_or_insert((f, link));
            }
        }
        if let Some((f, link)) = linked {
            // A node whose crate gen skipped is already reported.
            if failed.contains(&n.id) {
                continue;
            }
            lines.push(format!(
                "skipped    {f}: {}",
                crate::genfs::link_reason(&link)
            ));
            continue;
        }
        let mut written: Vec<Written> = Vec::new();
        if !failed.contains(&n.id) {
            match plan_files(reg, n, mine, &found, &known, p) {
                Ok((files, notes)) => {
                    for (rel, text) in files {
                        write(base, &rel, &text, true, &mut written)?;
                    }
                    lines.extend(
                        written
                            .iter()
                            .map(|w| format!("{:<10} {}", w.action, w.path)),
                    );
                    lines.extend(notes);
                }
                Err(why) => lines.push(format!("skipped    nodes/{}/{GENERATED}: {why}", n.id)),
            }
        }
        let keep: Vec<String> = written.into_iter().map(|w| w.path).collect();
        for f in &folders {
            lines.extend(prune(base, f, &keep)?);
        }
    }
    Ok(lines)
}

#[cfg(test)]
// Tests set up and inspect project folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn node(id: &str, mcu: &str, board: &str, role: &str) -> Node {
        serde_json::from_value(serde_json::json!({
            "id": id, "mcu": mcu, "board": board, "role": role, "port": "self-boot", "tag": id,
        }))
        .unwrap()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-extgen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // An untrusted logger (app chains) and a trusted boot observer.
        for (dir, package, manifest) in [
            (
                "logger",
                "boot-logger",
                "format = 1\nid = \"logger\"\nlayers = [\"boot\", \"app\"]\nobserves = [\"boot.*\"]\n\
                 [limits]\nmax_run_us = 500\nstack_bytes = 512\nram_bytes = 256\ntext_bytes = 2048\n",
            ),
            (
                "show",
                "boot-show",
                "format = 1\nid = \"show\"\nlayers = [\"boot\"]\nobserves = [\"boot.*\"]\ntrusted = true\n\
                 [limits]\nmax_run_us = 2000\n",
            ),
        ] {
            let e = d.join("extensions").join(dir);
            std::fs::create_dir_all(e.join("src")).unwrap();
            std::fs::write(
                e.join("Cargo.toml"),
                format!("[package]\nname = {package:?}\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
            std::fs::write(e.join("src/lib.rs"), "#![no_std]\n").unwrap();
            std::fs::write(e.join(MANIFEST_FILE), manifest).unwrap();
        }
        d
    }

    /// The DAGON source these tests take the boot core templates from.
    fn dagon_src() -> crate::scaffold::DagonSource {
        crate::scaffold::DagonSource {
            path: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")),
            ..Default::default()
        }
    }

    fn write(
        base: &Path,
        nodes: &[Node],
        only: &[String],
        failed: &[String],
        cfg: &Extensions,
    ) -> Result<Vec<String>, String> {
        let src = dagon_src();
        let p = ProjectRef {
            base,
            config: "dagon.toml",
            src: &src,
        };
        write_nodes(Registry::builtin(), &p, nodes, only, failed, cfg)
    }

    /// Node seg's chains: `boot` and `app`; `show` is declared trusted
    /// with the hash of its source as it is now.
    fn cfg_with(base: &Path, boot: &[&str], app: &[&str]) -> Extensions {
        let mut nodes = BTreeMap::new();
        let sha256 = source_sha256(&base.join("extensions/show")).unwrap();
        nodes.insert(
            "seg".to_string(),
            NodeExtensions {
                boot: boot.iter().map(|s| s.to_string()).collect(),
                app: app.iter().map(|s| s.to_string()).collect(),
                trusted: vec![Trusted {
                    id: "show".into(),
                    sha256,
                }],
            },
        );
        Extensions {
            packages: vec!["extensions/*".into()],
            nodes,
        }
    }

    fn cfg(base: &Path, boot: &[&str]) -> Extensions {
        cfg_with(base, boot, &[])
    }

    #[test]
    fn gen_writes_the_boot_crate_wiring_fragments_and_the_map_per_node() {
        let base = scratch("ok");
        let nodes = [
            node("seg", "stm32f401re", "nucleo-f401re", "node"),
            node(
                "hub",
                "rp2040",
                "waveshare-rp2040-touch-lcd-1.28",
                "coordinator",
            ),
            node("watch", "esp32s3", "m5stopwatch", "node"),
        ];
        let cfg = cfg_with(&base, &["show"], &["logger"]);
        let lines = write(&base, &nodes, &[], &[], &cfg).unwrap();
        let has = |p: &str| lines.iter().any(|l| l.ends_with(p));
        for p in [
            "nodes/seg/dagon/boot_wiring.rs",
            "boot/seg/Cargo.toml",
            "boot/seg/boot.rs",
            "boot/seg/.cargo/config.toml",
            "nodes/seg/dagon/memory-boot-a.x",
            "nodes/seg/dagon/memory-app-a.x",
            "nodes/seg/dagon/memory-app-b.x",
            "nodes/seg/dagon/protection.toml",
            "nodes/hub/dagon/boot_wiring.rs",
            "boot/hub/Cargo.toml",
            "nodes/hub/dagon/memory-boot-b.x",
            "nodes/watch/dagon/protection.toml",
        ] {
            assert!(has(p), "{p}: {lines:?}");
        }
        // No boot stage, no RAM in the definition: no boot crate, no
        // wiring, no fragments.
        assert!(!has("nodes/watch/dagon/boot_wiring.rs"));
        assert!(!lines.iter().any(|l| l.contains("boot/watch")));
        assert!(!lines
            .iter()
            .any(|l| l.contains("nodes/watch/dagon/memory-")));
        let wiring = std::fs::read_to_string(base.join("nodes/seg/dagon/boot_wiring.rs")).unwrap();
        assert!(
            wiring.contains("dagon_events::Chain<boot_show::DagonExtension, dagon_events::Nop>"),
            "{wiring}"
        );
        assert!(
            wiring.contains(
                "dagon_events::BootExtension::new(2000, &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0xf0]), // boot.*"
            ),
            "{wiring}"
        );
        // The trusted extension runs in DAGON's regions: no map at boot; the
        // app's sandboxed logger has its regions.
        let x = std::fs::read_to_string(base.join("nodes/seg/dagon/memory-boot-a.x")).unwrap();
        assert!(!x.contains("EXT0_TEXT"), "{x}");
        let x = std::fs::read_to_string(base.join("nodes/seg/dagon/memory-app-a.x")).unwrap();
        assert!(x.contains("EXT0_TEXT"), "{x}");
        // The boot crate names the node, its board and the trusted
        // extension as the boot stages' builds read them, and links it.
        let toml = std::fs::read_to_string(base.join("boot/seg/Cargo.toml")).unwrap();
        assert!(toml.starts_with("# Generated by `dagon gen`"), "{toml}");
        assert!(
            toml.contains("boot-show = { path = \"../../extensions/show\" }"),
            "{toml}"
        );
        assert!(
            toml.contains("dagon-stm32f401re-bootcore = { path = "),
            "{toml}"
        );
        let f =
            dagon_topology::defs::boot_folder::BootFolder::read(&base.join("boot/seg")).unwrap();
        assert_eq!(
            (
                f.meta.node.as_str(),
                f.meta.mcu.as_str(),
                f.meta.board.as_deref()
            ),
            ("seg", "stm32f401re", Some("nucleo-f401re"))
        );
        assert_eq!(f.meta.trusted.len(), 1);
        assert!(f.project().unwrap().ends_with("dagon.toml"));
        f.check_hashes().unwrap();
        let config = std::fs::read_to_string(base.join("boot/seg/.cargo/config.toml")).unwrap();
        assert!(config.contains("DAGON_TARGET_TAG = \"seg\""), "{config}");
        assert!(config.contains("DAGON_BOOT_CRATE"), "{config}");
        // A file of an earlier run this run does not write (a slot the MCU
        // no longer has) is removed; the rest stay as written, and what
        // cargo writes into the boot crate is never touched.
        std::fs::write(
            base.join("nodes/seg/dagon/memory-app-c.x"),
            "/* Generated by `dagon gen` for an earlier slot. */
",
        )
        .unwrap();
        std::fs::create_dir_all(base.join("boot/seg/target")).unwrap();
        std::fs::write(
            base.join("boot/seg/target/x"),
            "# Generated by `dagon gen`\n",
        )
        .unwrap();
        std::fs::write(base.join("boot/seg/Cargo.lock"), "lock\n").unwrap();
        let again = write(&base, &nodes, &["seg".into()], &[], &cfg).unwrap();
        assert!(
            again.contains(&"removed    nodes/seg/dagon/memory-app-c.x".to_string()),
            "{again:?}"
        );
        assert!(!base.join("nodes/seg/dagon/memory-app-c.x").exists());
        assert!(base.join("boot/seg/target/x").exists());
        assert!(base.join("boot/seg/Cargo.lock").exists());
        // A node whose crate gen skipped keeps no generated file.
        let gone = write(&base, &nodes, &["hub".into()], &["hub".into()], &cfg).unwrap();
        assert!(
            gone.iter()
                .all(|l| l.starts_with("removed    nodes/hub/dagon/")
                    || l.starts_with("removed    boot/hub/")),
            "{gone:?}"
        );
        assert!(gone.iter().any(|l| l.ends_with("boot_wiring.rs")));
        assert!(gone.iter().any(|l| l.ends_with(".cargo/config.toml")));
        assert!(!base.join("nodes/hub/dagon/protection.toml").exists());
        assert!(base.join("nodes/seg/dagon/protection.toml").exists());
        // Unchanged on a second run.
        let again = write(&base, &nodes, &["seg".into()], &[], &cfg).unwrap();
        assert!(
            again.iter().all(|l| l.starts_with("unchanged")),
            "{again:?}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn without_extensions_the_boot_crate_links_none() {
        let base = scratch("none");
        let seg = [node(
            "seg",
            "rp2040",
            "waveshare-rp2040-touch-lcd-1.28",
            "coordinator",
        )];
        write(&base, &seg, &[], &[], &cfg(&base, &[])).unwrap();
        let wiring = std::fs::read_to_string(base.join("nodes/seg/dagon/boot_wiring.rs")).unwrap();
        assert!(
            wiring.contains("pub type BootChain = dagon_events::Nop;"),
            "{wiring}"
        );
        let f =
            dagon_topology::defs::boot_folder::BootFolder::read(&base.join("boot/seg")).unwrap();
        assert!(f.meta.trusted.is_empty());
        f.check_hashes().unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gen_refuses_a_set_that_does_not_fit_by_name() {
        let base = scratch("refused");
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        write(&base, &seg, &[], &[], &cfg(&base, &["show"])).unwrap();
        assert!(base.join("nodes/seg/dagon/boot_wiring.rs").is_file());
        // A refused set removes what the previous gen wrote for the node.
        let lines = write(&base, &seg, &[], &[], &cfg(&base, &["nope"])).unwrap();
        assert_eq!(
            lines[0],
            "skipped    nodes/seg/dagon: extension nope is in no folder of [extensions] packages"
        );
        assert!(
            lines[1..].iter().all(|l| l.starts_with("removed")),
            "{lines:?}"
        );
        assert!(lines
            .iter()
            .any(|l| l.ends_with("nodes/seg/dagon/memory-boot-a.x")));
        assert!(!base.join("nodes/seg/dagon/boot_wiring.rs").exists());
        assert!(!base.join("boot/seg/Cargo.toml").exists());
        assert!(!base.join("nodes/seg/dagon/protection.toml").exists());
        // The boot core runs trusted extensions only.
        let lines = write(&base, &seg, &[], &[], &cfg(&base, &["logger"])).unwrap();
        assert!(
            lines[0].contains("boot extension logger is not trusted"),
            "{lines:?}"
        );
        // A trusted extension whose source changed after dagon.toml
        // declared its hash.
        let cfg = cfg(&base, &["show"]);
        std::fs::write(
            base.join("extensions/show/src/lib.rs"),
            "#![no_std]\n// more\n",
        )
        .unwrap();
        let lines = write(&base, &seg, &[], &[], &cfg).unwrap();
        assert!(
            lines[0].contains("trusted extension show") && lines[0].contains("hashes to"),
            "{lines:?}"
        );
        // A trusted manifest dagon.toml does not declare.
        let mut undeclared = cfg_with(&base, &["show"], &[]);
        undeclared.nodes.get_mut("seg").unwrap().trusted.clear();
        let lines = write(&base, &seg, &[], &[], &undeclared).unwrap();
        assert!(
            lines[0].contains("boot extension show is not trusted"),
            "{lines:?}"
        );
        // No DAGON boot stage: the reason is named.
        let watch = [node("seg", "esp32s3", "m5stopwatch", "node")];
        let lines = write(&base, &watch, &[], &[], &cfg_with(&base, &["show"], &[])).unwrap();
        assert!(
            lines[0].contains(
                "esp32s3 has no DAGON boot stage (its vendor's second stage starts the app)"
            ),
            "{lines:?}"
        );
        let e = write(&base, &[], &[], &[], &cfg_with(&base, &[], &[])).unwrap_err();
        assert!(e.contains("[extensions.nodes.seg]"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `nodes/seg/dagon` made a link to `<base>/src` holding a user file;
    /// `make` returns false where the OS does not allow that link kind.
    fn through_a_link(name: &str, make: fn(&Path, &Path) -> bool) {
        let base = scratch(name);
        std::fs::create_dir_all(base.join("src")).unwrap();
        std::fs::write(base.join("src/user.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(base.join("nodes/seg")).unwrap();
        if !make(&base.join("src"), &base.join("nodes/seg/dagon")) {
            let _ = std::fs::remove_dir_all(&base);
            return;
        }
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        let lines = write(&base, &seg, &[], &[], &cfg(&base, &["show"])).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("nodes/seg/dagon is a link"), "{lines:?}");
        // dagon gen ends non-zero, naming the node and the reason.
        let e = crate::scaffold::outcome(&lines).unwrap_err();
        assert!(
            e.contains("nodes/seg/dagon") && e.contains("is a link"),
            "{e}"
        );
        // A node the crate step already reported skipped is not reported
        // again, and nothing goes through the link either.
        let again = write(&base, &seg, &[], &["seg".into()], &cfg(&base, &["show"])).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(std::fs::read_dir(base.join("src")).unwrap().count(), 1);
        assert!(base.join("src/user.rs").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gen_never_writes_or_removes_through_a_junction() {
        through_a_link("junction", |target, link| {
            if cfg!(windows) {
                let ok = std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(link.components().collect::<std::path::PathBuf>())
                    .arg(target.components().collect::<std::path::PathBuf>())
                    .output()
                    .unwrap()
                    .status
                    .success();
                assert!(ok, "mklink /J");
                true
            } else {
                false
            }
        });
    }

    #[test]
    fn gen_never_writes_or_removes_through_a_symlink() {
        through_a_link("symlink", |target, link| {
            #[cfg(unix)]
            return std::os::unix::fs::symlink(target, link).is_ok();
            // Windows allows directory symlinks only with the privilege.
            #[cfg(windows)]
            return std::os::windows::fs::symlink_dir(target, link).is_ok();
        });
    }

    #[test]
    fn gen_removes_only_files_it_wrote() {
        let base = scratch("unmarked");
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        write(&base, &seg, &[], &[], &cfg(&base, &["show"])).unwrap();
        std::fs::write(base.join("nodes/seg/dagon/notes.md"), "mine\n").unwrap();
        let lines = write(&base, &seg, &[], &["seg".into()], &cfg(&base, &[])).unwrap();
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("kept       nodes/seg/dagon/notes.md")),
            "{lines:?}"
        );
        assert!(base.join("nodes/seg/dagon/notes.md").is_file());
        assert!(!base.join("nodes/seg/dagon/boot_wiring.rs").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
