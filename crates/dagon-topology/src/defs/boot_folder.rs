//! A node's generated boot crate folder (`boot/<id>/`) as the boot
//! stages' build scripts read it, and the source hash of a trusted
//! extension. `dagon gen` writes the folder: the boot crate's `Cargo.toml`
//! carries the node's identity in `[package.metadata.dagon.boot]`, so a
//! boot stage takes its board from the node it is built for, never from a
//! guess, and links only the extensions dagon.toml declares trusted.
//!
//! ```toml
//! [package.metadata.dagon.boot]
//! node = "hub"
//! mcu = "rp2040"
//! board = "waveshare-rp2040-touch-lcd-1.28"   # absent for a node without a board
//! project = "../../../dagon.toml"
//! [[package.metadata.dagon.boot.trusted]]
//! id = "boot-show"
//! path = "../../../extensions/boot-show"
//! sha256 = "<64 hex digits>"
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The environment variable a boot stage's build reads: the node's
/// generated folder. The boot crate `dagon gen` writes there sets it for
/// its own build (`.cargo/config.toml`); stage0 is built with it set.
pub const BOOT_FOLDER_ENV: &str = "DAGON_BOOT_CRATE";

/// One trusted extension the boot crate links.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedSource {
    pub id: String,
    /// Its package folder, relative to the boot folder.
    pub path: String,
    /// The source hash dagon.toml declares ([`source_sha256`]).
    pub sha256: String,
}

/// `[package.metadata.dagon.boot]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootMeta {
    pub node: String,
    pub mcu: String,
    #[serde(default)]
    pub board: Option<String>,
    /// The project's dagon.toml, relative to the boot folder (its inline
    /// boards); absent: the source tree's boards only.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub trusted: Vec<TrustedSource>,
}

/// The boot folder of one node.
#[derive(Debug, Clone)]
pub struct BootFolder {
    pub dir: PathBuf,
    pub meta: BootMeta,
}

fn err(at: &Path, msg: impl std::fmt::Display) -> String {
    format!("{}: {msg}", at.display())
}

