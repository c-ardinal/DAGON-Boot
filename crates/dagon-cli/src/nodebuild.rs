//! `dagon build`: per node `cargo build` following `[build.<id>]` in
//! dagon.toml, producing the ELF files `dagon image sign` takes:
//! `out/build/<id>/app-a.elf` and, with `slot_b_feature`, `app-b.elf`.
//!
//! Each build starts from an empty `out/build/<id>/`; `build.json` is
//! written last and only when every slot built, naming the outputs, their
//! SHA-256 and the version. Signing takes only what that record lists, so
//! a failed build or a slot no longer configured leaves nothing to sign.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::BuildArgs;
use crate::config::{BuildInputs, Config};

const OUT_DIR: &str = "out/build";
const RECORD: &str = "build.json";
/// The public key file the firmware build scripts embed.
pub const PUBKEY_ENV: &str = "DAGON_PUBKEY";

/// Secrets the CLI holds that a build (build scripts, proc macros) must not
/// see: the signing key path and every agent key a way names.
pub fn secret_env() -> Vec<String> {
    crate::agents::secret_env(crate::consent::home().ok().as_deref())
}

/// What the last successful build of a node produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRecord {
    pub version: Option<String>,
    /// (slot letter, SHA-256 hex of the ELF).
    pub slots: Vec<(char, String)>,
}
const POLL: Duration = Duration::from_millis(100);

/// One node's build outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub node: String,
    /// (slot letter, ELF path, size in bytes).
    pub slots: Vec<(char, PathBuf, u64)>,
}

pub fn elf_path(cfg: &Config, node: &str, slot: char) -> PathBuf {
    node_dir(cfg, node).join(format!("app-{slot}.elf"))
}

fn node_dir(cfg: &Config, node: &str) -> PathBuf {
    cfg.base.join(OUT_DIR).join(node)
}

fn sha_hex(b: &[u8]) -> String {
    dagon_sign::to_hex(&dagon_format::sha256(b))
}

/// (slot letter, ELF bytes) of a recorded build.
pub type SlotElfs = Vec<(char, Vec<u8>)>;

/// The ELF files of `node`'s last successful build, checked against its
/// record: (slot, bytes) and the recorded version.
pub fn last_build(cfg: &Config, node: &str) -> Result<(BuildRecord, SlotElfs), String> {
    let dir = crate::paths::contained(&cfg.base, &node_dir(cfg, node))?;
    let text = std::fs::read_to_string(dir.join(RECORD))
        .map_err(|_| format!("{node} has no successful build; run the build first"))?;
    let rec: BuildRecord =
        serde_json::from_str(&text).map_err(|e| format!("{node}: bad build record: {e}"))?;
    let mut out = Vec::new();
    for (slot, sha) in &rec.slots {
        let path = crate::paths::contained(&cfg.base, &elf_path(cfg, node, *slot))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if sha_hex(&bytes) != *sha {
            return Err(format!("{} changed since it was built", path.display()));
        }
        out.push((*slot, bytes));
    }
    Ok((rec, out))
}

/// Builds `nodes` (all configured nodes when empty). `log` gets cargo's
/// output line by line; `cancel` stops the running cargo.
pub fn build_nodes(
    cfg: &Config,
    nodes: &[String],
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<Built>, String> {
    // Checked once; the firmware gets these bytes, not the file again.
    let key = crate::config::build_pubkey(cfg)?;
    build_nodes_keyed(cfg, nodes, key, None, log, cancel)
}

/// [`build_nodes`] embedding `key` (none: DAGON_PUBKEY is not passed down).
/// A package may also lie inside `source` (the project's DAGON source, for
/// its samples), not only inside the work area.
pub fn build_nodes_keyed(
    cfg: &Config,
    nodes: &[String],
    key: Option<[u8; 32]>,
    source: Option<&Path>,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<Built>, String> {
    let ids: Vec<String> = if nodes.is_empty() {
        cfg.build.keys().cloned().collect()
    } else {
        nodes.to_vec()
    };
    // Before any build: 中止 of 雛形を作る leaves tables for crates not made yet.
    let missing: Vec<&str> = ids
        .iter()
        .filter(|id| {
            cfg.build
                .get(*id)
                .is_some_and(|b| !cfg.base.join(&b.package).join("Cargo.toml").is_file())
                && !source.is_some_and(|s| {
                    cfg.build[*id].package.is_relative()
                        && s.join(&cfg.build[*id].package).join("Cargo.toml").is_file()
                })
        })
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "no crate yet for {}; make them again with 雛形を作る (dagon gen)",
            missing.join(", ")
        ));
    }
    let key = key.map(|k| dagon_sign::to_hex(&k));
    let mut out = Vec::new();
    for id in ids {
        let inputs = cfg
            .build
            .get(&id)
            .ok_or_else(|| format!("no [build.{id}] in dagon.toml"))?;
        out.push(build_node(
            cfg,
            &id,
            inputs,
            key.as_deref(),
            source,
            log,
            cancel,
        )?);
    }
    Ok(out)
}

