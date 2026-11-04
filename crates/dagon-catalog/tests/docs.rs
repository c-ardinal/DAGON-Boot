//! The committed catalogs equal what the code generates; the registry has
//! no conflicts; the design's port table agrees with the port catalog.
//!
//! `DAGON_BLESS=1` writes the generated documents instead of comparing.

use std::path::PathBuf;

use dagon_catalog::{conflicts, events_md, ports_md, MACHINES};
use dagon_events::{state_machine, Layer, Port, Trust, PORTS};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn check(rel: &str, generated: &str) {
    let path = root().join(rel);
    if std::env::var_os("DAGON_BLESS").is_some() {
        std::fs::write(&path, generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == generated,
        "{rel} is not what the catalogs generate; run \
         `DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` and commit the result"
    );
}

#[test]
fn events_md_is_generated() {
    check("docs/events.md", &events_md(MACHINES));
}

#[test]
fn ports_md_is_generated() {
    check("docs/ports.md", &ports_md(PORTS));
}

#[test]
fn the_registry_has_no_conflicts() {
    assert_eq!(conflicts(MACHINES), Vec::<String>::new());
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
}

state_machine! {
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

/// Section 2.1 of the design shows the port table; every row there must
/// be a port of the catalog with the same layers and trust, and every port
/// must appear in it.
#[test]
fn the_design_port_table_agrees_with_the_catalog() {
    let design = std::fs::read_to_string(root().join("docs/design/lifecycle-events.md")).unwrap();
    let start = design
        .find("| Port | Layers | Default | Trust policy |")
        .expect("the design's port table");
    let mut seen = Vec::new();
    for row in design[start..].lines().skip(2) {
        if !row.starts_with('|') {
            break;
        }
        let cols: Vec<_> = row.trim_matches('|').split(" | ").map(str::trim).collect();
        let layers: Vec<_> = cols[1].split(", ").collect();
        let trust = cols[3];
        // The names come before the parenthesised description.
        let names = cols[0].split(" (").next().unwrap();
        for id in names.split('`').skip(1).step_by(2) {
            let p = Port::find(id).unwrap_or_else(|| panic!("design port {id} is not in PORTS"));
            let names: Vec<_> = p.layers.iter().map(|d| d.layer.name()).collect();
            assert_eq!(names, layers, "{id}");
            let want = if trust.starts_with("trusted only") {
                Trust::TrustedOnly
            } else if trust.starts_with("untrusted, additive") {
                Trust::Additive
            } else {
                assert!(trust.starts_with("untrusted"), "{id}: {trust}");
                Trust::Untrusted
            };
            assert_eq!(p.trust, want, "{id}");
            seen.push(id);
        }
    }
    for p in PORTS.iter().filter(|p| p.trust.is_port()) {
        assert!(
            seen.contains(&p.id),
            "{} is not in the design's port table",
            p.id
        );
    }
    assert!(Layer::ALL
        .iter()
        .all(|l| PORTS.iter().any(|p| p.in_layer(*l))));
}
