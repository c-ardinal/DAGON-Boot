//! How many slots each image kind has, and what writes it: one slot (`a`)
//! or two (`a` and `b`) for any kind, a single slot only when another kind
//! writes it; per built-in MCU, every combination of one or two slots of
//! each kind it has.

use std::collections::BTreeMap;
use std::path::Path;

use super::tests::{mcu_err, FAKE_MCU};
use super::*;

/// The built-in definitions with `edit` applied to MCU `mcu`'s TOML, loaded
/// as the JSON form is (validated as on load).
fn edited(mcu: &str, edit: impl Fn(&mut toml::Table)) -> Result<Registry, DefError> {
    let mut form: serde_json::Value = serde_json::from_str(&Registry::builtin().to_json()).unwrap();
    let mut found = false;
    for s in form["mcus"].as_array_mut().unwrap() {
        let mut t: toml::Table = s["toml"].as_str().unwrap().parse().unwrap();
        if t["id"].as_str() == Some(mcu) {
            edit(&mut t);
            s["toml"] = toml::to_string(&t).unwrap().into();
            found = true;
        }
    }
    assert!(found, "no built-in MCU {mcu}");
    Registry::from_json(&form.to_string())
}

/// `[[slot]]` and `[[slot_layout]]` entries of `t` for which `keep` holds
/// (`keep(kind, binding)`).
fn keep_slots(t: &mut toml::Table, keep: impl Fn(&str, &str) -> bool) {
    for (list, kind, slot) in [
        ("slot", "image", "binding"),
        ("slot_layout", "target", "slot"),
    ] {
        if let Some(a) = t.get_mut(list).and_then(|v| v.as_array_mut()) {
            a.retain(|e| keep(e[kind].as_str().unwrap(), e[slot].as_str().unwrap()));
        }
    }
}

/// Built-in MCU `mcu` with slot `b` left out for every kind in `single`.
fn with_single(mcu: &str, single: &[&str]) -> Result<Registry, DefError> {
    edited(mcu, |t| {
        keep_slots(t, |k, b| !(single.contains(&k) && b == "b"))
    })
}

/// The kinds of `m` that have a slot `b`: each may have one slot or two.
fn kinds_with_b(m: &McuDef) -> Vec<ImageKind> {
    m.slots
        .iter()
        .filter(|s| s.binding == Binding::B)
        .map(|s| s.image)
        .collect()
}

/// Every subset of `kinds`.
fn subsets(kinds: &[ImageKind]) -> Vec<Vec<ImageKind>> {
    (0..1usize << kinds.len())
        .map(|bits| {
            (kinds.iter().enumerate())
                .filter(|(i, _)| bits >> i & 1 == 1)
                .map(|(_, k)| *k)
                .collect()
        })
        .collect()
}

fn kinds(m: &McuDef) -> Vec<ImageKind> {
    let mut k: Vec<ImageKind> = m.slots.iter().map(|s| s.image).collect();
    k.dedup();
    k
}

#[test]
fn the_built_in_definitions_state_their_writers() {
    use ImageKind::{Agent, App, Bootcore};
    let want: BTreeMap<&str, Vec<(ImageKind, Vec<ImageKind>)>> = [
        ("rp2040", vec![(App, vec![App]), (Bootcore, vec![App])]),
        ("stm32f401re", vec![(App, vec![App]), (Bootcore, vec![App])]),
        ("esp32s3", vec![(App, vec![Agent]), (Agent, vec![App])]),
    ]
    .into_iter()
    .collect();
    for m in Registry::builtin().mcus() {
        let have: Vec<_> = m.writers.iter().map(|(k, w)| (*k, w.clone())).collect();
        let mut expected = want[m.id.as_str()].clone();
        expected.sort();
        assert_eq!(have, expected, "{}", m.id);
        assert!(
            m.writers.values().all(|w| !w.contains(&Bootcore)),
            "{}: no boot core writes anything",
            m.id
        );
    }
}

/// Each declared writer kind has a slot on the MCU and runs from storage
/// other than the slot it writes: another kind's slots lie apart from the
/// written kind's; a kind writing itself has a slot it is not running from.
#[test]
fn every_declared_writer_exists_and_runs_from_other_storage() {
    for m in Registry::builtin().mcus() {
        for (kind, by) in &m.writers {
            let written: Vec<&Slot> = m.slots.iter().filter(|s| s.image == *kind).collect();
            for w in by {
                let runs: Vec<&Slot> = m.slots.iter().filter(|s| s.image == *w).collect();
                assert!(!runs.is_empty(), "{}: {w:?} has no slot", m.id);
                if w == kind {
                    assert!(written.len() > 1, "{}: {kind:?} writes itself", m.id);
                    continue;
                }
                for (r, s) in runs
                    .iter()
                    .flat_map(|r| written.iter().map(move |s| (r, s)))
                {
                    let apart = r.start + r.size <= s.start || s.start + s.size <= r.start;
                    assert!(apart, "{}: {w:?} runs where it writes {kind:?}", m.id);
                }
            }
        }
    }
}

