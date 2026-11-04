use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;

fn root() -> PathBuf {
    PathBuf::from(crate::dagonsrc::BUILT_FROM)
}

/// Every Cargo package of the tree outside build output: name -> version.
fn manifests() -> BTreeMap<String, String> {
    fn walk(dir: &Path, depth: u8, out: &mut BTreeMap<String, String>) {
        let m = dir.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&m) {
            let t: toml::Table = toml::from_str(&text).unwrap();
            if let Some(p) = t.get("package").and_then(|p| p.as_table()) {
                let v = p
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(shared)");
                out.insert(p["name"].as_str().unwrap().into(), v.into());
            }
        }
        if depth == 0 {
            return;
        }
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let n = e.file_name();
            let skip = matches!(n.to_str(), Some("target" | "node_modules" | ".git" | "out"));
            if e.path().is_dir() && !skip {
                walk(&e.path(), depth - 1, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    for top in ["crates", "targets", "samples"] {
        walk(&root().join(top), 3, &mut out);
    }
    out
}

#[test]
fn the_table_formats_are_the_numbers_in_the_code() {
    let code: BTreeMap<String, u32> = formats().iter().map(|(c, n)| (c.to_string(), *n)).collect();
    assert_eq!(compat().formats, code);
}

#[test]
fn every_component_reads_the_current_number_of_each_contract_it_names() {
    let t = compat();
    for c in &t.component {
        for (contract, nums) in &c.formats {
            let cur = t.formats.get(contract);
            assert!(cur.is_some(), "{}: unknown contract {contract}", c.name);
            assert!(
                nums.contains(cur.unwrap()),
                "{}: {contract} {nums:?}",
                c.name
            );
        }
    }
    let cli = t.component.iter().find(|c| c.name == "cli").unwrap();
    let all: BTreeMap<String, Vec<u32>> = t
        .formats
        .iter()
        .map(|(c, n)| (c.clone(), vec![*n]))
        .collect();
    assert_eq!(cli.formats, all, "the CLI reads and writes every contract");
}

#[test]
fn component_versions_are_the_package_versions() {
    let m = manifests();
    let t = compat();
    let mut listed = BTreeSet::new();
    for c in &t.component {
        semver_ok(&c.version);
        for p in &c.packages {
            assert_eq!(m.get(p), Some(&c.version), "{} package {p}", c.name);
            listed.insert(p.clone());
        }
        for p in &c.paths {
            assert!(root().join(p).exists(), "{}: no {p}", c.name);
        }
    }
    let cli = t.component.iter().find(|c| c.name == "cli").unwrap();
    assert_eq!(cli.version, env!("CARGO_PKG_VERSION"));
    let web: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("web/package.json")).unwrap())
            .unwrap();
    assert_eq!(
        web["version"],
        cli.version.as_str(),
        "the Console is the CLI's"
    );
    // Packages left out of the table are samples, bring-up and test tools:
    // they stay 0.0.0.
    for (name, v) in &m {
        if !listed.contains(name) {
            assert_eq!(v, "0.0.0", "{name} {v} is not in compat.toml");
        }
    }
}

fn semver_ok(v: &str) {
    let (core, pre) = v.split_once('-').unwrap_or((v, ""));
    let parts: Vec<_> = core.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok()),
        "{v}"
    );
    assert!(
        pre.is_empty() || pre.starts_with("rc."),
        "{v}: only -rc.N pre-releases"
    );
}

#[test]
fn version_text_names_the_cli_version_and_every_format() {
    let v = version_lines();
    assert!(v.starts_with(&format!("dagon {}\n", env!("CARGO_PKG_VERSION"))));
    for (c, n) in formats() {
        assert!(v.contains(&format!("{c} {n}")), "{v}");
    }
    assert!(known_text().contains("boot-rp2040 0.1.0"));
}

#[test]
fn unknown_format_numbers_are_refused_with_both_numbers() {
    let mut raw = vec![0u8; 16];
    raw[0..4].copy_from_slice(&dagon_format::bundle::MAGIC);
    raw[4] = dagon_format::bundle::BN_VERSION as u8;
    assert_eq!(check_number("bundle", "x.dgb", &raw), Ok(()));
    raw[4] = 7;
    let e = check_number("bundle", "x.dgb", &raw).unwrap_err();
    assert!(
        e.contains("bundle format 7") && e.contains("reads bundle format 1"),
        "{e}"
    );
    // Not that format at all: the parser says so, not this check.
    raw[0] = b'X';
    assert_eq!(check_number("bundle", "x.dgb", &raw), Ok(()));
}

#[test]
fn a_device_on_another_wire_protocol_is_warned_about() {
    assert_eq!(wire_warning("hub", 1), None);
    let w = wire_warning("hub", 2).unwrap();
    assert!(
        w.contains("wire protocol 2") && w.contains("speaks 1"),
        "{w}"
    );
}