/// Empties node `id`'s output folder; its path relative to the work area.
/// The folder is DAGON's: cleared (and filled) through genfs, so a link on
/// the way (out, out/build or the node's folder) is refused, not followed.
fn clear_output(cfg: &Config, id: &str) -> Result<crate::genfs::Rel, String> {
    let out_rel = crate::genfs::Rel::new(format!("{OUT_DIR}/{id}"))?;
    crate::genfs::remove_tree(&cfg.base, &out_rel)
        .map_err(|e| format!("[build.{id}] output: {e}"))?;
    Ok(out_rel)
}

fn build_node(
    cfg: &Config,
    id: &str,
    b: &BuildInputs,
    pubkey_hex: Option<&str>,
    source: Option<&Path>,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Built, String> {
    // Checked through its canonical form, then passed on without the
    // Windows `\\?\` prefix: MSVC's link.exe cannot open object files
    // under such a --target-dir (LNK1181 for host build scripts).
    let dir = package_dir(cfg, b, source)
        .map(crate::config::plain_path)
        .map_err(|e| format!("[build.{id}]{e}"))?;
    let bin = match &b.bin {
        Some(n) => n.clone(),
        None => package_name(&dir)?,
    };
    let mut slots = vec![('a', b.features.clone(), None)];
    if let Some(f) = &b.slot_b_feature {
        let feats = b.features.iter().cloned().chain([f.clone()]).collect();
        slots.push(('b', feats, Some(dir.join("target").join("slot-b"))));
    }
    let link_baud = link_baud(cfg, id)?;
    let out_rel = clear_output(cfg, id)?;
    let mut built = Vec::new();
    let version = crate::srcver::crate_version(&dir)?;
    let mut record = BuildRecord {
        version: Some(version.to_string()),
        slots: Vec::new(),
    };
    for (slot, features, target_dir) in slots {
        log(&format!(
            "{id}: building slot {slot} in {}",
            b.package.display()
        ));
        let exe = cargo_build(
            &dir,
            &bin,
            &features,
            target_dir.as_deref(),
            FirmwareEnv {
                pubkey_hex,
                target_tag: None,
                boot_crate: None,
                link_baud,
            },
            log,
            cancel,
        )?;
        let dest = elf_path(cfg, id, slot);
        crate::genfs::copy(&cfg.base, &exe, &out_rel.join(format!("app-{slot}.elf"))?)
            .map_err(|e| format!("[build.{id}] output: {e}"))?;
        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        log(&format!(
            "{id}: slot {slot} -> {} ({size} bytes)",
            dest.display()
        ));
        let bytes = std::fs::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        record.slots.push((slot, sha_hex(&bytes)));
        built.push((slot, dest, size));
    }
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    crate::genfs::write(&cfg.base, &out_rel.join(RECORD)?, text.as_bytes())?;
    Ok(Built {
        node: id.into(),
        slots: built,
    })
}

/// `b`'s package folder, resolved through symlinks and junctions: inside
/// the work area, or inside `source` when given.
fn package_dir(cfg: &Config, b: &BuildInputs, source: Option<&Path>) -> Result<PathBuf, String> {
    let in_source =
        source.and_then(|s| crate::paths::contained(s, &cfg.base.join(&b.package)).ok());
    if let Some(dir) = in_source {
        b.validate_names().map_err(|e| format!(": {e}"))?;
        return Ok(dir);
    }
    b.validate().map_err(|e| format!(": {e}"))?;
    crate::paths::contained(&cfg.base, &b.package).map_err(|e| format!(" package: {e}"))
}

pub(crate) fn package_name(dir: &Path) -> Result<String, String> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    v.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| format!("{}: no package name", path.display()))
}

