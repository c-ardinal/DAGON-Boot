//! Fixed scenarios: the failure classes the formats must exclude, and the
//! cases the model's shrinker found while the formats were iterated (kept
//! as regressions).

use dagon_retained_model::harness::{check, shrink, World};
use dagon_retained_model::identity::{self, identity, LoadError};
use dagon_retained_model::world_trial::{self as wtr, TrialWorld};

const SEED: u64 = 0x5EED_F1C5;

fn ok<W: World>(cfg: &W::Config, ops: &[W::Op]) {
    if let Err(v) = check::<W>(cfg, ops, SEED) {
        panic!("{v:?}\n{cfg:#?}\n{ops:#?}");
    }
}

// --- MCU identity --------------------------------------------------------

/// P12: refusal at every carrier width with both ids named, unknown values
/// never matched, round trip through the topology node.
#[test]
fn identity_load_and_match() {
    // Few enough that no two meet in the identity reply's 16 bits.
    let ids: Vec<String> = (0..40).map(|i| format!("mcu-{i}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let t = identity::load(&refs, identity).unwrap();
    for id in &refs {
        assert_eq!(t.lookup(identity(id)), Some(*id));
    }
    for v in [
        0u64,
        u64::MAX,
        identity("not-loaded"),
        identity(refs[0]) ^ 1,
    ] {
        assert_eq!(t.lookup(v), None);
    }
    fn low_byte(id: &str) -> u64 {
        identity(id) & 0xFF | 0x100
    }
    match identity::load(&refs, low_byte) {
        Err(LoadError::Collision { a, b, .. }) => assert_ne!(a, b),
        other => panic!("{other:?}"),
    }
    // The identity reply carries 16 bits: two ids that meet there are
    // refused though their full identities differ.
    let twin = ["rp2040", "fake-m51917"];
    assert_ne!(identity(twin[0]), identity(twin[1]));
    match identity::load(&twin, identity) {
        Err(LoadError::Collision { bits: 16, .. }) => {}
        other => panic!("{other:?}"),
    }
}

/// One carrier list: the specification's carrier line names exactly the
/// model's `identity::CARRIERS`.
#[test]
fn carrier_list_matches_the_spec() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/src/content/docs/en/explanation/retained-formats.md"
    ))
    .unwrap();
    let text = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = text
        .split("Carriers: ")
        .nth(1)
        .and_then(|t| t.split(" (the model").next())
        .expect("carrier line");
    let named: Vec<&str> = line.split(", ").map(|c| c.trim_matches('`')).collect();
    let model: Vec<&str> = identity::CARRIERS.iter().map(|c| c.0).collect();
    assert_eq!(named, model);
}

// --- Harness -------------------------------------------------------------

/// The shrinker reduces a failure to the operations it needs.
#[test]
fn shrinker_minimises() {
    struct Toy;
    impl World for Toy {
        type Config = ();
        type Op = u8;
        fn generate(_: &mut dagon_retained_model::rng::Rng) -> ((), Vec<u8>) {
            ((), vec![])
        }
        fn run(_: &(), ops: &[u8], _: u64) -> Result<(), dagon_retained_model::harness::Violation> {
            let bad = ops.contains(&3) && ops.contains(&7);
            match bad {
                true => Err(dagon_retained_model::harness::Violation::new("P0", 0, "")),
                false => Ok(()),
            }
        }
        fn simpler(_: &u8) -> Vec<u8> {
            vec![]
        }
    }
    let ops = shrink::<Toy>(&(), vec![1, 3, 5, 9, 7, 2], 0, "P0");
    assert_eq!(ops, vec![3, 7]);
}

// --- Boot core trial cell ------------------------------------------------

/// Found by the trial world: a reset between the two stores of a trial
/// cell update left a pair that counted 0, so the start went uncounted and
/// a broken core could be tried for ever (C1). The cell now repeats one
/// self-checking word and counts the larger of the two.
#[test]
fn regression_trial_cell_update_cut_between_its_words() {
    let c = wtr::Config { max: 5 };
    let ops = [
        wtr::Op::SetPending { good: false },
        wtr::Op::Boot {
            fault: Some(wtr::Fault::Reset { at: 0 }),
        },
    ];
    ok::<TrialWorld>(&c, &ops);
}

/// Found by the trial world once it ran the cell check: the check wrote a
/// probe over both words of a running count, and a reset before the count
/// was back left a cell that counted 0 (C1). The check now writes nothing
/// while a word holds a count.
#[test]
fn regression_trial_cell_check_cut_after_its_probe() {
    let c = wtr::Config { max: 5 };
    let ops = [
        wtr::Op::SetPending { good: false },
        wtr::Op::Boot {
            fault: Some(wtr::Fault::Reset { at: 3 }),
        },
    ];
    ok::<TrialWorld>(&c, &ops);
}