impl BootFolder {
    /// Reads `dir/Cargo.toml`.
    pub fn read(dir: &Path) -> Result<BootFolder, String> {
        let path = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| err(&path, e))?;
        let t: toml::Table = text.parse().map_err(|e| err(&path, e))?;
        let meta = t
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("dagon"))
            .and_then(|d| d.get("boot"))
            .cloned()
            .ok_or_else(|| {
                err(
                    &path,
                    "no [package.metadata.dagon.boot]: not a boot folder dagon gen wrote",
                )
            })?;
        let meta: BootMeta = meta.try_into().map_err(|e| err(&path, e))?;
        Ok(BootFolder {
            dir: dir.to_path_buf(),
            meta,
        })
    }

    /// The project's dagon.toml.
    pub fn project(&self) -> Option<PathBuf> {
        self.meta.project.as_ref().map(|p| self.dir.join(p))
    }

    /// The trusted extensions' package folders.
    pub fn trusted_folders(&self) -> Vec<PathBuf> {
        self.trusted_dirs().into_iter().map(|(d, _)| d).collect()
    }

    fn trusted_dirs(&self) -> Vec<(PathBuf, &TrustedSource)> {
        self.meta
            .trusted
            .iter()
            .map(|t| (canon(&self.dir.join(&t.path)), t))
            .collect()
    }

    /// Checks that each trusted extension's source still hashes to what
    /// dagon.toml declares. Returns the files the hashes read.
    pub fn check_hashes(&self) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        for (dir, t) in self.trusted_dirs() {
            let (got, read) = source_files_sha256(&dir)?;
            if !got.eq_ignore_ascii_case(&t.sha256) {
                return Err(format!(
                    "trusted extension {} ({}): its source hashes to {got}; dagon.toml declares {}",
                    t.id,
                    dir.display(),
                    t.sha256
                ));
            }
            files.extend(read);
        }
        Ok(files)
    }

    /// Checks the boot crate's resolved dependency graph (`metadata`:
    /// `cargo metadata --filter-platform <target>` of the boot crate;
    /// `lock`: its Cargo.lock): every package built for it (normal and
    /// build dependencies, from the boot crate down; dev-dependencies are
    /// never built into it) is one of
    ///
    /// - DAGON's: a path package inside the DAGON source at `dagon_root`,
    ///   or a package from the same source as the boot core library
    ///   `own` (DAGON from git);
    /// - a declared trusted extension, or a path package inside one (its
    ///   files are in the source hash);
    /// - a registry crate whose Cargo.lock entry carries a checksum.
    ///
    /// Anything else (a path crate elsewhere, a git crate of another
    /// source, a registry crate without a checksum) is refused by name.
    pub fn check_graph(
        &self,
        metadata: &serde_json::Value,
        lock: &str,
        own: &str,
        dagon_root: &Path,
    ) -> Result<(), String> {
        let root = canon(dagon_root);
        let trusted: Vec<PathBuf> = self.trusted_dirs().into_iter().map(|(d, _)| d).collect();
        let lock: toml::Table = if lock.is_empty() {
            toml::Table::new()
        } else {
            lock.parse().map_err(|e| format!("Cargo.lock: {e}"))?
        };
        let pinned = |name: &str, version: &str, source: &str| {
            lock.get("package")
                .and_then(|p| p.as_array())
                .into_iter()
                .flatten()
                .any(|p| {
                    let s = |k: &str| p.get(k).and_then(|v| v.as_str());
                    s("name") == Some(name)
                        && s("version") == Some(version)
                        && s("source") == Some(source)
                        && s("checksum").is_some()
                })
        };
        let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
        let pkg = |id: &str| packages.iter().find(|p| p["id"] == id);
        let own_source = packages
            .iter()
            .find(|p| p["name"] == own)
            .map(|p| p["source"].clone())
            .ok_or_else(|| {
                format!(
                    "the boot crate of node {} does not link {own}",
                    self.meta.node
                )
            })?;
        let nodes = metadata["resolve"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let root_id = metadata["resolve"]["root"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut todo = vec![root_id.clone()];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(n) = nodes.iter().find(|n| n["id"] == id.as_str()) else {
                continue;
            };
            for d in n["deps"].as_array().into_iter().flatten() {
                let built = d["dep_kinds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|k| k["kind"].as_str() != Some("dev"));
                if built {
                    todo.push(d["pkg"].as_str().unwrap_or_default().to_string());
                }
            }
            if id == root_id {
                continue;
            }
            let p = pkg(&id).ok_or_else(|| format!("cargo metadata: no package {id}"))?;
            let name = p["name"].as_str().unwrap_or_default();
            let version = p["version"].as_str().unwrap_or_default();
            let refuse = |why: &str| {
                Err(format!(
                    "the boot crate of node {} links {name} {version}, {why}",
                    self.meta.node
                ))
            };
            match p["source"].as_str() {
                None => {
                    let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default())
                        .parent()
                        .map(canon)
                        .unwrap_or_default();
                    let ours = own_source.is_null() && dir.starts_with(&root);
                    if !ours && !trusted.iter().any(|t| dir.starts_with(t)) {
                        return refuse(&format!(
                            "a path crate ({}) in neither DAGON nor an extension dagon.toml declares trusted: not declared",
                            dir.display()
                        ));
                    }
                }
                Some(_) if p["source"] == own_source => {}
                Some(s) if s.starts_with("registry+") || s.starts_with("sparse+") => {
                    if !pinned(name, version, s) {
                        return refuse(
                            "a registry crate without a checksum in the boot crate's Cargo.lock",
                        );
                    }
                }
                Some(s) => return refuse(&format!("from {s}, which is not DAGON's source")),
            }
        }
        Ok(())
    }
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| err(dir, e))?;
    for e in entries {
        let e = e.map_err(|x| err(dir, x))?;
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().map_err(|x| err(&e.path(), x))?.is_dir();
        let r = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if is_dir {
            walk(&e.path(), &r, out)?;
        } else {
            out.push((r, e.path()));
        }
    }
    Ok(())
}

/// The source hash of an extension package: SHA-256 over every file of
/// the folder (sorted by path, `/`-separated, each as its path, a zero
/// byte, its length as u64 little-endian and its bytes with CRLF read as
/// LF, so a checkout's line endings do not change it). Lowercase hex.
///
/// Nothing in the folder is left out: `include!` and `#[path]` let the
/// compiler read any file in it, `target/` and hidden folders included,
/// so an exclusion would let unhashed code into the image. Running cargo
/// inside the extension's own folder (its `target/`, its `Cargo.lock`)
/// therefore changes the hash; a boot crate builds it elsewhere.
pub fn source_sha256(dir: &Path) -> Result<String, String> {
    source_files_sha256(dir).map(|(h, _)| h)
}

fn source_files_sha256(dir: &Path) -> Result<(String, Vec<PathBuf>), String> {
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort();
    let mut sha = dagon_format::Sha256::new();
    for (rel, path) in &files {
        let bytes = std::fs::read(path).map_err(|e| err(path, e))?;
        let mut text = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                text.push(b);
            }
        }
        sha.update(rel.as_bytes());
        sha.update(&[0]);
        sha.update(&(text.len() as u64).to_le_bytes());
        sha.update(&text);
    }
    let hex: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((hex, files.into_iter().map(|(_, p)| p).collect()))
}

