//! The extension and protection files `dagon gen` writes into
//! `nodes/<id>/dagon/`: `boot_wiring.rs` (MCUs with a DAGON boot stage),
//! one linker fragment per image slot (`memory-<image>-<slot>.x`, MCUs that
//! define their RAM) and the readable `protection.toml`.
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
use std::path::Path;

use dagon_extension::node::{self, Ext, Image, ImagePlan};
use dagon_extension::{gen, Known, Manifest, MANIFEST_FILE};
use dagon_topology::defs::{ImageKind, McuDef, PortKind, Registry};
use dagon_topology::schema::{Node, Role};
use serde::Deserialize;

use crate::paths::contained;

/// The folder of a node holding only what `dagon gen` writes here.
const GENERATED: &str = "dagon";
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
    krate: String,
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

/// The library name of the crate in `dir` (`[lib] name`, else the package
/// name with `-` as `_`).
fn lib_name(dir: &Path) -> Result<String, String> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let t: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let name = t
        .get("lib")
        .and_then(|l| l.get("name"))
        .or_else(|| t.get("package").and_then(|p| p.get("name")))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{}: no package name", path.display()))?;
    Ok(gen::lib_name(name))
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
        out.push(Found {
            krate: lib_name(&dir)?,
            manifest,
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
            Ok(Ext {
                manifest: &f.manifest,
                krate: f.krate.clone(),
                sha256: trusted
                    .iter()
                    .find(|t| &t.id == id)
                    .map(|t| t.sha256.to_lowercase()),
            })
        })
        .collect()
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
) -> Result<(Files, Vec<String>), String> {
    let def = reg
        .mcu(&n.mcu)
        .ok_or_else(|| format!("unknown MCU {}", n.mcu))?;
    let board = n.board.as_deref().and_then(|b| reg.board(b));
    let mut plans: Vec<ImagePlan<'_>> = Vec::new();
    if has_boot_stage(def) {
        plans.push(node::plan(
            def,
            board,
            Image::Bootcore,
            chain(found, &mine.boot, &mine.trusted)?,
            known,
        )?);
    } else if !mine.boot.is_empty() {
        return Err(format!(
            "{} has no DAGON boot stage; boot extensions cannot run there",
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
    let mut files = Vec::new();
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
fn prune(base: &Path, id: &str, keep: &[String]) -> Result<Vec<String>, String> {
    let rel = format!("nodes/{id}/{GENERATED}");
    let dir = contained(base, Path::new(&rel))?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !keep.contains(&format!("{rel}/{n}")))
        .collect();
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
        crate::genfs::remove(base, &format!("{rel}/{name}"))?;
        out.push(format!("{:<10} {rel}/{name}", "removed"));
    }
    Ok(out)
}

/// Writes the extension files of the nodes `only` names (all when
/// empty) and removes whatever else their generated folders hold; nodes
/// in `failed` (their crates were not generated) keep no generated files.
/// Returns gen's report lines (written, skipped with the refusal, removed,
/// notes).
pub fn write_nodes(
    reg: &Registry,
    base: &Path,
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
    let known = Known::dagon();
    let found = discover(base, cfg, &known)?;
    let none = NodeExtensions::default();
    let mut lines = Vec::new();
    for n in nodes
        .iter()
        .filter(|n| only.is_empty() || only.contains(&n.id))
    {
        let mine = cfg.nodes.get(&n.id).unwrap_or(&none);
        if let Some(link) = crate::genfs::link_in(base, &format!("nodes/{}/{GENERATED}", n.id))? {
            // A node whose crate gen skipped is already reported.
            if failed.contains(&n.id) {
                continue;
            }
            lines.push(format!(
                "skipped    nodes/{}/{GENERATED}: {}",
                n.id,
                crate::genfs::link_reason(&link)
            ));
            continue;
        }
        let mut written: Vec<Written> = Vec::new();
        if !failed.contains(&n.id) {
            match plan_files(reg, n, mine, &found, &known) {
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
        lines.extend(prune(base, &n.id, &keep)?);
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
        std::fs::create_dir_all(d.join("extensions/logger")).unwrap();
        std::fs::write(
            d.join("extensions/logger/Cargo.toml"),
            "[package]
name = \"boot-logger\"
version = \"0.1.0\"
",
        )
        .unwrap();
        std::fs::write(
            d.join("extensions/logger").join(MANIFEST_FILE),
            "format = 1
id = \"logger\"
layers = [\"boot\", \"app\"]
observes = [\"boot.*\"]
             [limits]
max_run_us = 500
stack_bytes = 512
ram_bytes = 256
text_bytes = 2048
",
        )
        .unwrap();
        d
    }

    fn cfg(boot: &[&str]) -> Extensions {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "seg".to_string(),
            NodeExtensions {
                boot: boot.iter().map(|s| s.to_string()).collect(),
                ..NodeExtensions::default()
            },
        );
        Extensions {
            packages: vec!["extensions/*".into()],
            nodes,
        }
    }

    #[test]
    fn gen_writes_wiring_fragments_and_the_map_per_node() {
        let base = scratch("ok");
        let reg = Registry::builtin();
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
        let lines = write_nodes(reg, &base, &nodes, &[], &[], &cfg(&["logger"])).unwrap();
        let has = |p: &str| lines.iter().any(|l| l.ends_with(p));
        for p in [
            "nodes/seg/dagon/boot_wiring.rs",
            "nodes/seg/dagon/memory-boot-a.x",
            "nodes/seg/dagon/memory-app-a.x",
            "nodes/seg/dagon/memory-app-b.x",
            "nodes/seg/dagon/protection.toml",
            "nodes/hub/dagon/boot_wiring.rs",
            "nodes/hub/dagon/memory-boot-b.x",
            "nodes/watch/dagon/protection.toml",
        ] {
            assert!(has(p), "{p}: {lines:?}");
        }
        // No boot stage, no RAM in the definition: no wiring, no fragments.
        assert!(!has("nodes/watch/dagon/boot_wiring.rs"));
        assert!(!lines
            .iter()
            .any(|l| l.contains("nodes/watch/dagon/memory-")));
        let wiring = std::fs::read_to_string(base.join("nodes/seg/dagon/boot_wiring.rs")).unwrap();
        assert!(
            wiring.contains("dagon_events::Chain<boot_logger::DagonExtension, dagon_events::Nop>")
        );
        assert!(wiring.contains("(\"logger\", &["));
        let x = std::fs::read_to_string(base.join("nodes/seg/dagon/memory-boot-a.x")).unwrap();
        assert!(x.contains("EXT0_TEXT"), "{x}");
        // A file of an earlier run this run does not write (a slot the MCU
        // no longer has) is removed; the rest stay as written.
        std::fs::write(
            base.join("nodes/seg/dagon/memory-app-c.x"),
            "/* Generated by `dagon gen` for an earlier slot. */
",
        )
        .unwrap();
        let again =
            write_nodes(reg, &base, &nodes, &["seg".into()], &[], &cfg(&["logger"])).unwrap();
        assert!(
            again.contains(&"removed    nodes/seg/dagon/memory-app-c.x".to_string()),
            "{again:?}"
        );
        assert!(!base.join("nodes/seg/dagon/memory-app-c.x").exists());
        // A node whose crate gen skipped keeps no generated file.
        let gone = write_nodes(
            reg,
            &base,
            &nodes,
            &["hub".into()],
            &["hub".into()],
            &cfg(&[]),
        )
        .unwrap();
        assert!(
            gone.iter()
                .all(|l| l.starts_with("removed    nodes/hub/dagon/")),
            "{gone:?}"
        );
        assert!(gone.iter().any(|l| l.ends_with("boot_wiring.rs")));
        assert!(!base.join("nodes/hub/dagon/protection.toml").exists());
        assert!(base.join("nodes/seg/dagon/protection.toml").exists());
        // Unchanged on a second run.
        let again =
            write_nodes(reg, &base, &nodes, &["seg".into()], &[], &cfg(&["logger"])).unwrap();
        assert!(
            again.iter().all(|l| l.starts_with("unchanged")),
            "{again:?}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gen_refuses_a_set_that_does_not_fit_by_name() {
        let base = scratch("refused");
        let reg = Registry::builtin();
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        write_nodes(reg, &base, &seg, &[], &[], &cfg(&["logger"])).unwrap();
        assert!(base.join("nodes/seg/dagon/boot_wiring.rs").is_file());
        // A refused set removes what the previous gen wrote for the node.
        let lines = write_nodes(reg, &base, &seg, &[], &[], &cfg(&["nope"])).unwrap();
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
        assert!(!base.join("nodes/seg/dagon/protection.toml").exists());
        let watch = [node("seg", "esp32s3", "m5stopwatch", "node")];
        let lines = write_nodes(reg, &base, &watch, &[], &[], &cfg(&["logger"])).unwrap();
        assert!(
            lines[0].contains("esp32s3 has no DAGON boot stage"),
            "{lines:?}"
        );
        let e = write_nodes(reg, &base, &[], &[], &[], &cfg(&[])).unwrap_err();
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
        let reg = Registry::builtin();
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        let lines = write_nodes(reg, &base, &seg, &[], &[], &cfg(&["logger"])).unwrap();
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
        let again = write_nodes(reg, &base, &seg, &[], &["seg".into()], &cfg(&["logger"])).unwrap();
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
        let reg = Registry::builtin();
        let seg = [node("seg", "stm32f401re", "nucleo-f401re", "node")];
        write_nodes(reg, &base, &seg, &[], &[], &cfg(&["logger"])).unwrap();
        std::fs::write(base.join("nodes/seg/dagon/notes.md"), "mine\n").unwrap();
        let lines = write_nodes(reg, &base, &seg, &[], &["seg".into()], &cfg(&[])).unwrap();
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
