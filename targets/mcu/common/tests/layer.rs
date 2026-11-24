//! The driver helpers sit below the node's services: this package depends
//! on the port, protocol and format crates only, and no module names the
//! agent, the events, the boot core logic or `dagon-node` (where the
//! services are). A boot image that takes the helpers takes none of them.

use std::path::Path;

/// The crates the helpers may depend on (dev-dependencies aside).
const ALLOWED: &[&str] = &["dagon-format", "dagon-port", "dagon-proto", "embassy-sync"];

/// The service crates, as code names them.
const SERVICES: &[&str] = &["dagon_agent", "dagon_events", "dagon_core", "dagon_node"];

fn dependencies(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if in_deps && !line.starts_with('#') {
            if let Some((name, _)) = line.split_once('=') {
                out.push(name.trim().to_string());
            }
        }
    }
    out
}

/// Lines of `src` that name a service crate, outside comments.
fn service_uses(src: &str) -> Vec<String> {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .filter(|l| SERVICES.iter().any(|s| l.contains(s)))
        .map(String::from)
        .collect()
}

#[test]
fn the_helpers_depend_on_no_service() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let deps = dependencies(&manifest);
    let extra: Vec<_> = deps
        .iter()
        .filter(|d| !ALLOWED.contains(&d.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "dependencies beyond {ALLOWED:?}: {extra:?}"
    );
    assert!(deps.iter().any(|d| d == "dagon-port"), "{deps:?}");
    let mut found = Vec::new();
    for e in std::fs::read_dir(dir.join("src")).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        for l in service_uses(&text) {
            found.push(format!("{}: {l}", e.file_name().to_string_lossy()));
        }
    }
    assert!(found.is_empty(), "the helpers use a service: {found:#?}");
}

#[test]
fn a_planted_service_use_is_found() {
    assert_eq!(
        service_uses("use dagon_agent::Agent;\n// dagon_node::serve\n"),
        ["use dagon_agent::Agent;"]
    );
    let m = "[dependencies]\ndagon-port = { path = \"x\" }\n# note\n\n[dev-dependencies]\ndagon-sim = 1\n";
    assert_eq!(dependencies(m), ["dagon-port"]);
}
