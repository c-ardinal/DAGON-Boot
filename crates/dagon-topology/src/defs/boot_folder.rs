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
    /// The boot crate's `[dependencies]`.
    deps: toml::Table,
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
        let deps = t
            .get("dependencies")
            .and_then(|d| d.as_table())
            .cloned()
            .unwrap_or_default();
        Ok(BootFolder {
            dir: dir.to_path_buf(),
            meta,
            deps,
        })
    }

    /// The project's dagon.toml.
    pub fn project(&self) -> Option<PathBuf> {
        self.meta.project.as_ref().map(|p| self.dir.join(p))
    }

    /// Checks that the boot crate links only DAGON's packages (from the
    /// DAGON source at `dagon_root`, or from a git source) and the trusted
    /// extensions, and that each trusted extension's source still hashes
    /// to what dagon.toml declares. Returns the files the hashes read.
    pub fn check_linked(&self, dagon_root: &Path) -> Result<Vec<PathBuf>, String> {
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let root = canon(dagon_root);
        let trusted: Vec<(PathBuf, &TrustedSource)> = self
            .meta
            .trusted
            .iter()
            .map(|t| (canon(&self.dir.join(&t.path)), t))
            .collect();
        for (name, dep) in &self.deps {
            match dep.get("path").and_then(|p| p.as_str()) {
                Some(p) => {
                    let p = canon(&self.dir.join(p));
                    if !p.starts_with(&root) && !trusted.iter().any(|(t, _)| *t == p) {
                        return Err(format!(
                            "the boot crate of node {} links {name} ({}), which dagon.toml does not declare trusted for it",
                            self.meta.node,
                            p.display()
                        ));
                    }
                }
                None if name.starts_with("dagon-") => {}
                None => {
                    return Err(format!(
                        "the boot crate of node {} links {name}, which is no DAGON package and no declared trusted extension",
                        self.meta.node
                    ))
                }
            }
        }
        let mut files = Vec::new();
        for (dir, t) in &trusted {
            let (got, read) = source_files_sha256(dir)?;
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
}

/// Whether a folder entry is left out of the source hash: what cargo
/// writes (`target/`, a dependency's unused `Cargo.lock`) and hidden
/// entries (`.git`, editor state).
fn skipped(name: &str, is_dir: bool) -> bool {
    name.starts_with('.') || (is_dir && name == "target") || (!is_dir && name == "Cargo.lock")
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| err(dir, e))?;
    for e in entries {
        let e = e.map_err(|x| err(dir, x))?;
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().map_err(|x| err(&e.path(), x))?.is_dir();
        if skipped(&name, is_dir) {
            continue;
        }
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
/// LF, so a checkout's line endings do not change it), leaving out
/// `target/`, `Cargo.lock` and hidden entries. Lowercase hex.
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
    fn the_source_hash_follows_the_source_only() {
        let d = scratch("hash");
        ext(&d, "pub fn a() {}\n");
        let h = source_sha256(&d).unwrap();
        assert_eq!(h.len(), 64);
        // What cargo writes and line endings do not change it.
        std::fs::create_dir_all(d.join("target/debug")).unwrap();
        std::fs::write(d.join("target/debug/x"), "x").unwrap();
        std::fs::write(d.join("Cargo.lock"), "x").unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".git/HEAD"), "x").unwrap();
        std::fs::write(d.join("src/lib.rs"), "pub fn a() {}\r\n").unwrap();
        assert_eq!(source_sha256(&d).unwrap(), h);
        // A changed byte, an added file or a renamed one does.
        std::fs::write(d.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
        let changed = source_sha256(&d).unwrap();
        assert_ne!(changed, h);
        std::fs::write(d.join("src/more.rs"), "").unwrap();
        assert_ne!(source_sha256(&d).unwrap(), changed);
        std::fs::rename(d.join("src/more.rs"), d.join("src/other.rs")).unwrap();
        assert_ne!(source_sha256(&d).unwrap(), changed);
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
        let read = f.check_linked(&dagon).unwrap();
        assert!(read.iter().any(|p| p.ends_with("lib.rs")));
        std::fs::remove_dir_all(&d).unwrap();

        // The source changed after dagon.toml declared its hash.
        let (d, f) = project("changed", &"0".repeat(64), "");
        let e = f.check_linked(&dagon).unwrap_err();
        assert!(
            e.contains("trusted extension show") && e.contains("hashes to"),
            "{e}"
        );
        std::fs::remove_dir_all(&d).unwrap();

        // A crate dagon.toml does not declare trusted for the node.
        let (d, f) = project(
            "undeclared",
            "",
            "other = { path = \"../../../extensions/other\" }\n",
        );
        let e = f.check_linked(&dagon).unwrap_err();
        assert!(
            e.contains("links other") && e.contains("does not declare"),
            "{e}"
        );
        std::fs::remove_dir_all(&d).unwrap();
        let (d, f) = project("registry", "", "heapless = \"0.9\"\n");
        let e = f.check_linked(&dagon).unwrap_err();
        assert!(e.contains("links heapless"), "{e}");
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
