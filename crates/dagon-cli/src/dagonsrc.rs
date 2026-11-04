//! Where a project's DAGON source is: the release it is pinned to (`[dagon]
//! git` + `tag`, fetched by cargo into its git checkouts) or a local clone
//! (`path`, `DAGON_HOME`, else the clone this CLI was built from). gen reads
//! the board packages' templates from it and flash builds the boot images
//! from it, so both follow the same version as the generated crates.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::scaffold::DagonSource;

/// Repository released projects take the DAGON crates from; a build can
/// point it elsewhere with DAGON_REPO_URL.
pub const REPO_URL: &str = match option_env!("DAGON_REPO_URL") {
    Some(u) => u,
    None => "https://github.com/c-ardinal/DAGON-Boot.git",
};
/// The DAGON clone this CLI was built from.
pub const BUILT_FROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// Overrides the DAGON clone used as source (DAGON development).
pub const HOME_ENV: &str = "DAGON_HOME";
/// A small DAGON package every release has: fetching it checks the whole
/// repository out.
const PROBE_DEP: &str = "dagon-format";
/// Where the probe crate lives in a project.
const PROBE_DIR: &str = "out/dagon-source";

/// The release tag of this CLI's own version (`cli-v<version>`,
/// docs/versioning.md).
pub fn release_tag() -> String {
    format!("cli-v{}", env!("CARGO_PKG_VERSION"))
}

/// The `[dagon]` source a new project is written with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewSource {
    /// A release: the repository and its tag.
    Release { git: String, tag: String },
    /// A local clone (DAGON development).
    Clone(PathBuf),
}

impl NewSource {
    /// `dagon new`'s choice: `--dagon-path` (or DAGON_HOME) keeps a local
    /// clone; otherwise the release of this CLI's version, from
    /// `--dagon-git` / `--dagon-tag` when given.
    pub fn from_options(
        path: Option<&str>,
        git: Option<&str>,
        tag: Option<&str>,
    ) -> Result<NewSource, String> {
        if path.is_some() && (git.is_some() || tag.is_some()) {
            return Err("--dagon-path takes no --dagon-git or --dagon-tag".into());
        }
        if let Some(p) = path {
            let abs = std::fs::canonicalize(p)
                .map_err(|_| format!("--dagon-path {p}: no such folder"))?;
            return Ok(NewSource::Clone(crate::config::plain_path(abs)));
        }
        if git.is_none() && tag.is_none() {
            return Ok(Self::default_for_new());
        }
        Ok(NewSource::Release {
            git: git.unwrap_or(REPO_URL).to_string(),
            tag: tag.map_or_else(release_tag, String::from),
        })
    }

    /// DAGON_HOME set: that clone; else this CLI's own release.
    pub fn default_for_new() -> NewSource {
        match std::env::var_os(HOME_ENV) {
            Some(h) => NewSource::Clone(PathBuf::from(h)),
            None => NewSource::Release {
                git: REPO_URL.to_string(),
                tag: release_tag(),
            },
        }
    }

    /// The body of the `[dagon]` table.
    pub fn toml(&self) -> String {
        match self {
            NewSource::Release { git, tag } => format!(
                "git = {git:?}\ntag = {tag:?}\n# path = \"<local DAGON clone>\"   # DAGON development\n"
            ),
            NewSource::Clone(p) => format!(
                "# git = \"<DAGON repository URL>\"\n# tag = \"<release tag>\"\npath = {:?}\n",
                crate::config::forward_path(p)
            ),
        }
    }
}

/// The local clone a project in `base` uses, or `None` in release mode.
pub fn clone_root(src: &DagonSource, base: &Path) -> Option<PathBuf> {
    if src.git.is_some() && src.tag.is_some() {
        return None;
    }
    Some(match (&src.path, std::env::var_os(HOME_ENV)) {
        (Some(p), _) => base.join(p),
        (None, Some(h)) => PathBuf::from(h),
        (None, None) => PathBuf::from(BUILT_FROM),
    })
}

/// The root of the DAGON source a project in `base` uses: its clone, or
/// cargo's checkout of `[dagon] git` at `tag` (fetched when missing).
pub fn root(src: &DagonSource, base: &Path) -> Result<PathBuf, String> {
    match (clone_root(src, base), &src.git, &src.tag) {
        (Some(r), _, _) => Ok(r),
        (None, Some(git), Some(tag)) => checkout(git, tag, base),
        _ => Err("[dagon] in dagon.toml needs git and tag, or path".into()),
    }
}

