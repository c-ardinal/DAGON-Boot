//! Embeds the built browser page (web/dist) into the dagon executable, so
//! `dagon serve` needs no Node or wasm-pack on the developer's machine.
//!
//! The page is built separately into page/ (`python scripts/page.py
//! build`, committed); this script only includes what page/ holds, in
//! sorted order, and never runs npm. Without it the table is empty and
//! `dagon serve` falls back to a page folder on disk. `DAGON_WEB_DIST`
//! points at another built page.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if path.is_dir() {
            files(&path, &rel, out);
        } else if path.is_file() {
            out.push((rel, path));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_WEB_DIST");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let dist = env::var_os("DAGON_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("page"));
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut list = Vec::new();
    files(&dist, "", &mut list);
    list.retain(|(rel, _)| rel != "SOURCE");
    let mut code = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &list {
        let abs = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        code.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            abs.to_string_lossy()
        ));
    }
    code.push_str("];\n");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets it"));
    fs::write(out_dir.join("web_assets.rs"), code).expect("write web_assets.rs");

    // Project templates for `dagon gen`: templates/<package>/<files>.
    let tdir = manifest.join("templates");
    println!("cargo:rerun-if-changed={}", tdir.display());
    let mut code = String::from("pub static TEMPLATES: &[(&str, &str, &str)] = &[\n");
    let mut packages: Vec<_> = fs::read_dir(&tdir)
        .map(|d| d.filter_map(Result::ok).collect())
        .unwrap_or_default();
    packages.sort_by_key(|e| e.file_name());
    for p in packages.iter().filter(|p| p.path().is_dir()) {
        let name = p.file_name().to_string_lossy().into_owned();
        let mut list = Vec::new();
        files(&p.path(), "", &mut list);
        for (rel, path) in list {
            let abs = fs::canonicalize(&path).unwrap_or(path);
            code.push_str(&format!(
                "    ({name:?}, {rel:?}, include_str!({:?})),\n",
                abs.to_string_lossy()
            ));
        }
    }
    code.push_str("];\n");
    fs::write(out_dir.join("templates.rs"), code).expect("write templates.rs");
}
