//! A kind with one slot: the app boot decision over a one-slot layout and
//! stage0's choice among one boot core slot. With one slot there is no
//! previous image to go back to; trial and confirmation stay.

mod bench;

use bench::*;
use dagon_core::bootcore::{choose, trial_cell, CLEARED};
use dagon_core::log::LogState;

/// The F401 bench layout with slot A only.
const ONE: SlotLayout = SlotLayout {
    slots: &[(0x0802_0000, 0x2_0000)],
};

fn boot_one(m: &mut Mem) -> Decision {
    let mut log = Log::load(m, &LOG).unwrap();
    decide(m, &mut log, &ONE, &cfg(), &verifier())
}

/// Slot A confirmed with `image(A, 0)`; its reference.
fn confirmed_a(m: &mut Mem) -> SlotRef {
    let a = install(m, A, &image(A, 0));
    write_log(m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    a
}

#[test]
fn a_one_slot_layout_starts_its_confirmed_image() {
    let mut m = Mem::new();
    confirmed_a(&mut m);
    assert_eq!(
        boot_one(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
}

#[test]
fn a_new_image_in_the_single_slot_is_tried_then_runs_confirmed() {
    let mut m = Mem::new();
    let a = confirmed_a(&mut m);
    let new = install(&mut m, A, &image(A, 1));
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, new),
        ],
    );
    assert_eq!(
        boot_one(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: true
        }
    );
    assert_eq!(state(&mut m).attempts, 1);
}

/// The pending image overwrote the only slot and does not check out: it
/// is rolled back, and the confirmed image it replaced is gone, so nothing
/// starts (recovery writes the slot again).
#[test]
fn a_bad_image_in_the_single_slot_starts_nothing() {
    let mut m = Mem::new();
    let a = confirmed_a(&mut m);
    let new = install(&mut m, A, &image(A, 1));
    m.bytes[(SLOTS.slots[0].0 - BASE) as usize + HEADER_SIZE + 7] ^= 0xFF;
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, new),
        ],
    );
    assert_eq!(boot_one(&mut m), Decision::NoImage);
    let s = state(&mut m);
    assert_eq!(s.pending, None);
    assert_eq!(s.last_reason, reason::HASH_MISMATCH);
}

#[test]
fn a_record_naming_a_slot_the_layout_lacks_starts_nothing() {
    let mut m = Mem::new();
    let b = install(&mut m, B, &image(B, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, b), (kind::CONFIRMED, b)]);
    assert_eq!(boot_one(&mut m), Decision::NoImage);
}

/// Stage0 checks only the boot core slots it is given: with one, a slot B
/// is never looked at, whatever the log names.
#[test]
fn stage0_checks_only_the_configured_core_slots() {
    let in_b = LogState {
        confirmed: Some(SlotRef {
            slot: 1,
            version: Version::new(1, 0, 0),
            digest16: [0; 16],
        }),
        ..LogState::default()
    };
    for (slots, want, named) in [(1u8, vec![], false), (2, vec![1u8], true)] {
        let mut seen = Vec::new();
        let e = choose(&in_b, CLEARED, 3, slots, |s, _| {
            seen.push(s);
            Err::<(), u8>(s)
        })
        .unwrap_err();
        assert_eq!(e.named, named);
        assert_eq!(seen, want, "{slots} slots");
    }
    // A pending core in the only slot is tried there.
    let p = SlotRef {
        slot: 0,
        version: Version::new(1, 1, 0),
        digest16: [1; 16],
    };
    let st = LogState {
        pending: Some(p),
        ..LogState::default()
    };
    let start = choose(&st, CLEARED, 3, 1, |s, _| Ok::<u8, u8>(s)).unwrap();
    assert_eq!(
        (start.slot, start.trial, start.cell),
        (0, true, trial_cell(&p, 1))
    );
}

/// A log record naming a boot core slot the MCU does not give is never
/// checked or started: a pending core in slot B is not tried, a confirmed
/// core in slot B is not run; the confirmed core A starts, and with no
/// record for slot A nothing starts.
#[test]
fn stage0_ignores_records_naming_a_slot_it_does_not_have() {
    let r = |slot, minor| SlotRef {
        slot,
        version: Version::new(1, minor, 0),
        digest16: [minor; 16],
    };
    let cases = [
        LogState {
            confirmed: Some(r(0, 0)),
            pending: Some(r(1, 1)),
            ..LogState::default()
        },
        LogState {
            confirmed: Some(r(1, 0)),
            ..LogState::default()
        },
        LogState {
            confirmed: Some(r(1, 0)),
            pending: Some(r(1, 1)),
            ..LogState::default()
        },
    ];
    let mut seen = Vec::new();
    let start = choose(&cases[0], CLEARED, 3, 1, |s, rec| {
        seen.push((s, rec.copied()));
        Ok::<u8, u8>(s)
    })
    .unwrap();
    assert_eq!((start.slot, start.trial, start.cell), (0, false, CLEARED));
    assert_eq!(seen, [(0, Some(r(0, 0)))]);
    for st in &cases[1..] {
        let mut seen = Vec::new();
        let e = choose(st, CLEARED, 3, 1, |s, _| {
            seen.push(s);
            Ok::<u8, u8>(s)
        })
        .unwrap_err();
        assert!(seen.is_empty(), "{st:?}: {seen:?}");
        assert!(!e.named, "a record outside the slots names no core");
    }
}
