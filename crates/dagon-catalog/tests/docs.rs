//! The committed catalogs equal what the code generates; every crate
//! that declares machines is registered; the registry has no conflicts.
//!
//! `DAGON_BLESS=1` writes the generated documents instead of comparing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use dagon_catalog::{conflicts, design_ports_table, events_md, machines, ports_md, REGISTRY};
use dagon_events::{state_machine, Layer, PORTS};

const BLESS: &str = "`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel))
        .unwrap_or_default()
        .replace("\r\n", "\n")
}

fn check(rel: &str, generated: &str) {
    if std::env::var_os("DAGON_BLESS").is_some() {
        std::fs::write(root().join(rel), generated).unwrap();
        return;
    }
    assert!(
        read(rel) == generated,
        "{rel} is not what the catalogs generate; run {BLESS} and commit the result"
    );
}

#[test]
fn events_md_is_generated() {
    check("docs/events.md", &events_md(&machines()));
}

#[test]
fn ports_md_is_generated() {
    check("docs/ports.md", &ports_md(PORTS));
}

/// Section 2.1 of the design holds the port table generated from PORTS.
#[test]
fn the_design_port_table_is_generated() {
    const DESIGN: &str = "docs/design/lifecycle-events.md";
    const BEGIN: &str = "<!-- generated from dagon_events::PORTS -->\n";
    const END: &str = "<!-- end of generated table -->\n";
    let text = read(DESIGN);
    let a = text.find(BEGIN).expect("the design's table markers") + BEGIN.len();
    let b = a + text[a..].find(END).expect("the design's end marker");
    let mut whole = text.clone();
    whole.replace_range(a..b, &design_ports_table(PORTS));
    check(DESIGN, &whole);
}

#[test]
fn the_registry_has_no_conflicts() {
    assert_eq!(conflicts(&machines()), Vec::<String>::new());
}

/// Every workspace package that depends on `dagon-events` (other than
/// this registry) declares machines and must be in REGISTRY; each crate
/// once.
#[test]
fn every_machine_crate_is_registered_once() {
    let out = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root())
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut machine_crates = BTreeSet::from(["dagon-events".to_string()]);
    for pkg in meta["packages"].as_array().unwrap() {
        let name = pkg["name"].as_str().unwrap();
        let uses_events = pkg["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "dagon-events" && d["kind"].is_null());
        if uses_events && name != "dagon-catalog" {
            machine_crates.insert(name.to_string());
        }
    }
    let listed: Vec<_> = REGISTRY.iter().map(|m| m.krate.to_string()).collect();
    let unique: BTreeSet<_> = listed.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        listed.len(),
        "a crate is listed twice: {listed:?}"
    );
    assert_eq!(
        unique, machine_crates,
        "dagon_catalog::REGISTRY must list the MACHINES of every crate that depends on dagon-events"
    );
    assert!(!machines().is_empty());
}

state_machine! {
    /// First.
    machine one in Boot {
        initial A;
        states { A, B { n: u8 } }
        transitions {
            /// Went.
            go(n: u8): A => B { n } emits Went = 0x01 { n: u8 } observe(live, replay) action;
            back(): * => A emits Back = 0x02 {} observe(host) retired;
        }
    }

    machine two in Boot {
        initial X;
        states { X }
        transitions {
            again(): X => X emits Back = 0x01 {} observe(live);
        }
    }
}

#[test]
fn conflicts_across_machines_of_a_layer_are_named() {
    let c = conflicts(&[one::CATALOG, two::CATALOG, one::CATALOG]);
    assert!(
        c.contains(&"boot: code 0x01 is one.went and two.back".to_string()),
        "{c:?}"
    );
    assert!(
        c.contains(&"boot.back is emitted by one and two".to_string()),
        "{c:?}"
    );
    assert!(
        c.contains(&"boot: two machines named one".to_string()),
        "{c:?}"
    );
    assert!(conflicts(&[one::CATALOG]).is_empty());
}

#[test]
fn a_machine_renders_every_transition() {
    let md = events_md(&[one::CATALOG]);
    assert!(md
        .contains("## Layer `boot`\n\n### Machine `one`\n\nFirst.\n\nStates: `A` (initial), `B`."));
    assert!(md.contains(
        "| `boot.went` | 0x01 | `go`: A → B | `n: u8` (1 B) | live, replay | action | Went. |"
    ));
    assert!(md.contains("| `boot.back` | 0x02 | `back`: * → A | – | host | retired |  |"));
    assert!(!md.contains("## Layer `app`"));
}

#[test]
fn every_layer_has_a_port() {
    assert!(Layer::ALL
        .iter()
        .all(|l| PORTS.iter().any(|p| p.in_layer(*l))));
}
