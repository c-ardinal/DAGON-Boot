//! Writes the registry's list: every `[dependencies]` crate of this
//! package that invokes `state_machine!`, so a crate with machines is
//! listed by depending on it, with no list to edit. The macro's own crate
//! (dagon-events, which declares the layers' shared machines) comes first,
//! the others in the manifest's order.

use std::path::{Path, PathBuf};
use std::{env, fs};

const MACRO_CRATE: &str = "dagon-events";

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest = dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest).unwrap();
    let mut listed = Vec::new();
    for (name, path) in dependencies(&text) {
        let src = dir.join(path).join("src");
        println!("cargo:rerun-if-changed={}", src.display());
        if name == MACRO_CRATE {
            listed.insert(0, name);
        } else if invokes_macro(&src) {
            listed.push(name);
        }
    }
    assert_eq!(
        listed.first().map(String::as_str),
        Some(MACRO_CRATE),
        "{MACRO_CRATE} is a path dependency"
    );
    let idents: Vec<String> = listed.iter().map(|n| n.replace('-', "_")).collect();
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("registry.rs");
    fs::write(out, format!("registry![{}]\n", idents.join(", "))).unwrap();
}

/// `name = { path = "..." }` lines of the `[dependencies]` table, in order.
fn dependencies(manifest: &str) -> Vec<(String, String)> {
    manifest
        .lines()
        .skip_while(|l| l.trim() != "[dependencies]")
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter_map(|l| {
            let (name, rest) = l.split_once('=')?;
            let path = rest.split_once("path = \"")?.1.split_once('"')?.0;
            Some((name.trim().to_string(), path.to_string()))
        })
        .collect()
}

fn invokes_macro(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        if p.is_dir() {
            invokes_macro(&p)
        } else {
            p.extension().is_some_and(|x| x == "rs")
                && fs::read_to_string(&p).is_ok_and(|s| s.contains(concat!("state_machine", "!")))
        }
    })
}
