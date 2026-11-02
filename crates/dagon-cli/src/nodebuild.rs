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

use crate::config::{BuildInputs, Config, Opts};

const OUT_DIR: &str = "out/build";
const RECORD: &str = "build.json";
/// Secrets the CLI holds that a build (build scripts, proc macros) must not
/// see: the signing key path and the Claude API key.
/// The public key file the firmware build scripts embed.
pub const PUBKEY_ENV: &str = "DAGON_PUBKEY";

pub const SECRET_ENV: [&str; 2] = [crate::config::KEY_ENV, crate::ai::KEY_ENV];

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
    let ids: Vec<String> = if nodes.is_empty() {
        cfg.build.keys().cloned().collect()
    } else {
        nodes.to_vec()
    };
    if let Some(p) = &cfg.pubkey {
        crate::config::checked_pubkey(cfg, p).map_err(|e| format!("pubkey in dagon.toml: {e}"))?;
    }
    let mut out = Vec::new();
    for id in ids {
        let inputs = cfg
            .build
            .get(&id)
            .ok_or_else(|| format!("no [build.{id}] in dagon.toml"))?;
        out.push(build_node(cfg, &id, inputs, log, cancel)?);
    }
    Ok(out)
}

fn build_node(
    cfg: &Config,
    id: &str,
    b: &BuildInputs,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Built, String> {
    b.validate().map_err(|e| format!("[build.{id}]: {e}"))?;
    // Resolved through symlinks and junctions; must stay in the work area.
    let dir = crate::paths::contained(&cfg.base, &b.package)
        .map_err(|e| format!("[build.{id}] package: {e}"))?;
    let bin = match &b.bin {
        Some(n) => n.clone(),
        None => package_name(&dir)?,
    };
    let mut slots = vec![('a', b.features.clone(), None)];
    if let Some(f) = &b.slot_b_feature {
        let feats = b.features.iter().cloned().chain([f.clone()]).collect();
        slots.push(('b', feats, Some(dir.join("target").join("slot-b"))));
    }
    let out_dir = crate::paths::contained(&cfg.base, &node_dir(cfg, id))
        .map_err(|e| format!("[build.{id}] output: {e}"))?;
    match std::fs::remove_dir_all(&out_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot clear {}: {e}", out_dir.display())),
    }
    let mut built = Vec::new();
    let mut record = BuildRecord {
        version: b.version.clone(),
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
            cfg.pubkey.as_deref(),
            log,
            cancel,
        )?;
        let dest = elf_path(cfg, id, slot);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(&exe, &dest)
            .map_err(|e| format!("cannot copy {} to {}: {e}", exe.display(), dest.display()))?;
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
    crate::config::write_file(&out_dir.join(RECORD), text.as_bytes())?;
    Ok(Built {
        node: id.into(),
        slots: built,
    })
}

fn package_name(dir: &Path) -> Result<String, String> {
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
    for k in SECRET_ENV.iter().chain(&INHERITED_BUILD_ENV) {
        cmd.env_remove(k);
    }
}

enum Line {
    Out(String),
    Err(String),
}

/// Runs `cargo build --release --locked` in `dir`; returns the executable
/// of binary `bin`. A crate without Cargo.lock yet (just generated) is
/// resolved once without --locked, and keeps the lock file cargo writes.
/// `pubkey` (the project's) goes to the firmware as DAGON_PUBKEY, over the
/// environment's.
fn cargo_build(
    dir: &Path,
    bin: &str,
    features: &[String],
    target_dir: Option<&Path>,
    pubkey: Option<&Path>,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let mut cmd = Command::new("cargo");
    scrub(&mut cmd);
    if let Some(p) = pubkey {
        cmd.env(PUBKEY_ENV, p);
    }
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

/// `dagon build [<id>...] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let o = Opts::parse(args, &["config"])?;
    let cfg = Config::load(o.get("config"))?;
    if cfg.build.is_empty() {
        return Err("no [build.<id>] tables in dagon.toml".into());
    }
    let nodes: Vec<String> = o.pos.iter().map(|s| s.to_string()).collect();
    let mut log = |l: &str| println!("{l}");
    let built = build_nodes(&cfg, &nodes, &mut log, &AtomicBool::new(false))?;
    for b in &built {
        let files: Vec<String> = b.slots.iter().map(|s| s.1.display().to_string()).collect();
        println!("{}: {}", b.node, files.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let pubkey = dagon_sign::generate_to_dir(&base.join("keys"))
            .unwrap()
            .public_path;
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
                version: None,
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
        assert!(e.contains("secret"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn build_inputs_stay_inside_the_work_area() {
        let ok = BuildInputs {
            package: PathBuf::from("targets/f401-app"),
            bin: None,
            features: vec!["conform".into()],
            slot_b_feature: Some("slot-b".into()),
            version: Some("1.1.0".into()),
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
        assert!(with(&|b| b.version = Some("one".into())).is_err());
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
                version: None,
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
