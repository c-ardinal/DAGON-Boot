//! The committed catalogs equal what the code generates; every machine
//! declared in the repository is registered; the registry has no
//! conflicts.
//!
//! `DAGON_BLESS=1` writes the generated documents instead of comparing.

use std::path::{Path, PathBuf};

use dagon_catalog::{conflicts, design_ports_table, events_md, ports_md, MACHINES};
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
    check("docs/events.md", &events_md(MACHINES));
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
    assert_eq!(conflicts(MACHINES), Vec::<String>::new());
}

/// Every `state_machine!` declaration outside `tests/` folders, as
/// (file, machine, layer).
fn declarations() -> Vec<(String, String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = e.file_name();
            let skip = matches!(
                name.to_str(),
                Some("target" | "node_modules" | ".git" | "vendor" | "tests" | "pkg")
            );
            if p.is_dir() && !skip {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root(), &mut files);
    let mut found = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for (at, _) in code.match_indices("state_machine!") {
            let words: Vec<_> = code[at..].split_whitespace().take(12).collect();
            let i = words
                .iter()
                .position(|w| *w == "machine")
                .unwrap_or_else(|| panic!("{}: a state_machine! without `machine`", f.display()));
            assert_eq!(words.get(i + 2), Some(&"in"), "{}", f.display());
            found.push((
                f.strip_prefix(root()).unwrap_or(&f).display().to_string(),
                words[i + 1].to_string(),
                words[i + 3].trim_end_matches('{').to_string(),
            ));
        }
    }
    found
}

#[test]
fn every_declared_machine_is_registered() {
    let found = declarations();
    assert!(
        found.iter().any(|(_, m, _)| m == "decisions"),
        "the scan finds dagon-events' own machine: {found:?}"
    );
    for (file, machine, layer) in &found {
        assert!(
            MACHINES
                .iter()
                .any(|c| c.machine == *machine && format!("{:?}", c.layer) == *layer),
            "{file}: machine {machine} in {layer} is not in dagon_catalog::MACHINES"
        );
    }
    assert_eq!(found.len(), MACHINES.len(), "{found:?}");
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

#[test]
fn every_layer_has_a_port() {
    assert!(Layer::ALL
        .iter()
        .all(|l| PORTS.iter().any(|p| p.in_layer(*l))));
}