/// The host test in which each built-in writer performs its write through
/// the agent: (MCU, written kind, writer) -> (file, test).
const WRITES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "rp2040",
        "app",
        "app",
        "crates/dagon-host/tests/sim.rs",
        "full_apply_then_rollback",
    ),
    (
        "rp2040",
        "bootcore",
        "app",
        "crates/dagon-host/tests/sim.rs",
        "a_release_with_hub_boot_cores_writes_the_idle_core_slot_and_the_new_core_takes_over",
    ),
    (
        "stm32f401re",
        "app",
        "app",
        "targets/mcu/stm32f401re/tests/flow.rs",
        "update_confirm_then_back_to_previous_version",
    ),
    (
        "stm32f401re",
        "bootcore",
        "app",
        "crates/dagon-host/tests/sim.rs",
        "the_f401_takes_a_new_boot_core_through_the_release",
    ),
    (
        "esp32s3",
        "app",
        "agent",
        "targets/mcu/esp32s3/src/tests_factory.rs",
        "factory_receives_a_signed_app_and_hands_over_to_it",
    ),
    (
        "esp32s3",
        "agent",
        "app",
        "targets/mcu/esp32s3/src/tests_factory.rs",
        "the_running_app_writes_a_new_factory_agent",
    ),
];

#[test]
fn every_built_in_writer_is_shown_writing_through_the_agent_in_a_host_test() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut declared: Vec<(String, &str, &str)> = Vec::new();
    for m in Registry::builtin().mcus() {
        for (kind, by) in &m.writers {
            for w in by {
                declared.push((m.id.clone(), kind.as_str(), w.as_str()));
            }
        }
    }
    let mut named: Vec<(String, &str, &str)> = WRITES
        .iter()
        .map(|(m, k, w, _, _)| (m.to_string(), *k, *w))
        .collect();
    declared.sort();
    named.sort();
    assert_eq!(declared, named, "each declared writer names its host test");
    for (_, _, _, file, test) in WRITES {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(text.contains(&format!("fn {test}(")), "{file}: no {test}");
    }
}

/// Per built-in MCU, every combination of one or two slots of each kind
/// that has a `b`: a kind with one slot loads only when a kind other than
/// itself writes it; otherwise the load names the kind.
#[test]
fn every_one_or_two_slot_combination_of_each_built_in_mcu_loads_or_names_the_kind() {
    for m in Registry::builtin().mcus() {
        for single in subsets(&kinds_with_b(m)) {
            let names: Vec<&str> = single.iter().map(|k| k.as_str()).collect();
            let unwritable = single.iter().find(|k| m.writers[k].iter().all(|w| w == *k));
            match (with_single(&m.id, &names), unwritable) {
                (Ok(r), None) => {
                    let got = r.mcu(&m.id).unwrap();
                    for k in kinds(m) {
                        let n = got.slots.iter().filter(|s| s.image == k).count();
                        let want = if single.contains(&k) {
                            1
                        } else {
                            kinds_count(m, k)
                        };
                        assert_eq!(n, want, "{} {names:?} {k:?}", m.id);
                    }
                }
                (Err(e), Some(k)) => assert!(
                    e.msg.contains(&format!("{} has one slot", k.as_str()))
                        && e.msg.contains("no other image on this MCU can write it"),
                    "{} {names:?}: {e}",
                    m.id
                ),
                (got, want) => panic!("{} {names:?}: {got:?}, refusing {want:?}", m.id),
            }
        }
    }
}

fn kinds_count(m: &McuDef, k: ImageKind) -> usize {
    m.slots.iter().filter(|s| s.image == k).count()
}

/// The combinations the rule gives on the built-in MCUs, written out.
#[test]
fn the_built_in_combinations_are_these() {
    for (mcu, single, ok) in [
        ("rp2040", &[][..], true),
        ("rp2040", &["bootcore"][..], true),
        ("rp2040", &["app"][..], false),
        ("rp2040", &["app", "bootcore"][..], false),
        ("stm32f401re", &[][..], true),
        ("stm32f401re", &["bootcore"][..], true),
        ("stm32f401re", &["app"][..], false),
        ("stm32f401re", &["app", "bootcore"][..], false),
    ] {
        assert_eq!(with_single(mcu, single).is_ok(), ok, "{mcu} {single:?}");
    }
    // The ESP32-S3's app and agent each have one slot, written by the other.
    let esp = Registry::builtin().mcu("esp32s3").unwrap();
    assert_eq!(kinds_count(esp, ImageKind::App), 1);
    assert_eq!(kinds_count(esp, ImageKind::Agent), 1);
}

