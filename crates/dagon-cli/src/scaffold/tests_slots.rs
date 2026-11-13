//! Per built-in MCU, every combination of one or two slots of each image
//! kind it has: accepted ones load and `dagon gen` gives a node the
//! second-slot build feature only when its app has a `b` slot; refused
//! ones name the kind no other image can write. Building and signing the
//! boot core slots that exist is `gen_boot_core_slots_sign_*_one_core`.

use dagon_topology::defs::{Binding, DefError, ImageKind, Registry};

use super::*;

/// The built-in definitions with slot `b` (and its slot layout) left out of
/// MCU `mcu` for every kind in `single`, loaded as the JSON form is.
pub(super) fn with_single(mcu: &str, single: &[&str]) -> Result<Registry, DefError> {
    let mut form: serde_json::Value = serde_json::from_str(&Registry::builtin().to_json()).unwrap();
    for s in form["mcus"].as_array_mut().unwrap() {
        let mut t: toml::Table = s["toml"].as_str().unwrap().parse().unwrap();
        if t["id"].as_str() != Some(mcu) {
            continue;
        }
        for (list, kind, slot) in [
            ("slot", "image", "binding"),
            ("slot_layout", "target", "slot"),
        ] {
            if let Some(a) = t.get_mut(list).and_then(|v| v.as_array_mut()) {
                a.retain(|e| {
                    !(single.contains(&e[kind].as_str().unwrap()) && e[slot].as_str() == Some("b"))
                });
            }
        }
        s["toml"] = toml::to_string(&t).unwrap().into();
    }
    Registry::from_json(&form.to_string())
}

/// Every subset of the kinds of `mcu` that have a `b` slot.
fn combinations(mcu: &str) -> Vec<Vec<ImageKind>> {
    let m = Registry::builtin().mcu(mcu).unwrap();
    let kinds: Vec<ImageKind> = m
        .slots
        .iter()
        .filter(|s| s.binding == Binding::B)
        .map(|s| s.image)
        .collect();
    (0..1usize << kinds.len())
        .map(|bits| {
            (kinds.iter().enumerate())
                .filter(|(i, _)| bits >> i & 1 == 1)
                .map(|(_, k)| *k)
                .collect()
        })
        .collect()
}

#[test]
fn gen_gives_the_second_slot_feature_only_to_an_app_with_a_b_slot() {
    let t = dagon_topology::parse_str(SAMPLE).unwrap();
    let d = super::tests::tmp("slots");
    let mut seen = 0;
    for m in Registry::builtin().mcus() {
        let node = t.nodes.iter().find(|n| n.mcu == m.id).unwrap();
        for single in combinations(&m.id) {
            let names: Vec<&str> = single.iter().map(|k| k.as_str()).collect();
            let refused = single.iter().find(|k| m.writers[k].iter().all(|w| w == *k));
            let reg = match (with_single(&m.id, &names), refused) {
                (Ok(reg), None) => reg,
                (Err(e), Some(k)) => {
                    let want = format!(
                        "{} has one slot, and no other image on this MCU can write it",
                        k.as_str()
                    );
                    assert!(e.msg.contains(&want), "{} {names:?}: {e}", m.id);
                    continue;
                }
                (got, want) => panic!("{} {names:?}: {got:?}, refusing {want:?}", m.id),
            };
            let config = d.join(format!("{}-{}.toml", m.id, names.join("-")));
            std::fs::write(&config, "").unwrap();
            add_build_tables(&reg, &config, &t).unwrap();
            let doc: toml::Table = std::fs::read_to_string(&config).unwrap().parse().unwrap();
            let feature = doc["build"][&node.id].get("slot_b_feature");
            let app_b = reg
                .mcu(&m.id)
                .unwrap()
                .slots
                .iter()
                .any(|s| s.image == ImageKind::App && s.binding == Binding::B);
            assert_eq!(
                feature.and_then(|f| f.as_str()),
                app_b.then_some("slot-b"),
                "{} {names:?}",
                m.id
            );
            seen += 1;
        }
    }
    assert!(seen >= 5, "{seen} accepted combinations");
    let _ = std::fs::remove_dir_all(&d);
}
