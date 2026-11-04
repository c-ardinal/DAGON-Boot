//! Embeds the builtin definitions: every `targets/mcu/<id>/dagon-mcu.toml`
//! and `targets/boards/<id>/dagon-board.toml` of this source tree, in
//! folder-name order (the order a source tree loads in), with the text
//! files an MCU definition names (`partitions`). Adding a package is
//! adding its folder; nothing lists them by hand.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn packages(dir: &Path, file: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(file).is_file())
        .collect();
    found.sort();
    found
}

fn lit(p: &Path) -> String {
    format!("{:?}", p.display().to_string())
}

fn label(root: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(root).expect("package inside the tree");
    let parts: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
    format!("{:?}", parts.join("/"))
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mcu_dir = root.join("targets/mcu");
    let board_dir = root.join("targets/boards");
    println!("cargo:rerun-if-changed={}", mcu_dir.display());
    println!("cargo:rerun-if-changed={}", board_dir.display());

    // Every file embedded, relative to the tree (`BUILTIN_FILES`).
    let mut embedded: Vec<String> = Vec::new();
    let rel = |p: &Path| {
        let parts: Vec<_> = p
            .strip_prefix(&root)
            .expect("inside the tree")
            .iter()
            .map(|c| c.to_string_lossy().to_string())
            .collect();
        parts.join("/")
    };
    let mut out = String::from("pub(super) const BUILTIN_MCUS: &[Embedded] = &[\n");
    for dir in packages(&mcu_dir, "dagon-mcu.toml") {
        let file = dir.join("dagon-mcu.toml");
        let text = std::fs::read_to_string(&file).expect("definition");
        let table: toml::Table = text
            .parse()
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let mut files = String::new();
        embedded.push(rel(&file));
        if let Some(name) = table.get("partitions").and_then(|v| v.as_str()) {
            let _ = write!(files, "({name:?}, include_str!({})),", lit(&dir.join(name)));
            embedded.push(rel(&dir.join(name)));
        }
        let _ = writeln!(
            out,
            "    ({}, include_str!({}), &[{files}]),",
            label(&root, &dir),
            lit(&file)
        );
    }
    out.push_str("];\n\npub(super) const BUILTIN_BOARDS: &[(&str, &str)] = &[\n");
    for dir in packages(&board_dir, "dagon-board.toml") {
        embedded.push(rel(&dir.join("dagon-board.toml")));
        let _ = writeln!(
            out,
            "    ({}, include_str!({})),",
            label(&root, &dir),
            lit(&dir.join("dagon-board.toml"))
        );
    }
    out.push_str("];\n\n/// Every file of the tree embedded here, relative to the tree.\npub const BUILTIN_FILES: &[&str] = &[\n");
    for f in &embedded {
        let _ = writeln!(out, "    {f:?},");
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("builtin_defs.rs");
    std::fs::write(dest, out).expect("write builtin_defs.rs");
}