/// The STATUS entries a node fills are the slots its definition gives:
/// the app's (an `any` slot both A and B) and the agent's factory slot for
/// the app; the boot core's for the boot core.
#[test]
fn status_rows_are_the_slots_the_definition_gives() {
    use slot_layout::TargetName::{App, Bootcore};
    let reg = Registry::builtin();
    let rows = |m: &str, t| reg.mcu(m).unwrap().status_rows(t);
    assert_eq!(rows("rp2040", App), [0, 1]);
    assert_eq!(rows("rp2040", Bootcore), [0, 1]);
    assert_eq!(rows("esp32s3", App), [0, 1, 2]);
    assert!(rows("esp32s3", Bootcore).is_empty());
    let one = with_single("stm32f401re", &["bootcore"]).unwrap();
    let f401 = one.mcu("stm32f401re").unwrap();
    assert_eq!(f401.status_rows(App), [0, 1]);
    assert_eq!(f401.status_rows(Bootcore), [0]);
}

#[test]
fn a_b_slot_without_an_a_slot_is_refused() {
    for m in Registry::builtin().mcus() {
        for k in kinds_with_b(m) {
            let e = edited(&m.id, |t| {
                keep_slots(t, |kind, b| !(kind == k.as_str() && b == "a"))
            })
            .unwrap_err();
            assert!(
                e.msg.contains(&format!("slot {k:?}/B needs slot {k:?}/A")),
                "{}: {e}",
                m.id
            );
        }
    }
}

/// The fake MCU (boot core a; app a and b) with `writers` replaced.
fn fake_with(writers: &str) -> String {
    let from = "[writers]\napp = [\"app\"]\nbootcore = [\"app\"]\n";
    assert!(FAKE_MCU.contains(from));
    FAKE_MCU.replace(from, writers)
}

#[test]
fn two_slots_written_by_their_own_kind_load() {
    load::parse_mcu_text(
        &fake_with("[writers]\napp = [\"app\"]\nbootcore = [\"app\"]\n"),
        &[],
    )
    .unwrap();
}

#[test]
fn a_single_slot_whose_only_writer_is_itself_is_refused() {
    let e = mcu_err(&fake_with(
        "[writers]\napp = [\"app\"]\nbootcore = [\"bootcore\"]\n",
    ));
    assert!(
        e.ends_with(concat!(
            "bootcore has one slot, and no other image on this MCU can write it: ",
            "an image cannot rewrite the slot it runs from ",
            "(writers.bootcore = [\"bootcore\"]) [image writers (agent and boot stages)]"
        )),
        "{e}"
    );
    let e = mcu_err(&fake_with("[writers]\napp = [\"app\"]\nbootcore = []\n"));
    assert!(
        e.contains("bootcore has one slot, and no other image"),
        "{e}"
    );
}

#[test]
fn a_writer_or_written_kind_without_a_slot_is_refused() {
    let e = mcu_err(&fake_with(
        "[writers]\napp = [\"agent\", \"app\"]\nbootcore = [\"app\"]\n",
    ));
    assert!(
        e.contains("writers.app: agent has no slot on this MCU"),
        "{e}"
    );
    let e = mcu_err(&fake_with(
        "[writers]\napp = [\"app\"]\nbootcore = [\"app\"]\nagent = [\"app\"]\n",
    ));
    assert!(
        e.contains("writers.agent: agent has no slot on this MCU"),
        "{e}"
    );
}

#[test]
fn every_kind_with_a_slot_states_its_writers() {
    let e = mcu_err(&fake_with("[writers]\napp = [\"app\"]\n"));
    assert!(e.contains("writers has no entry for bootcore"), "{e}");
    let e = mcu_err(&fake_with(""));
    assert!(e.contains("missing field `writers`"), "{e}");
}

/// A node that relays to others may use one slot like any other node: the
/// sample's relaying RP2040 and its F401 node with one boot core slot each
/// pass the topology check.
#[test]
fn a_relaying_node_may_have_one_slot() {
    let mut form: serde_json::Value = serde_json::from_str(&Registry::builtin().to_json()).unwrap();
    for s in form["mcus"].as_array_mut().unwrap() {
        let mut t: toml::Table = s["toml"].as_str().unwrap().parse().unwrap();
        keep_slots(&mut t, |k, b| !(k == "bootcore" && b == "b"));
        s["toml"] = toml::to_string(&t).unwrap().into();
    }
    let reg = Registry::from_json(&form.to_string()).unwrap();
    for id in ["rp2040", "stm32f401re"] {
        let n = reg.mcu(id).unwrap().slots.iter();
        assert_eq!(n.filter(|s| s.image == ImageKind::Bootcore).count(), 1);
    }
    let sample = include_str!("../../../../topology/sample.yaml");
    let t = crate::parse_str(sample).unwrap();
    assert!(
        t.links.iter().any(|l| l.end("hub").is_some()),
        "a node relays"
    );
    let r = crate::check_with(&reg, &t);
    assert!(r.errors().next().is_none(), "{:?}", r.issues);
}
