//! The execution model's rule for DAGON's own code (site/src/content/docs/en/explanation/
//! lifecycle-events.md, section 12, rule 2) is enforced by clippy:
//! `targets/mcu/clippy.toml` refuses every way to mask interrupts in the MCU
//! packages (clippy resolves the path, so an alias or a wrapper does not
//! hide it). This test keeps that list in place and keeps the exemptions to
//! the one reviewed function that masks.

use std::path::{Path, PathBuf};

/// Every masking primitive the MCU packages may reach.
const MASKING: &[&str] = &[
    "critical_section::with",
    "critical_section::acquire",
    "cortex_m::interrupt::free",
    "cortex_m::interrupt::disable",
    "embassy_sync::blocking_mutex::Mutex::lock",
];

/// Attributes that would lift the lint (itself, or a group holding it).
const LIFTS: &[&str] = &[
    "allow(clippy::disallowed_methods",
    "expect(clippy::disallowed_methods",
    "allow(clippy::style",
    "expect(clippy::style",
    "allow(clippy::all",
    "expect(clippy::all",
];

/// The functions allowed to mask, by file: each says why on itself.
const EXEMPT: &[(&str, &str)] = &[
    ("common/src/guard.rs", "fn with_state"),
    ("rp2040/src/relay/state.rs", "fn locked"),
];

fn mcu_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && !matches!(e.file_name().to_str(), Some("target" | "vendor")) {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn clippy_refuses_every_masking_primitive() {
    let conf = std::fs::read_to_string(mcu_dir().join("clippy.toml")).unwrap();
    let table: toml_like::Paths = toml_like::paths(&conf);
    for m in MASKING {
        assert!(
            table.iter().any(|p| p == m),
            "targets/mcu/clippy.toml must refuse {m}"
        );
    }
}

#[test]
fn only_the_reviewed_function_may_mask() {
    let root = mcu_dir();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let lifts = LIFTS.iter().any(|x| l.contains(x));
            if lifts && !rel.ends_with("tests/masking.rs") {
                let next = lines.get(i + 1).copied().unwrap_or("").trim().to_string();
                found.push((rel.clone(), next));
            }
        }
    }
    let exempt: Vec<(String, String)> = EXEMPT
        .iter()
        .map(|(f, n)| (f.to_string(), n.to_string()))
        .collect();
    let unexpected: Vec<_> = found
        .iter()
        .filter(|(f, next)| {
            !exempt
                .iter()
                .any(|(ef, en)| f == ef && next.contains(en.as_str()))
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "masking allowed outside the reviewed function: {unexpected:?}"
    );
    assert_eq!(found.len(), EXEMPT.len(), "{found:?}");
}

/// The `path = "…"` entries of a clippy.toml, without a TOML parser.
mod toml_like {
    pub type Paths = Vec<String>;

    pub fn paths(text: &str) -> Paths {
        text.split("path = \"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .map(str::to_string)
            .collect()
    }
}