/// Settings a parent cargo or rustup (dagon run through `cargo run` or a
/// test) would pass down, overriding each node crate's own toolchain file,
/// target folder and rustflags.
const INHERITED_BUILD_ENV: [&str; 7] = [
    "RUSTUP_TOOLCHAIN",
    "RUSTC",
    "RUSTC_WRAPPER",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
];

/// Removes the CLI's secrets, and build settings inherited from a parent
/// cargo, from a child's environment.
fn scrub(cmd: &mut Command) {
    for k in secret_env() {
        cmd.env_remove(k);
    }
    for k in INHERITED_BUILD_ENV {
        cmd.env_remove(k);
    }
}

enum Line {
    Out(String),
    Err(String),
}

/// The node tag a boot core or stage0 build embeds (apps take theirs from
/// their own `.cargo/config.toml`).
pub const TARGET_TAG_ENV: &str = "DAGON_TARGET_TAG";

/// What a firmware build is given; a value left `None` is not passed down
/// at all (removed from the inherited environment).
#[derive(Clone, Copy, Default)]
pub(crate) struct FirmwareEnv<'a> {
    /// Checked by the caller; goes to the firmware as DAGON_PUBKEY.
    pub pubkey_hex: Option<&'a str>,
    /// DAGON_TARGET_TAG: the node's tag from the topology.
    pub target_tag: Option<&'a str>,
    /// DAGON_BOOT_CRATE: the node's boot crate folder, which a boot stage
    /// (stage0, boot core) is built for.
    pub boot_crate: Option<&'a Path>,
    /// DAGON_LINK_BAUD: the node's UART link speed ([`link_baud`]).
    pub link_baud: Option<u32>,
}

/// The UART link speed node `id`'s firmware is built with: the project
/// topology's ([`dagon_topology::Topology::uart_baud`]), the same for both
/// ends of the link; `None` without a topology or a UART link. A topology
/// with errors (a speed one end cannot make, V-26) stops the build.
pub(crate) fn link_baud(cfg: &Config, id: &str) -> Result<Option<u32>, String> {
    let Some(path) = &cfg.topology else {
        return Ok(None);
    };
    crate::config::read_topology(cfg, path)?.uart_baud(id)
}

