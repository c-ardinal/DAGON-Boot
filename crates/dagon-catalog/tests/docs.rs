//! The committed catalogs equal what the code generates; every crate
//! that declares machines is registered; the registry has no conflicts.
//!
//! `DAGON_BLESS=1` writes the generated documents instead of comparing.

use std::collections::{BTreeMap, BTreeSet};
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

/// Workspace packages whose resolved normal dependencies reach
/// `dagon-events` other than through this registry: each declares
/// machines and must be in REGISTRY, once, by its own `MACHINES`.
#[test]
fn every_machine_crate_is_registered_once() {
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(root())
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let name_of: BTreeMap<&str, &str> = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str().unwrap()))
        .collect();
    // Normal (shipped) dependency edges, by package name.
    let mut deps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in meta["resolve"]["nodes"].as_array().unwrap() {
        let from = name_of[node["id"].as_str().unwrap()];
        for d in node["deps"].as_array().unwrap() {
            let normal = d["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k["kind"].is_null() || k["kind"] == "build");
            if normal {
                deps.entry(from)
                    .or_default()
                    .push(name_of[d["pkg"].as_str().unwrap()]);
            }
        }
    }
    let reaches_events = |start: &str| {
        let mut todo = vec![start];
        let mut seen = BTreeSet::new();
        while let Some(n) = todo.pop() {
            if n == "dagon-events" {
                return true;
            }
            if n == "dagon-catalog" || !seen.insert(n) {
                continue;
            }
            todo.extend(deps.get(n).into_iter().flatten().copied());
        }
        false
    };
    let mut machine_crates = BTreeSet::new();
    for id in meta["workspace_members"].as_array().unwrap() {
        let name = name_of[id.as_str().unwrap()];
        if name == "dagon-events" || (name != "dagon-catalog" && reaches_events(name)) {
            machine_crates.insert(name.to_string());
        }
    }
    let listed: Vec<_> = REGISTRY.iter().map(|m| m.krate().to_string()).collect();
    let unique: BTreeSet<_> = listed.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        listed.len(),
        "a crate is listed twice: {listed:?}"
    );
    assert_eq!(
        unique, machine_crates,
        "dagon_catalog::REGISTRY must list the MACHINES of every crate that reaches dagon-events"
    );
    // Each entry is that crate's own static, not a copy.
    assert!(std::ptr::eq(REGISTRY[0], &dagon_events::MACHINES));
    for (i, a) in REGISTRY.iter().enumerate() {
        assert!(REGISTRY[i + 1..].iter().all(|b| !std::ptr::eq(*a, *b)));
    }
    assert!(!machines().is_empty());
}

/// dagon-events' hidden items (its double-underscore macros, module,
/// methods and fields) are reached only through its own macro
/// expansions: no source file outside dagon-events' `src` names them.
#[test]
fn hidden_items_are_used_only_by_dagon_events() {
    // Spelled in two parts so this file does not name them itself.
    const HIDDEN: &[&str] = &[
        concat!("_", "_machine"),
        concat!("_", "_sm_from"),
        concat!("_", "_dagon_events_state_machine_once_per_crate"),
        concat!("_", "_private"),
        concat!("_", "_new"),
        concat!("_", "_replay"),
        concat!("_", "_leaves"),
        concat!("_", "_id_at"),
        concat!("_", "_accept_at"),
        concat!("_", "_reboot_at"),
        concat!("_", "_check_at"),
    ];
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let skip = matches!(
                e.file_name().to_str(),
                Some("target" | "node_modules" | ".git" | "vendor" | "pkg")
            );
            if p.is_dir() && !skip {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let own = root()
        .join("crates/dagon-events/src")
        .canonicalize()
        .unwrap();
    let mut files = Vec::new();
    walk(&root(), &mut files);
    let mut found = Vec::new();
    for f in files {
        if f.canonicalize().unwrap().starts_with(&own) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        for name in HIDDEN {
            if text.contains(name) {
                found.push(format!("{}: {name}", f.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "hidden dagon-events items used outside it: {found:?}"
    );
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
