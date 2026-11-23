//! The committed catalogs equal what the code generates; every crate
//! that declares machines is registered; the registry has no conflicts.
//!
//! `DAGON_BLESS=1` writes the generated documents instead of comparing.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use dagon_catalog::{
    conflicts, design_ports_table, events_md, gaps_md, machines, ports_md, InTree, Lang, REGISTRY,
};
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

/// The Japanese of what the catalogs say, keyed by their English.
const JA_TABLE: &str = "site/i18n/catalog-ja.toml";

fn ja_table() -> BTreeMap<String, String> {
    toml::from_str(&read(JA_TABLE)).unwrap_or_else(|e| panic!("{JA_TABLE}: {e}"))
}

/// `make` run with the Japanese table; the English it asked for and the table lacks fail.
fn with_japanese<T>(make: impl FnOnce(Lang<'_>) -> T) -> (T, BTreeSet<String>) {
    let table = ja_table();
    let asked = RefCell::new(BTreeSet::new());
    let look_up = |en: &str| {
        asked.borrow_mut().insert(en.to_string());
        table.get(en).cloned().unwrap_or_else(|| en.to_string())
    };
    let made = make(Lang::Ja(&look_up));
    (made, asked.into_inner())
}

/// Every `.rs` file under `dir`, build output and vendored trees left out.
fn rust_files(dir: &std::path::Path) -> Vec<PathBuf> {
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
    let mut files = Vec::new();
    walk(dir, &mut files);
    files
}

#[test]
fn events_md_is_generated() {
    let (ja, _) = with_japanese(|lang| events_md(&machines(), lang));
    check("site/src/content/docs/reference/events.md", &ja);
    check(
        "site/src/content/docs/en/reference/events.md",
        &events_md(&machines(), Lang::En),
    );
}

#[test]
fn ports_md_is_generated() {
    let (ja, _) = with_japanese(|lang| ports_md(PORTS, lang));
    check("site/src/content/docs/reference/ports.md", &ja);
    check(
        "site/src/content/docs/en/reference/ports.md",
        &ports_md(PORTS, Lang::En),
    );
}

/// Section 2.1 of the design holds the port table generated from PORTS.
#[test]
fn the_design_port_table_is_generated() {
    const BEGIN: &str = "<!-- generated from dagon_events::PORTS -->\n";
    const END: &str = "<!-- end of generated table -->\n";
    let (ja, _) = with_japanese(|lang| design_ports_table(PORTS, lang));
    for (design, table) in [
        ("site/src/content/docs/explanation/lifecycle-events.md", ja),
        (
            "site/src/content/docs/en/explanation/lifecycle-events.md",
            design_ports_table(PORTS, Lang::En),
        ),
    ] {
        let text = read(design);
        let a = text.find(BEGIN).expect("the design's table markers") + BEGIN.len();
        let b = a + text[a..].find(END).expect("the design's end marker");
        let mut whole = text.clone();
        whole.replace_range(a..b, &table);
        check(design, &whole);
    }
}

/// The limitations page lists what the tree lacks of the catalog, between
/// its markers; the rest of the page is written by hand.
#[test]
fn the_limitations_gap_list_is_generated() {
    const BEGIN: &str =
        "<!-- generated from dagon_events::PORTS: DAGON_BLESS=1 cargo test -p dagon-catalog --test docs -->\n";
    const END: &str = "<!-- end of generated list -->\n";
    let (ja, _) = with_japanese(|lang| gaps_md(PORTS, lang));
    for (page, list) in [
        ("site/src/content/docs/explanation/limitations.md", ja),
        (
            "site/src/content/docs/en/explanation/limitations.md",
            gaps_md(PORTS, Lang::En),
        ),
    ] {
        let text = read(page);
        let a = text
            .find(BEGIN)
            .unwrap_or_else(|| panic!("{page}: no gap list marker {BEGIN}"))
            + BEGIN.len();
        let b = a + text[a..].find(END).expect("the gap list's end marker");
        let mut whole = text.clone();
        whole.replace_range(a..b, &list);
        check(page, &whole);
    }
}

/// What the catalog says the tree has, named as code so the build checks
/// it: a port's path as a trait bound (this file stops building when the
/// trait is gone, private or no longer a trait), a part's path by `use`.
/// Evaluates to the paths as written.
macro_rules! named_in_code {
    (ports: $($t:path),* ; parts: $($($p:ident)::+),* $(,)?) => {{
        $({
            fn _is_a_public_trait<T: ?Sized + $t>() {}
        })*
        $({
            #[allow(unused_imports)]
            use $($p)::+ as _;
        })*
        [$(stringify!($t)),*, $(stringify!($($p)::+)),*]
    }};
}

/// The catalog's `Has` paths are exactly those the build checks above, so
/// a path added to the catalog without its check, or one left here after
/// the catalog drops it, fails.
#[test]
fn what_the_catalog_says_the_tree_has_is_checked_by_the_build() {
    let checked: BTreeSet<String> = named_in_code!(
        ports:
            dagon_port::Link,
            dagon_host::Transport,
            dagon_format::SigVerifier,
            dagon_port::Watchdog,
            dagon_agent::SlotStore,
            dagon_events::Extension,
            dagon_events::BoardBringUp,
            dagon_port::NodeLink;
        parts:
            dagon_agent::image,
            dagon_agent::session,
            dagon_events::Composed,
    )
    .iter()
    .map(|s| s.replace(' ', ""))
    .collect();
    let mut catalog = BTreeSet::new();
    for p in PORTS {
        if let InTree::Has(paths) = p.in_tree {
            assert!(!paths.is_empty(), "{}", p.id);
            catalog.extend(paths.iter().map(|s| s.to_string()));
        }
    }
    assert_eq!(catalog, checked);
}

/// A new or changed description fails until it is translated, and the
/// table holds nothing the catalogs no longer say.
#[test]
fn the_japanese_table_is_exactly_what_the_catalogs_say() {
    let (_, asked) = with_japanese(|lang| {
        events_md(&machines(), lang);
        ports_md(PORTS, lang);
        design_ports_table(PORTS, lang);
        gaps_md(PORTS, lang);
    });
    let have: BTreeSet<String> = ja_table().into_keys().collect();
    let missing: Vec<_> = asked.difference(&have).collect();
    let unused: Vec<_> = have.difference(&asked).collect();
    assert!(
        missing.is_empty() && unused.is_empty(),
        "{JA_TABLE}: add a Japanese value for missing, drop unused\nmissing:\n{}\nunused:\n{}",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        unused
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn the_registry_has_no_conflicts() {
    assert_eq!(conflicts(&machines()), Vec::<String>::new());
}

/// Every machine's transition table, generated from its declaration and
/// used by the code that decides, says what its catalog says.
#[test]
fn every_generated_transition_table_agrees_with_its_catalog() {
    for c in machines() {
        let states = u8::try_from(c.states.len()).unwrap();
        for (i, state) in (0..states).zip(c.states) {
            for e in c.entries {
                let named = e.from.contains(&"*") || e.from.contains(state);
                assert_eq!(
                    c.leaves(i, e.code),
                    Some(named),
                    "{}.{}: {} from {state}",
                    c.layer.name(),
                    c.machine,
                    e.trigger
                );
            }
        }
        assert_eq!(c.leaves(states, c.entries[0].code), None, "{}", c.machine);
    }
}

/// Events carry their layer and code; the host's display names them as
/// the Events page does (`layer.kind`), from the same catalogs.
#[test]
fn the_display_name_of_every_kind_is_its_catalog_name() {
    for c in machines() {
        for e in c.entries {
            let want = format!("{}.{}", c.layer.name(), e.kind);
            assert_eq!(
                dagon_catalog::name(c.layer, e.code).as_deref(),
                Some(&*want)
            );
        }
    }
    assert_eq!(dagon_catalog::name(Layer::App, 0xEE), None);
    assert_eq!(
        dagon_catalog::BOOT_OVERRAN,
        dagon_events::boot_extensions::ExtensionOverran::ENTRY.code
    );
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
    // A crate that depends on this registry reads the catalogs; it cannot
    // be listed here (that would be a dependency cycle), so it declares no
    // machine: its sources never invoke `state_machine!`.
    let reads_registry = |name: &str| deps.get(name).is_some_and(|d| d.contains(&"dagon-catalog"));
    let mut machine_crates = BTreeSet::new();
    for id in meta["workspace_members"].as_array().unwrap() {
        let name = name_of[id.as_str().unwrap()];
        if reads_registry(name) {
            let manifest = meta["packages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == *id)
                .and_then(|p| p["manifest_path"].as_str())
                .unwrap();
            let src = PathBuf::from(manifest).with_file_name("src");
            let declares = rust_files(&src).into_iter().find(|f| {
                std::fs::read_to_string(f)
                    .unwrap_or_default()
                    .contains(concat!("state_machine", "!"))
            });
            assert_eq!(
                declares, None,
                "{name} reads the registry and cannot declare machines"
            );
            continue;
        }
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
        concat!("_", "_sm_id"),
        concat!("_", "_transits"),
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
    let own = root()
        .join("crates/dagon-events/src")
        .canonicalize()
        .unwrap();
    let files = rust_files(&root());
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
    let ja = events_md(&[one::CATALOG], Lang::Ja(&|en| format!("訳 {en}")));
    assert!(ja.contains(
        "## レイヤー`boot`\n\n### 状態機械`one`\n\n訳 First.\n\n状態: `A` (初期状態), `B`。"
    ));
    assert!(ja.contains("| action | 訳 Went. |"));
    let md = events_md(&[one::CATALOG], Lang::En);
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