/// Cargo's git checkout of `git` at `tag`: a probe crate under `base`
/// depends on one DAGON package from there, and `cargo metadata` names
/// where cargo put it.
fn checkout(git: &str, tag: &str, base: &Path) -> Result<PathBuf, String> {
    let dir = base.join(PROBE_DIR);
    let manifest = format!(
        "# Written by dagon: finds the DAGON source of the project's release.\n\
         [package]\nname = \"dagon-source-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
         publish = false\n\n[workspace]\n\n[lib]\npath = \"lib.rs\"\n\n\
         [dependencies]\n{PROBE_DEP} = {{ git = {git:?}, tag = {tag:?} }}\n"
    );
    crate::genfs::write_out(base, &dir.join("Cargo.toml"), manifest.as_bytes())?;
    crate::genfs::write_out(base, &dir.join("lib.rs"), b"")?;
    let mut cmd = Command::new("cargo");
    for k in crate::nodebuild::SECRET_ENV {
        cmd.env_remove(k);
    }
    let o = cmd
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !o.status.success() {
        let why = String::from_utf8_lossy(&o.stderr);
        let last: Vec<&str> = why.lines().filter(|l| !l.trim().is_empty()).collect();
        return Err(format!(
            "cannot get the DAGON source {tag} from {git} (offline, or no such tag?): {}",
            last[last.len().saturating_sub(3)..].join(" / ")
        ));
    }
    let v: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let at = v["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["name"] == PROBE_DEP)
        .and_then(|p| p["manifest_path"].as_str())
        .map(PathBuf::from)
        .ok_or_else(|| format!("{git} at {tag} has no {PROBE_DEP} package"))?;
    // The checkout's top: the folder holding its .git.
    at.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("cannot find the checkout around {}", at.display()))
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
pub(crate) mod tests {
    use super::*;

    /// A git repository at `dir` holding `files` (path, content), committed
    /// and tagged `tag`: a stand-in for the DAGON repository, offline.
    pub(crate) fn repo(dir: &Path, tag: &str, files: &[(&str, &str)]) -> String {
        let _ = std::fs::remove_dir_all(dir);
        for (p, text) in files {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        commit_and_tag(dir, tag)
    }

    /// Commits everything in `dir` as a new repository tagged `tag`; its
    /// file URL.
    pub(crate) fn commit_and_tag(dir: &Path, tag: &str) -> String {
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "release"]);
        git(&["tag", tag]);
        let abs = std::fs::canonicalize(dir).unwrap();
        let abs = crate::config::forward_path(&abs);
        format!("file:///{}", abs.trim_start_matches('/'))
    }

    pub(crate) fn package(name: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
             [workspace]\n[lib]\npath = \"lib.rs\"\n"
        )
    }

    #[test]
    fn release_mode_finds_the_source_in_cargos_checkout_of_the_tag() {
        let d = std::env::temp_dir().join(format!("dagon-src-{}", std::process::id()));
        let url = repo(
            &d.join("repo"),
            "v9.9.9",
            &[
                ("crates/dagon-format/Cargo.toml", &package("dagon-format")),
                ("crates/dagon-format/lib.rs", ""),
            ],
        );
        let base = d.join("proj");
        std::fs::create_dir_all(&base).unwrap();
        let src = DagonSource {
            git: Some(url.clone()),
            tag: Some("v9.9.9".into()),
            path: None,
        };
        let r = root(&src, &base).unwrap();
        assert!(
            r.join("crates/dagon-format/Cargo.toml").is_file(),
            "{}",
            r.display()
        );
        assert!(
            !r.starts_with(&d),
            "cargo's own checkout, not the repository"
        );

        let missing = DagonSource {
            tag: Some("v0.0.404".into()),
            ..src
        };
        let e = root(&missing, &base).unwrap_err();
        assert!(e.contains("cannot get the DAGON source v0.0.404"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// stderr of `cargo metadata` on the probe crate a resolve left in `base`.
    fn probe_stderr(base: &Path) -> String {
        let o = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(base.join(PROBE_DIR).join("Cargo.toml"))
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8_lossy(&o.stderr).into_owned()
    }

    #[test]
    fn template_manifests_do_not_trouble_cargo_in_a_release_checkout() {
        let d = std::env::temp_dir().join(format!("dagon-src-tpl-{}", std::process::id()));
        // A board template as the packages ship it: placeholders in the
        // manifest, which is not TOML until gen renders it.
        let tpl = "[package]\nname = \"{{node}}\"\n\n[dependencies]\n{{dep:dagon-mcu-rp2040}}\n";
        let format = package("dagon-format");
        for (name, manifest, noisy) in [
            ("tpl", "targets/boards/x/template/Cargo.toml.tpl", false),
            ("raw", "targets/boards/x/template/Cargo.toml", true),
        ] {
            let files = [
                ("crates/dagon-format/Cargo.toml", format.as_str()),
                ("crates/dagon-format/lib.rs", ""),
                (manifest, tpl),
            ];
            let url = repo(&d.join(name), "v1.0.0", &files);
            let base = d.join(format!("proj-{name}"));
            std::fs::create_dir_all(&base).unwrap();
            let src = DagonSource {
                git: Some(url),
                tag: Some("v1.0.0".into()),
                path: None,
            };
            root(&src, &base).unwrap();
            let err = probe_stderr(&base);
            assert_eq!(
                err.contains("invalid key-value pair"),
                noisy,
                "{manifest}: {err}"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn this_repository_ships_no_template_manifest_cargo_would_read() {
        let here = Path::new(BUILT_FROM);
        let mut stack = vec![here.join("targets")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = e.path();
                let name = e.file_name();
                if p.is_dir() && name != "target" {
                    stack.push(p);
                } else if name == "Cargo.toml" {
                    let in_template = p
                        .components()
                        .any(|c| matches!(c.as_os_str().to_str(), Some("template" | "templates")));
                    assert!(!in_template, "{} must be Cargo.toml.tpl", p.display());
                }
            }
        }
    }

    #[test]
    fn path_mode_is_the_clone() {
        let src = DagonSource {
            git: None,
            tag: None,
            path: Some(PathBuf::from("../dagon")),
        };
        let base = Path::new("C:/p");
        assert_eq!(root(&src, base).unwrap(), base.join("../dagon"));
    }
}