/// The file in a boot folder that records a passed host check of the boot
/// crate's dependency graph (`dagon gen`).
pub const STAMP_FILE: &str = "graph.stamp";

/// What the stamp file holds: gen's mark and the stamp.
pub fn stamp_text(stamp: &str) -> String {
    format!(
        "# Generated by `dagon gen`: the boot crate's dependency graph was checked for these inputs. Do not edit.\n{stamp}\n"
    )
}

fn normalized(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
            out.push(b);
        }
    }
    out
}

impl BootFolder {
    /// The stamp of the boot crate's current inputs: SHA-256 over its
    /// `Cargo.toml`, its `Cargo.lock` (empty when there is none) and each
    /// trusted extension's id and source hash, which must equal what
    /// dagon.toml declares. The host check writes it after the graph
    /// passed ([`BootFolder::check_graph`]); the boot core's build
    /// recomputes it ([`BootFolder::verify_stamp`]).
    pub fn stamp(&self) -> Result<String, String> {
        self.check_hashes()?;
        let mut sha = dagon_format::Sha256::new();
        sha.update(b"dagon boot graph 1\0");
        for f in ["Cargo.toml", "Cargo.lock"] {
            let bytes = normalized(&std::fs::read(self.dir.join(f)).unwrap_or_default());
            sha.update(&(bytes.len() as u64).to_le_bytes());
            sha.update(&bytes);
        }
        for (dir, t) in self.trusted_dirs() {
            sha.update(t.id.as_bytes());
            sha.update(&[0]);
            sha.update(source_sha256(&dir)?.as_bytes());
        }
        Ok(sha.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Refuses a build whose boot folder has no stamp, or one of other
    /// inputs than the current ones.
    pub fn verify_stamp(&self) -> Result<(), String> {
        let path = self.dir.join(STAMP_FILE);
        let text = std::fs::read_to_string(&path).map_err(|_| {
            format!(
                "{}: no checked dependency graph for node {}: run `dagon gen`",
                path.display(),
                self.meta.node
            )
        })?;
        let want = self.stamp()?;
        let got = text.lines().last().unwrap_or_default().trim();
        if got != want {
            return Err(format!(
                "{}: the boot crate of node {} changed since its dependency graph was checked \
                 (its Cargo.toml or Cargo.lock, or a trusted extension's source): run `dagon gen`",
                path.display(),
                self.meta.node
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("dagon-bootfolder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ext(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"boot-show\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), body).unwrap();
    }

    #[test]
    fn the_source_hash_covers_every_file_the_compiler_could_read() {
        let d = scratch("hash");
        ext(
            &d,
            "pub fn a() {}
",
        );
        let h = source_sha256(&d).unwrap();
        assert_eq!(h.len(), 64);
        // Line endings do not change it.
        std::fs::write(
            d.join("src/lib.rs"),
            "pub fn a() {}
",
        )
        .unwrap();
        assert_eq!(source_sha256(&d).unwrap(), h);
        // Every other file does, wherever it is: `include!` and `#[path]`
        // can read a file in target/ or in a hidden folder as well.
        for f in [
            "target/debug/x.rs",
            ".hidden/x.rs",
            "Cargo.lock",
            "src/more.rs",
        ] {
            let path = d.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                "pub fn x() {}
",
            )
            .unwrap();
            assert_ne!(source_sha256(&d).unwrap(), h, "{f}");
            std::fs::remove_file(&path).unwrap();
            assert_eq!(source_sha256(&d).unwrap(), h, "{f}");
        }
        std::fs::write(
            d.join("src/lib.rs"),
            "pub fn b() {}
",
        )
        .unwrap();
        assert_ne!(source_sha256(&d).unwrap(), h);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The firmware lint builds each boot core library against its MCU
    /// package's `bootcore/lint` folder (a node without board or
    /// extensions), which carries its stamp like any boot folder.
    /// `DAGON_BLESS=1` rewrites them.
    #[test]
    fn the_lint_boot_folders_carry_current_stamps() {
        let mcu = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/mcu");
        let mut seen = 0;
        for e in std::fs::read_dir(&mcu).unwrap().flatten() {
            let dir = e.path().join("bootcore/lint");
            if !dir.is_dir() {
                continue;
            }
            seen += 1;
            let f = BootFolder::read(&dir).unwrap();
            assert!(
                f.meta.trusted.is_empty() && f.meta.board.is_none(),
                "{}",
                dir.display()
            );
            if std::env::var_os("DAGON_BLESS").is_some() {
                std::fs::write(dir.join(STAMP_FILE), stamp_text(&f.stamp().unwrap())).unwrap();
            }
            f.verify_stamp().unwrap();
        }
        assert!(seen > 0);
    }

    #[test]
    fn a_boot_build_needs_the_stamp_of_its_current_inputs() {
        let (d, f) = project("stamp", "", "");
        let e = f.verify_stamp().unwrap_err();
        assert!(e.contains("dagon gen"), "{e}");
        std::fs::write(f.dir.join(STAMP_FILE), stamp_text(&f.stamp().unwrap())).unwrap();
        f.verify_stamp().unwrap();
        // Stale after any input changes: the boot crate's manifest, its
        // lock file, or a trusted extension's source.
        for change in [
            (
                "nodes/n/dagon/Cargo.toml",
                "
# more
",
            ),
            (
                "nodes/n/dagon/Cargo.lock",
                "# lock
",
            ),
            (
                "extensions/show/src/lib.rs",
                "// more
",
            ),
        ] {
            let path = d.join(change.0);
            let before = std::fs::read(&path).unwrap_or_default();
            let mut after = before.clone();
            after.extend_from_slice(change.1.as_bytes());
            std::fs::write(&path, &after).unwrap();
            let f = BootFolder::read(&f.dir).unwrap();
            assert!(f.verify_stamp().is_err(), "{}", change.0);
            std::fs::write(&path, &before).unwrap();
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A project with the boot folder of node `n` and one extension.
    fn project(name: &str, sha: &str, extra_dep: &str) -> (PathBuf, BootFolder) {
        let d = scratch(name);
        ext(&d.join("extensions/show"), "pub fn a() {}\n");
        let boot = d.join("nodes/n/dagon");
        std::fs::create_dir_all(&boot).unwrap();
        let sha = if sha.is_empty() {
            source_sha256(&d.join("extensions/show")).unwrap()
        } else {
            sha.to_string()
        };
        std::fs::write(
            boot.join("Cargo.toml"),
            format!(
                "[package]\nname = \"n-boot\"\nversion = \"0.1.0\"\n\
                 [package.metadata.dagon.boot]\nnode = \"n\"\nmcu = \"rp2040\"\nproject = \"../../../dagon.toml\"\n\
                 [[package.metadata.dagon.boot.trusted]]\nid = \"show\"\npath = \"../../../extensions/show\"\nsha256 = \"{sha}\"\n\
                 [dependencies]\ndagon-events = {{ git = \"https://example.invalid/dagon\", tag = \"v1\" }}\n\
                 boot-show = {{ path = \"../../../extensions/show\" }}\n{extra_dep}"
            ),
        )
        .unwrap();
        let f = BootFolder::read(&boot).unwrap();
        (d, f)
    }

    #[test]
    fn a_boot_folder_links_only_what_dagon_toml_declares() {
        let dagon = scratch("dagon-root");
        let (d, f) = project("ok", "", "");
        assert_eq!(f.meta.node, "n");
        assert_eq!(f.meta.board, None);
        assert!(f.project().unwrap().ends_with("dagon.toml"));
        let read = f.check_hashes().unwrap();
        assert!(read.iter().any(|p| p.ends_with("lib.rs")));
        std::fs::remove_dir_all(&d).unwrap();

        // The source changed after dagon.toml declared its hash.
        let (d, f) = project("changed", &"0".repeat(64), "");
        let e = f.check_hashes().unwrap_err();
        assert!(
            e.contains("trusted extension show") && e.contains("hashes to"),
            "{e}"
        );
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&dagon).unwrap();
    }

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
    const DAGON_GIT: &str = "git+https://example.invalid/dagon?tag=v1#0123abcd";

    /// A resolved graph as `cargo metadata` gives it: the boot crate `n-boot`
    /// at the root, `edges` as (from, to, kind) by package name.
    fn graph(
        pkgs: &[(&str, Option<&str>, &Path)],
        edges: &[(&str, &str, &str)],
    ) -> serde_json::Value {
        let id = |n: &str| format!("{n} 0.1.0");
        let packages: Vec<_> = pkgs
            .iter()
            .map(|(name, source, dir)| {
                serde_json::json!({
                    "name": name, "version": "0.1.0", "id": id(name), "source": source,
                    "manifest_path": dir.join("Cargo.toml"),
                })
            })
            .collect();
        let nodes: Vec<_> = pkgs
            .iter()
            .map(|(name, _, _)| {
                let deps: Vec<_> = edges
                    .iter()
                    .filter(|(from, _, _)| from == name)
                    .map(|(_, to, kind)| {
                        let kind = if *kind == "normal" { serde_json::Value::Null } else { (*kind).into() };
                        serde_json::json!({ "pkg": id(to), "dep_kinds": [{ "kind": kind, "target": null }] })
                    })
                    .collect();
                serde_json::json!({ "id": id(name), "deps": deps })
            })
            .collect();
        serde_json::json!({ "packages": packages, "resolve": { "root": id("n-boot"), "nodes": nodes } })
    }

    fn lock(entries: &[(&str, &str, bool)]) -> String {
        entries
            .iter()
            .map(|(name, source, sum)| {
                format!(
                    "[[package]]\nname = {name:?}\nversion = \"0.1.0\"\nsource = {source:?}\n{}\n",
                    if *sum { "checksum = \"00\"" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn the_whole_resolved_graph_is_dagons_declared_or_pinned() {
        let dagon = scratch("graph-dagon");
        let (d, f) = project("graph", "", "");
        let boot = f.dir.clone();
        let ext = d.join("extensions/show");
        let inner = ext.join("vendor/helper");
        let outside = d.join("elsewhere/helper");
        let core = dagon.join("targets/mcu/x/bootcore");
        for p in [&inner, &outside, &core] {
            std::fs::create_dir_all(p).unwrap();
        }
        let own = "dagon-x-bootcore";
        let base = [
            ("n-boot", None, boot.as_path()),
            (own, None, core.as_path()),
            ("boot-show", None, ext.as_path()),
            ("heapless", Some(CRATES_IO), dagon.as_path()),
            ("inner", None, inner.as_path()),
            ("outside", None, outside.as_path()),
            (
                "gitdep",
                Some("git+https://example.invalid/other#ff"),
                dagon.as_path(),
            ),
            ("dagon-fake", Some(CRATES_IO), dagon.as_path()),
        ];
        let pinned = lock(&[
            ("heapless", CRATES_IO, true),
            ("dagon-fake", CRATES_IO, false),
        ]);
        let ok_edges = [
            ("n-boot", own, "normal"),
            ("n-boot", "boot-show", "normal"),
            (own, "heapless", "normal"),
            ("boot-show", "inner", "normal"),
            // Dev-dependencies are never built into the image.
            ("boot-show", "outside", "dev"),
        ];
        f.check_graph(&graph(&base, &ok_edges), &pinned, own, &dagon)
            .unwrap();
        // A path crate outside DAGON and outside the declared extension,
        // even two steps down.
        let mut e = ok_edges.to_vec();
        e.push(("inner", "outside", "normal"));
        let err = f
            .check_graph(&graph(&base, &e), &pinned, own, &dagon)
            .unwrap_err();
        assert!(
            err.contains("outside") && err.contains("not declared"),
            "{err}"
        );
        // A build-dependency runs code in the build: checked too.
        let mut e = ok_edges.to_vec();
        e.push(("boot-show", "outside", "build"));
        assert!(f
            .check_graph(&graph(&base, &e), &pinned, own, &dagon)
            .is_err());
        // A registry crate without a checksum in Cargo.lock, whatever its
        // name says.
        let mut e = ok_edges.to_vec();
        e.push(("boot-show", "dagon-fake", "normal"));
        let err = f
            .check_graph(&graph(&base, &e), &pinned, own, &dagon)
            .unwrap_err();
        assert!(
            err.contains("dagon-fake") && err.contains("checksum"),
            "{err}"
        );
        // A git crate from another source than DAGON's.
        let mut e = ok_edges.to_vec();
        e.push(("boot-show", "gitdep", "normal"));
        let err = f
            .check_graph(&graph(&base, &e), &pinned, own, &dagon)
            .unwrap_err();
        assert!(err.contains("gitdep"), "{err}");
        // DAGON from git: its packages share the boot core library's source.
        let git = [
            ("n-boot", None, boot.as_path()),
            (own, Some(DAGON_GIT), dagon.as_path()),
            ("dagon-events", Some(DAGON_GIT), dagon.as_path()),
        ];
        let e = [("n-boot", own, "normal"), (own, "dagon-events", "normal")];
        f.check_graph(&graph(&git, &e), "", own, &dagon).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&dagon).unwrap();
    }

    #[test]
    fn a_folder_gen_did_not_write_is_refused() {
        let d = scratch("not-gen");
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let e = BootFolder::read(&d).unwrap_err();
        assert!(e.contains("[package.metadata.dagon.boot]"), "{e}");
        assert!(BootFolder::read(&d.join("missing")).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
