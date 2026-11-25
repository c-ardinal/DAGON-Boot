//! The crates' logic names no MCU: what differs between MCUs comes from
//! their definitions, so a new MCU needs no edit here. A string literal
//! holding a built-in MCU id outside test code is allowed only in the
//! files below, which hold the sample 構成's own nodes or test tooling.

use std::path::{Path, PathBuf};

use dagon_topology::defs::Registry;

/// Files that name an MCU, and why that is not logic.
const SAMPLE_OR_TEST_ONLY: &[(&str, &str)] = &[
    ("dagon-sim/src/lib.rs", "the sample 構成's nodes"),
    ("dagon-sim/src/flash.rs", "the sample 構成's flashes"),
    ("dagon-sim/src/rom.rs", "the sample 構成's ROM loader"),
    ("dagon-sim/src/store.rs", "the sample 構成's stores"),
    ("dagon-cli/src/simrun.rs", "the sample 構成's images"),
    ("dagon-cli/src/demo.rs", "a test-only module"),
    ("dagon-cli/src/esp_image.rs", "used by tests only"),
    ("dagon-cli/src/esp_image_qemu.rs", "a test-only module"),
    (
        "dagon-cli/src/esp_image_qemu_bootcore.rs",
        "a test-only module",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Whether `file` is test code by its name (`tests.rs`, `tests_*.rs`, a
/// `tests` folder).
fn test_file(rel: &str) -> bool {
    rel.split('/')
        .any(|part| part == "tests" || part == "tests.rs" || part.starts_with("tests_"))
}

#[test]
fn no_logic_names_a_built_in_mcu() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ids: Vec<String> = Registry::builtin()
        .mcus()
        .iter()
        .map(|m| format!("\"{}\"", m.id))
        .collect();
    assert!(!ids.is_empty());
    let mut files = Vec::new();
    for krate in std::fs::read_dir(crates).unwrap().flatten() {
        let src = krate.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut named = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if test_file(&rel) || SAMPLE_OR_TEST_ONLY.iter().any(|(a, _)| *a == rel) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        // A file's own tests follow its code.
        let code = text.split("#[cfg(test)]").next().unwrap();
        for (i, line) in code.lines().enumerate() {
            let line = line.trim_start();
            if !line.starts_with("//") && ids.iter().any(|id| line.contains(id.as_str())) {
                named.push(format!("{rel}:{}: {line}", i + 1));
            }
        }
    }
    assert!(
        named.is_empty(),
        "an MCU is named in logic (read it from the MCU's definition):\n{}",
        named.join("\n")
    );
    // Every listed file still exists, so the list does not outlive its reasons.
    for (rel, why) in SAMPLE_OR_TEST_ONLY {
        assert!(crates.join(rel).is_file(), "{rel} ({why}) is gone");
    }
}
