//! Project files from templates: the packages of a DAGON source and the
//! `template/` each ships, the dependency lines its placeholders name, and
//! the writing of a generated file under a project (kept, created,
//! unchanged or rewritten).

use std::path::{Path, PathBuf};

use crate::config::DagonSource;
use crate::genfs;

/// A file written (or kept) by gen, relative to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: String,
    pub action: &'static str,
}

pub(crate) fn write(
    base: &Path,
    rel: &str,
    text: &str,
    overwrite: bool,
    out: &mut Vec<Written>,
) -> Result<(), String> {
    // A link on the way is refused, never followed or replaced (genfs).
    let rel_path = genfs::Rel::new(rel)?;
    let path = genfs::checked(base, &rel_path)?;
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
        genfs::write(base, &rel_path, text.as_bytes())?;
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

/// Package manifests under `root` (target/, .git, node_modules and out/
/// skipped), as (directory, parsed Cargo.toml).
pub(crate) fn manifests(root: &Path) -> Vec<(PathBuf, toml::Value)> {
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

pub(crate) fn package_name(v: &toml::Value) -> Option<&str> {
    v.get("package")?.get("name")?.as_str()
}

/// The package directory named `name` in the clone at `root`.
pub fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    manifests(root)
        .into_iter()
        .find(|(_, v)| package_name(v) == Some(name))
        .map(|(d, _)| std::fs::canonicalize(&d).unwrap_or(d))
}

/// The dependency line for package `name`.
fn dependency(name: &str, src: &DagonSource, base: &Path) -> Result<String, String> {
    match crate::dagonsrc::clone_root(src, base) {
        None => Ok(format!(
            "{name} = {{ git = {:?}, tag = {:?} }}",
            src.git.as_deref().unwrap_or_default(),
            src.tag.as_deref().unwrap_or_default()
        )),
        Some(root) => {
            let dir = find_named(&root, name)
                .ok_or_else(|| format!("no package '{name}' in {}", root.display()))?;
            let dir = crate::paths::forward_path(&dir);
            Ok(format!("{name} = {{ path = {dir:?} }}"))
        }
    }
}

/// Project template files of a package: the `template/` it ships.
pub(crate) fn template_for(dir: &Path) -> Option<Vec<(String, String)>> {
    let mut files = Vec::new();
    collect(&dir.join("template"), "", &mut files);
    (!files.is_empty()).then(|| real_names(files))
}

/// Suffix of template files kept under another name: a template manifest
/// is `Cargo.toml.tpl`, so cargo (scanning a checkout of the DAGON
/// repository, or packaging a crate) never reads its placeholders as a
/// package.
const TEMPLATE_SUFFIX: &str = ".tpl";

/// The project file names of template files (`Cargo.toml.tpl` becomes
/// `Cargo.toml`).
fn real_names(files: Vec<(String, String)>) -> Vec<(String, String)> {
    files
        .into_iter()
        .map(|(rel, text)| match rel.strip_suffix(TEMPLATE_SUFFIX) {
            Some(real) => (real.to_string(), text),
            None => (rel, text),
        })
        .collect()
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

/// Fills every `{{dep:<package>}}` of a template file with the dependency
/// line for that DAGON package (from the project's `[dagon]` source).
pub(crate) fn render_deps(text: &str, src: &DagonSource, base: &Path) -> Result<String, String> {
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
    Ok(out)
}

/// The project template of the boot core of `mcu`: the `template/` of the
/// package declaring `[package.metadata.dagon] bootcore = "<mcu>"` in the
/// DAGON source at `root`, as (file, text) with `.tpl` names restored.
pub(crate) fn boot_template(root: &Path, mcu: &str) -> Option<Vec<(String, String)>> {
    let (dir, _) = manifests(root).into_iter().find(|(_, v)| {
        v.get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("dagon"))
            .and_then(|d| d.get("bootcore"))
            .and_then(|x| x.as_str())
            == Some(mcu)
    })?;
    template_for(&dir)
}
