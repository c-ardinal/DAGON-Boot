//! One field reader and one command table for the whole tree: no source
//! outside dagon-proto looks into a frame's payload bytes, reads a varint,
//! or keeps its own list of command numbers or resend rules.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !matches!(name.as_str(), "target" | "tests" | "node_modules" | ".git") {
                rust_sources(&p, out);
            }
        } else if name.ends_with(".rs") && !name.starts_with("tests") {
            out.push(p);
        }
    }
}

/// Shipped source of every crate: test folders, test files and the
/// `#[cfg(test)]` tail of a file are left out.
fn shipped() -> Vec<(String, String)> {
    let mut files = Vec::new();
    for top in ["crates", "targets", "samples"] {
        rust_sources(&root().join(top), &mut files);
    }
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap().to_string();
            let name = p
                .strip_prefix(root())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (name, code)
        })
        .collect()
}

fn offenders(found: impl Fn(&str, &str) -> bool) -> Vec<String> {
    shipped()
        .iter()
        .flat_map(|(name, code)| {
            code.lines()
                .enumerate()
                .filter(|(_, l)| !l.trim_start().starts_with("//") && found(name, l))
                .map(|(i, l)| format!("{name}:{}: {}", i + 1, l.trim()))
                .collect::<Vec<_>>()
        })
        .collect()
}

const PROTO: &str = "crates/dagon-proto/src/";

/// A frame's `payload` goes whole to a dagon-proto reader (or on to the
/// next link); nothing indexes, measures or splits it.
#[test]
fn nothing_outside_dagon_proto_looks_into_a_payload() {
    const INTO: [&str; 9] = [
        ".payload[",
        ".payload.get(",
        ".payload.first(",
        ".payload.last(",
        ".payload.split",
        ".payload.is_empty(",
        ".payload.iter(",
        ".payload.chunks",
        ".payload.starts_with(",
    ];
    // Only files that use dagon-proto hold frames.
    let framed: Vec<String> = shipped()
        .into_iter()
        .filter(|(name, code)| !name.starts_with(PROTO) && code.contains("dagon_proto"))
        .map(|(name, _)| name)
        .collect();
    let hits =
        offenders(|name, l| framed.iter().any(|f| f == name) && INTO.iter().any(|p| l.contains(p)));
    assert!(
        hits.is_empty(),
        "payload bytes read by hand:\n{}",
        hits.join("\n")
    );
}

#[test]
fn one_varint_reader_and_one_field_reader() {
    let varints = offenders(|_, l| l.contains("fn varint") || l.contains("fn read_varint"));
    assert_eq!(varints.len(), 1, "{varints:?}");
    assert!(varints[0].starts_with("crates/dagon-proto/src/field.rs"));
    // The typed payloads hold no cursor of their own.
    let cursors = offenders(|name, l| {
        name.starts_with(PROTO) && !name.ends_with("field.rs") && l.contains("split_at(")
    });
    assert!(cursors.is_empty(), "{cursors:?}");
    let leb = offenders(|name, l| !name.starts_with(PROTO) && l.to_lowercase().contains("leb128"));
    assert!(leb.is_empty(), "{leb:?}");
}

#[test]
fn command_numbers_and_resend_rules_come_from_the_table() {
    let lists = offenders(|name, l| {
        l.contains("NEVER_RESENT")
            || (!name.starts_with(PROTO)
                && l.contains("const CMD_")
                && l.contains(": u8")
                && l.contains("0x"))
    });
    assert!(lists.is_empty(), "{lists:?}");
    // The only Cmd enum, and it is the table's.
    let enums = offenders(|_, l| l.contains("enum Cmd "));
    assert_eq!(enums.len(), 1, "{enums:?}");
    assert!(enums[0].starts_with("crates/dagon-proto/src/table.rs"));
}
