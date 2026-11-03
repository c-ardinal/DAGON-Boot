//! The projects serve opened lately, kept in ~/.dagon/recent.json: newest
//! first, at most [`MAX_RECENT`], only folders under home that still hold a
//! dagon.toml (outside the key folders).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::CONFIG_FILE;

/// Projects the list keeps.
pub const MAX_RECENT: usize = 10;
const FILE: &str = "recent.json";
/// A recent.json larger than this is ignored (it holds at most 10 paths).
const MAX_FILE: usize = 256 * 1024;

fn file(home: &Path) -> PathBuf {
    home.join(".dagon").join(FILE)
}

/// `dir` when it may be listed: under home, outside the key folders, with a
/// dagon.toml; resolved.
fn listable(home: &Path, dir: &Path) -> Option<PathBuf> {
    let d = std::fs::canonicalize(dir).ok()?;
    let keys = super::project::key_dirs(home);
    (super::project::in_dir(&d, home)
        && !keys.iter().any(|k| super::project::in_dir(&d, k))
        && d.join(CONFIG_FILE).is_file())
    .then_some(d)
}

/// The listed folders, newest first (unusable entries left out).
pub fn read(home: &Path) -> Vec<PathBuf> {
    let text = crate::paths::read_capped(&file(home), MAX_FILE)
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    let list: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in list {
        if let Some(d) = listable(home, Path::new(&p)) {
            if !out.contains(&d) {
                out.push(d);
            }
        }
        if out.len() == MAX_RECENT {
            break;
        }
    }
    out
}

/// Puts `dir` first. A list that cannot be written is only reported: the
/// project is open either way.
pub fn add(home: &Path, dir: &Path) {
    let Some(d) = listable(home, dir) else { return };
    let mut list = read(home);
    list.retain(|p| *p != d);
    list.insert(0, d);
    list.truncate(MAX_RECENT);
    let names: Vec<String> = list.iter().map(|p| shown(p)).collect();
    let text = serde_json::to_string_pretty(&names).unwrap_or_default();
    let r = std::fs::create_dir_all(home.join(".dagon"))
        .map_err(|e| e.to_string())
        .and_then(|_| crate::paths::write_atomic(&file(home), text.as_bytes()));
    if r.is_err() {
        eprintln!("dagon serve: the recent projects list could not be saved");
    }
}

/// A path as the page shows it (forward slashes, no verbatim prefix).
pub fn shown(p: &Path) -> String {
    crate::config::forward_path(p)
}

/// The list for GET /v1/project.
pub fn json(home: &Path) -> Value {
    Value::Array(
        read(home)
            .iter()
            .map(|p| {
                json!({
                    "path": shown(p),
                    "name": p.file_name().map(|n| n.to_string_lossy().into_owned()),
                })
            })
            .collect(),
    )
}