/// Sets (or removes) the firmware build inputs of `fw` on `cmd`.
fn firmware_env(cmd: &mut Command, fw: FirmwareEnv<'_>) {
    for (name, value) in [(PUBKEY_ENV, fw.pubkey_hex), (TARGET_TAG_ENV, fw.target_tag)] {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
    match fw.boot_crate {
        Some(d) => cmd.env(dagon_topology::defs::boot_folder::BOOT_FOLDER_ENV, d),
        None => cmd.env_remove(dagon_topology::defs::boot_folder::BOOT_FOLDER_ENV),
    };
    let baud = dagon_topology::defs::codegen::LINK_BAUD_ENV;
    match fw.link_baud {
        Some(b) => cmd.env(baud, b.to_string()),
        None => cmd.env_remove(baud),
    };
}

/// Runs `cargo build --release --locked` in `dir`; returns the executable
/// of binary `bin`. A crate without Cargo.lock yet (just generated) is
/// resolved once without --locked, and keeps the lock file cargo writes.
/// A version raised in its Cargo.toml is brought into its own lock entry
/// first ([`crate::srcver::lock_own_version`]).
pub(crate) fn cargo_build(
    dir: &Path,
    bin: &str,
    features: &[String],
    target_dir: Option<&Path>,
    fw: FirmwareEnv<'_>,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let mut cmd = Command::new("cargo");
    scrub(&mut cmd);
    firmware_env(&mut cmd, fw);
    cmd.args([
        "build",
        "--release",
        "--message-format=json-render-diagnostics",
        "--bin",
        bin,
    ])
    .current_dir(dir)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if let Some(v) = crate::srcver::lock_own_version(dir)? {
        log(&format!("Cargo.lock: the crate's own entry is now {v}"));
    }
    if dir.join("Cargo.lock").is_file() {
        cmd.arg("--locked");
    } else {
        log("no Cargo.lock yet: resolving the dependencies once; keep the Cargo.lock it writes");
    }
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    if let Some(t) = target_dir {
        cmd.arg("--target-dir").arg(t);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run cargo in {}: {e}", dir.display()))?;
    let (tx, rx) = mpsc::channel();
    let out = child.stdout.take().expect("piped");
    let err = child.stderr.take().expect("piped");
    let t1 = tx.clone();
    std::thread::spawn(move || {
        BufReader::new(out)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| drop(t1.send(Line::Out(l))))
    });
    std::thread::spawn(move || {
        BufReader::new(err)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| drop(tx.send(Line::Err(l))))
    });
    let mut exe = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        match rx.recv_timeout(POLL) {
            Ok(Line::Err(l)) => log(&l),
            Ok(Line::Out(l)) => {
                if let Some(p) = artifact(&l, bin) {
                    exe = Some(p);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build failed in {}", dir.display()));
    }
    exe.ok_or_else(|| format!("cargo built no binary named '{bin}' in {}", dir.display()))
}

/// The executable of `bin` in one line of cargo's JSON messages.
fn artifact(line: &str, bin: &str) -> Option<PathBuf> {
    let v: Value = serde_json::from_str(line).ok()?;
    let ours = v["reason"] == "compiler-artifact" && v["target"]["name"] == bin;
    ours.then(|| v["executable"].as_str().map(PathBuf::from))
        .flatten()
}

/// `dagon build`
pub fn command(a: &BuildArgs) -> Result<(), String> {
    let cfg = Config::load(a.config.as_deref())?;
    if cfg.build.is_empty() {
        return Err("no [build.<id>] tables in dagon.toml".into());
    }
    let nodes = &a.nodes;
    let mut log = |l: &str| println!("{l}");
    let built = build_nodes(&cfg, nodes, &mut log, &AtomicBool::new(false))?;
    for b in &built {
        let files: Vec<String> = b.slots.iter().map(|s| s.1.display().to_string()).collect();
        println!("{}: {}", b.node, files.join(", "));
    }
    Ok(())
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {

    #[test]
    fn the_output_folder_is_never_cleared_through_a_junction() {
        if !cfg!(windows) {
            return; // the link cases themselves are in genfs
        }
        let base = std::env::temp_dir().join(format!("dagon-nb-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("nodes/seg/src")).unwrap();
        std::fs::write(
            base.join("nodes/seg/src/app.rs"),
            "// mine
",
        )
        .unwrap();
        let cfg = Config {
            base: crate::config::base_of(&base.join("dagon.toml")),
            ..Config::default()
        };
        let out = node_dir(&cfg, "seg");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let p = |x: &Path| x.components().collect::<PathBuf>();
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(p(&out))
            .arg(p(&cfg.base.join("nodes/seg")))
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J");
        let e = clear_output(&cfg, "seg").unwrap_err();
        assert!(
            e.contains("[build.seg] output") && e.contains("is a link"),
            "{e}"
        );
        assert!(base.join("nodes/seg/src/app.rs").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }
    use super::*;

    #[test]
    fn both_ends_of_a_uart_link_are_built_with_the_topologys_baud() {
        let base = std::env::temp_dir().join(format!("dagon-nb-baud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let yaml = crate::scaffold::SAMPLE.replace("baud: 115200", "baud: 230400");
        std::fs::write(base.join("topology.yaml"), yaml).unwrap();
        let mut cfg = Config {
            base: crate::config::base_of(&base.join("dagon.toml")),
            ..Config::default()
        };
        assert_eq!(link_baud(&cfg, "hub"), Ok(None), "no topology: none given");
        cfg.topology = Some(base.join("topology.yaml"));
        assert_eq!(link_baud(&cfg, "hub"), Ok(Some(230_400)));
        assert_eq!(link_baud(&cfg, "watch"), Ok(Some(230_400)));
        assert_eq!(link_baud(&cfg, "seg"), Ok(None));
        let mut cmd = Command::new("cargo");
        let fw = FirmwareEnv {
            link_baud: Some(230_400),
            ..FirmwareEnv::default()
        };
        firmware_env(&mut cmd, fw);
        let env = |k: &str| {
            cmd.get_envs()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(
            env(dagon_topology::defs::codegen::LINK_BAUD_ENV),
            Some(Some("230400".into()))
        );
        // Not given: removed, never inherited from the caller's shell.
        let mut cmd = Command::new("cargo");
        firmware_env(&mut cmd, FirmwareEnv::default());
        assert!(cmd
            .get_envs()
            .any(|(n, v)| n == dagon_topology::defs::codegen::LINK_BAUD_ENV && v.is_none()));
        // An out-of-range speed is refused before anything builds.
        std::fs::write(
            base.join("topology.yaml"),
            crate::scaffold::SAMPLE.replace("baud: 115200", "baud: 921600"),
        )
        .unwrap();
        let e = link_baud(&cfg, "watch").unwrap_err();
        assert!(e.contains("topology has errors"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cargo_does_not_inherit_the_cli_secrets() {
        let mut cmd = Command::new("cargo");
        scrub(&mut cmd);
        let removed: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.iter().any(|k| k == "DAGON_SIGNING_KEY"));
        assert!(removed.iter().any(|k| k == "ANTHROPIC_API_KEY"));
        assert!(
            removed.iter().any(|k| k == "RUSTUP_TOOLCHAIN"),
            "toolchain file wins"
        );
    }

    #[test]
    fn picks_the_executable_of_the_named_binary() {
        let line = r#"{"reason":"compiler-artifact","target":{"name":"f401-app"},"executable":"C:\\t\\f401-app"}"#;
        assert_eq!(
            artifact(line, "f401-app"),
            Some(PathBuf::from("C:\\t\\f401-app"))
        );
        assert_eq!(artifact(line, "other"), None);
        let lib =
            r#"{"reason":"compiler-artifact","target":{"name":"f401-app"},"executable":null}"#;
        assert_eq!(artifact(lib, "f401-app"), None);
        assert_eq!(artifact("not json", "f401-app"), None);
    }

    #[test]
    fn a_new_crate_builds_without_a_lock_file_with_the_projects_pubkey() {
        let base = std::env::temp_dir().join(format!("dagon-nolock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pkg = base.join("pkg");
        std::fs::create_dir_all(pkg.join("src")).unwrap();
        std::fs::write(
            pkg.join("Cargo.toml"),
            "[package]\nname = \"fresh\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n",
        )
        .unwrap();
        // Compiles only when the build sees DAGON_PUBKEY.
        std::fs::write(
            pkg.join("src/main.rs"),
            "const K: &str = env!(\"DAGON_PUBKEY\");\nfn main() { println!(\"{K}\"); }\n",
        )
        .unwrap();
        let pair = dagon_sign::generate_to_dir(&base.join("keys")).unwrap();
        let pubkey = pair.public_path.clone();
        let mut cfg = Config {
            base: base.clone(),
            pubkey: Some(pubkey),
            ..Config::default()
        };
        cfg.build.insert(
            "fresh".into(),
            BuildInputs {
                package: PathBuf::from("pkg"),
                bin: None,
                features: vec![],
                slot_b_feature: None,
                boot_features: vec![],
            },
        );
        let mut lines = Vec::new();
        let r = build_nodes(
            &cfg,
            &[],
            &mut |l| lines.push(l.to_string()),
            &AtomicBool::new(false),
        );
        assert!(r.is_ok(), "{lines:?}");
        // The firmware gets the checked key itself, not the file's path.
        let exe = &r.unwrap()[0].slots[0].1;
        let out = Command::new(exe).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            dagon_sign::to_hex(&pair.public_key)
        );
        assert!(lines.iter().any(|l| l.contains("no Cargo.lock yet")));
        assert!(pkg.join("Cargo.lock").is_file(), "the lock file is kept");
        // Next time it builds --locked, without the note.
        let mut again = Vec::new();
        build_nodes(
            &cfg,
            &[],
            &mut |l| again.push(l.to_string()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!again.iter().any(|l| l.contains("no Cargo.lock yet")));
        // Without the project's pubkey the build does not see one.
        if std::env::var_os(PUBKEY_ENV).is_none() {
            cfg.pubkey = None;
            std::fs::write(
                pkg.join("src/main.rs"),
                "const K: &str = env!(\"DAGON_PUBKEY\");\nfn main() { println!(\"{K} \"); }\n",
            )
            .unwrap();
            assert!(build_nodes(&cfg, &[], &mut |_| {}, &AtomicBool::new(false)).is_err());
        }
        // A secret key named as the pubkey never goes into firmware.
        let seed =
            std::fs::read_to_string(base.join("keys").join(dagon_sign::SECRET_FILE)).unwrap();
        let leak = base.join("keys/leak.pub");
        std::fs::write(&leak, seed).unwrap();
        cfg.pubkey = Some(leak);
        let e = build_nodes(&cfg, &[], &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("holds a secret key"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_raised_crate_version_builds_with_only_its_own_lock_entry_changed() {
        let base = std::env::temp_dir().join(format!("dagon-raised-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pkg = base.join("pkg");
        std::fs::create_dir_all(pkg.join("src")).unwrap();
        let manifest = |v: &str| {
            format!("[package]\nname = \"raised\"\nversion = \"{v}\"\nedition = \"2021\"\n[workspace]\n")
        };
        std::fs::write(pkg.join("Cargo.toml"), manifest("1.0.0")).unwrap();
        std::fs::write(pkg.join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut cfg = Config {
            base: base.clone(),
            ..Config::default()
        };
        cfg.build.insert(
            "raised".into(),
            BuildInputs {
                package: PathBuf::from("pkg"),
                bin: None,
                features: vec![],
                slot_b_feature: None,
                boot_features: vec![],
            },
        );
        let quiet = &AtomicBool::new(false);
        build_nodes(&cfg, &[], &mut |_| {}, quiet).unwrap();
        let before = std::fs::read_to_string(pkg.join("Cargo.lock")).unwrap();
        // The developer raises the version; the next build takes it.
        std::fs::write(pkg.join("Cargo.toml"), manifest("1.1.0")).unwrap();
        let mut lines = Vec::new();
        let r = build_nodes(&cfg, &[], &mut |l| lines.push(l.to_string()), quiet);
        assert!(r.is_ok(), "{lines:?}");
        assert_eq!(
            last_build(&cfg, "raised").unwrap().0.version.as_deref(),
            Some("1.1.0")
        );
        let after = std::fs::read_to_string(pkg.join("Cargo.lock")).unwrap();
        assert_eq!(
            after,
            before.replace("version = \"1.0.0\"", "version = \"1.1.0\""),
            "only the crate's own entry changes"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 中止 of 雛形を作る leaves tables for crates not made yet.
    #[test]
    fn a_build_names_every_crate_not_made_yet_before_building_any() {
        let base =
            std::env::temp_dir().join(format!("dagon-missing-crates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut cfg = Config {
            base: base.clone(),
            ..Config::default()
        };
        for id in ["seg", "watch"] {
            cfg.build.insert(
                id.into(),
                BuildInputs {
                    package: PathBuf::from(format!("nodes/{id}")),
                    bin: None,
                    features: vec![],
                    slot_b_feature: None,
                    boot_features: vec![],
                },
            );
        }
        let mut log = Vec::new();
        let e = build_nodes(
            &cfg,
            &[],
            &mut |l| log.push(l.to_string()),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            e.contains("seg") && e.contains("watch") && e.contains("雛形を作る"),
            "{e}"
        );
        assert!(log.is_empty(), "{log:?}");
    }

    #[test]
    fn build_inputs_stay_inside_the_work_area() {
        let ok = BuildInputs {
            package: PathBuf::from("targets/f401-app"),
            bin: None,
            features: vec!["conform".into()],
            slot_b_feature: Some("slot-b".into()),
            boot_features: vec![],
        };
        assert!(ok.validate().is_ok());
        let with = |f: &dyn Fn(&mut BuildInputs)| {
            let mut b = ok.clone();
            f(&mut b);
            b.validate()
        };
        assert!(with(&|b| b.package = PathBuf::from("../elsewhere")).is_err());
        assert!(with(&|b| b.package = PathBuf::from("C:/abs")).is_err());
        assert!(with(&|b| b.features = vec!["a,b".into()]).is_err());
        assert!(with(&|b| b.bin = Some("x y".into())).is_err());
        // A name cargo would read as an option.
        assert!(with(&|b| b.bin = Some("--offline".into())).is_err());
        assert!(with(&|b| b.features = vec!["-x".into()]).is_err());
    }

    #[test]
    fn a_package_inside_the_dagon_source_is_taken_only_when_the_source_is_given() {
        let here = std::fs::canonicalize(crate::dagonsrc::BUILT_FROM).unwrap();
        let base = std::env::temp_dir().join(format!("dagon-insrc-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg = Config {
            base: base.clone(),
            ..Config::default()
        };
        let sample = BuildInputs {
            package: here.join("samples/f401-app"),
            bin: None,
            features: vec![],
            slot_b_feature: None,
            boot_features: vec![],
        };
        let dir = package_dir(&cfg, &sample, Some(&here)).unwrap();
        assert!(dir.join("Cargo.toml").is_file(), "{}", dir.display());
        assert!(
            package_dir(&cfg, &sample, None).is_err(),
            "only flash passes the source"
        );
        // Names are still checked; a folder outside both is refused.
        let bad = BuildInputs {
            bin: Some("--offline".into()),
            ..sample.clone()
        };
        assert!(package_dir(&cfg, &bad, Some(&here)).is_err());
        let outside = BuildInputs {
            package: std::env::temp_dir(),
            ..sample
        };
        assert!(package_dir(&cfg, &outside, Some(&here)).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_package_folder_goes_to_cargo_without_the_verbatim_prefix() {
        let dir = std::env::temp_dir();
        let canon = std::fs::canonicalize(&dir).unwrap();
        let plain = crate::config::plain_path(canon.clone());
        let verbatim: String = [char::from(92), char::from(92), '?'].iter().collect();
        assert!(!plain.to_string_lossy().starts_with(&verbatim));
        assert_eq!(std::fs::canonicalize(&plain).unwrap(), canon);
    }

    #[test]
    fn builds_a_package_and_copies_its_elf() {
        // A tiny host binary package stands in for a firmware package.
        let base = std::env::temp_dir().join(format!("dagon-build-{}", std::process::id()));
        let pkg = base.join("pkg");
        std::fs::create_dir_all(pkg.join("src")).unwrap();
        std::fs::write(
            pkg.join("Cargo.toml"),
            "[package]\nname = \"tiny\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
             [features]\nslot-b = []\nbroken = []\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("src/main.rs"),
            "#[cfg(feature = \"broken\")]\ncompile_error!(\"broken\");\nfn main() {}\n",
        )
        .unwrap();
        // Firmware crates' dependencies have build scripts, linked for the
        // host: both slots (slot B in its own target folder) must link one.
        std::fs::write(pkg.join("build.rs"), "fn main() {}\n").unwrap();
        // --locked needs a lock file; an empty package resolves offline.
        std::fs::create_dir_all(pkg.join("src/bin")).unwrap();
        std::fs::write(pkg.join("src/bin/other.rs"), "fn main() {}\n").unwrap();
        let gen = Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(&pkg)
            .status()
            .unwrap();
        assert!(gen.success());
        let mut cfg = Config {
            base: base.clone(),
            ..Config::default()
        };
        cfg.build.insert(
            "tiny".into(),
            BuildInputs {
                package: PathBuf::from("pkg"),
                bin: None,
                features: vec![],
                slot_b_feature: Some("slot-b".into()),
                boot_features: vec![],
            },
        );
        let mut lines = Vec::new();
        let built = build_nodes(
            &cfg,
            &[],
            &mut |l| lines.push(l.to_string()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(built[0].slots.len(), 2);
        assert!(elf_path(&cfg, "tiny", 'a').exists() && elf_path(&cfg, "tiny", 'b').exists());
        assert!(lines.iter().any(|l| l.contains("slot b")));
        let other = pkg
            .join("target/release")
            .join(format!("other{}", std::env::consts::EXE_SUFFIX));
        assert!(!other.exists(), "only the named binary is built (--bin)");
        let (rec, elfs) = last_build(&cfg, "tiny").unwrap();
        assert_eq!(elfs.iter().map(|e| e.0).collect::<Vec<_>>(), ['a', 'b']);
        assert_eq!(rec.slots.len(), 2);

        // A tampered output is not handed to signing.
        std::fs::write(elf_path(&cfg, "tiny", 'a'), b"other").unwrap();
        assert!(last_build(&cfg, "tiny").is_err());

        // Without slot B configured, the old app-b.elf goes away.
        let quiet = &AtomicBool::new(false);
        cfg.build.get_mut("tiny").unwrap().slot_b_feature = None;
        build_nodes(&cfg, &[], &mut |_| {}, quiet).unwrap();
        assert!(!elf_path(&cfg, "tiny", 'b').exists());
        assert_eq!(last_build(&cfg, "tiny").unwrap().1.len(), 1);

        // A failed build leaves nothing signable.
        cfg.build.get_mut("tiny").unwrap().features = vec!["broken".into()];
        assert!(build_nodes(&cfg, &[], &mut |_| {}, quiet).is_err());
        assert!(last_build(&cfg, "tiny").is_err());
        assert!(!elf_path(&cfg, "tiny", 'a').exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
