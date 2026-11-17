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

fn web_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "pkg" {
                web_sources(&p, out);
            }
        } else if (name.ends_with(".ts") || name.ends_with(".svelte"))
            && !name.ends_with(".test.ts")
            && !name.ends_with(".d.ts")
        {
            out.push(p);
        }
    }
}

/// Whether a line of the page's source gives a command, status or field
/// its number: a name starting with `CMD`, `STATUS_`, `FIELD` or `F_`, or
/// (`entries`: in the files that speak the wire) an all-capitals command
/// or status name, set to a number with `=` or `:`.
fn names_a_wire_number(line: &str, entries: bool) -> bool {
    const ENTRY: [&str; 9] = [
        "CMD",
        "STATUS",
        "HELLO",
        "RELAY",
        "XFER",
        "REBOOT",
        "CONFIRM",
        "SHOW",
        "SET_PENDING",
    ];
    let b = line.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if !word(b[i]) || (i > 0 && (word(b[i - 1]) || b[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word(b[i]) {
            i += 1;
        }
        let name = &line[start..i];
        let wire = ["CMD", "STATUS_", "FIELD", "F_"]
            .iter()
            .any(|p| name.starts_with(p));
        let capitals = name.bytes().all(|c| c.is_ascii_uppercase() || c == b'_')
            && ENTRY.iter().any(|p| name.starts_with(p));
        if !(wire || (entries && capitals)) {
            continue;
        }
        let rest = line[i..].trim_start();
        let value = match rest.as_bytes().first() {
            Some(b'=') if !rest.starts_with("==") && !rest.starts_with("=>") => &rest[1..],
            Some(b':') => &rest[1..],
            _ => continue,
        };
        if value.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// The page takes every command, status and field number from dagon-proto
/// through dagon-wasm: its source writes none, and names no field at all.
#[test]
fn the_page_holds_no_command_status_or_field_number() {
    // The rule itself: what it catches and what it lets through.
    for planted in [
        "const CMD_XFER_BEGIN = 0x10;",
        "  HELLO: 0x01,",
        "const STATUS_ARG = 3;",
        "const F_SLOT = 1;",
        "export const CMD = { STATUS: 2 };",
    ] {
        assert!(names_a_wire_number(planted, true), "not caught: {planted}");
    }
    for fine in [
        "    get HELLO() {",
        "    case CMD.HELLO:",
        "    if (frame.cmd === CMD.REBOOT && body[0] === STATUS.OK) {",
        "const HDR_VERSION_AT = 0x018;",
        "  const status = 0;",
    ] {
        assert!(!names_a_wire_number(fine, true), "caught: {fine}");
    }
    let mut files = Vec::new();
    web_sources(&root().join("web/src"), &mut files);
    assert!(files.len() > 20, "the page's source was not found");
    let mut hits = Vec::new();
    for p in files {
        let path = p.strip_prefix(root()).unwrap().to_string_lossy();
        let name = path.replace(std::path::MAIN_SEPARATOR, "/");
        let speaks_wire = name.contains("/serial/") || name.contains("/wasm/");
        let text = std::fs::read_to_string(&p).unwrap();
        for (i, l) in text.lines().enumerate() {
            let code = l.trim_start();
            if code.starts_with("//") || code.starts_with("*") || code.starts_with("/*") {
                continue;
            }
            let field_code = [
                "payload_read",
                "PayloadWriter",
                "readPayload",
                "writePayload",
            ]
            .iter()
            .any(|w| l.contains(w));
            if names_a_wire_number(l, speaks_wire) || field_code {
                hits.push(format!("{name}:{}: {}", i + 1, code));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "wire numbers written in the page:\n{}",
        hits.join("\n")
    );
}
