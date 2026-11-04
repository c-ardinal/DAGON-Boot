//! Only a node's start and its serving task hold the agent: no public
//! function of an MCU package returns one (or a value holding one), and
//! no public field holds one, so application code cannot send requests or
//! confirm through it. Apps get the node's events (`AppEvents`) and
//! `Confirm` from the start, nothing else.

use std::path::{Path, PathBuf};

/// Types that are, or hold, a node's agent.
const AGENT_TYPES: &[&str] = &["Agent<", "AppAgent", "Opened"];

fn mcu_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let skip = matches!(
            e.file_name().to_str(),
            Some("target" | "vendor" | "tests" | "bootcore" | "stage0" | "template")
        );
        if p.is_dir() && !skip {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The signature of every `pub fn` (up to its body), and every `pub`
/// field or binding line, of `text`.
fn public_items(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("pub fn ") {
        let sig = &rest[at..];
        let end = sig.find(['{', ';']).unwrap_or(sig.len());
        out.push(sig[..end].split_whitespace().collect::<Vec<_>>().join(" "));
        rest = &sig[end..];
    }
    out.extend(
        text.lines()
            .map(str::trim)
            .filter(|l| l.starts_with("pub ") && l.contains(": ") && !l.contains("fn "))
            .map(str::to_string),
    );
    out
}

/// Whether `item` hands out an agent: a function returning one, or a
/// public field typed as one.
fn hands_out_agent(item: &str) -> bool {
    let handed = match item.split_once("->") {
        Some((_, ret)) => ret,
        None if item.starts_with("pub fn") => return false,
        None => item.split_once(": ").map_or("", |(_, t)| t),
    };
    AGENT_TYPES.iter().any(|t| handed.contains(t))
}

#[test]
fn the_rule_sees_what_it_must() {
    let src = "pub fn agent(tag: &str) -> (AppAgent, AppEvents) {\n}\n\
               pub struct Opened {\n    pub agent: AppAgent,\n}\n\
               pub fn finish(agent: &mut Agent<S, X>) -> bool {\n}\n\
               pub fn start(\n    a: App,\n) -> Result<AppEvents, LogError> {\n}\n";
    let flagged: Vec<String> = public_items(src)
        .into_iter()
        .filter(|i| hands_out_agent(i))
        .collect();
    assert_eq!(
        flagged,
        [
            "pub fn agent(tag: &str) -> (AppAgent, AppEvents)",
            "pub agent: AppAgent,"
        ]
    );
}

#[test]
fn no_public_path_hands_out_a_nodes_agent() {
    let root = mcu_dir();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(files.iter().any(|f| f.ends_with("node.rs")));
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for item in public_items(&text) {
            if hands_out_agent(&item) {
                found.push(format!("{}: {item}", f.display()));
            }
        }
    }
    assert!(found.is_empty(), "public paths to the agent: {found:#?}");
}
