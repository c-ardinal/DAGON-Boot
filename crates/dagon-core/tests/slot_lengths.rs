//! Each app slot has its own length: an image is checked against the slot
//! it is in, not against a length shared by all slots.

mod bench;

use bench::*;

/// Slot A as on the bench; slot B just long enough for the bench image's
/// 2048-byte body after its header.
const ROOMY_B: SlotLayout = SlotLayout {
    slots: &[(0x0802_0000, 0x2_0000), (0x0804_0000, 0xA00)],
};

/// Slot B four bytes too short for that body; slot A unchanged.
const TIGHT_B: SlotLayout = SlotLayout {
    slots: &[(0x0802_0000, 0x2_0000), (0x0804_0000, 0x9FC)],
};

fn boot_with(m: &mut Mem, slots: &SlotLayout) -> Decision {
    let mut log = Log::load(m, &LOG).unwrap();
    decide(m, &mut log, slots, &cfg(), &verifier())
}

#[test]
fn an_image_that_fits_its_shorter_slot_is_tried() {
    let mut m = with_pending_b();
    assert_eq!(
        boot_with(&mut m, &ROOMY_B),
        Decision::Boot {
            slot: B,
            entry: entry(B),
            trial: true
        }
    );
}

/// The same image in a slot shorter than its body is refused and the
/// confirmed image in the longer slot A starts.
#[test]
fn an_image_longer_than_its_slot_is_refused_while_the_longer_slot_still_starts() {
    let mut m = with_pending_b();
    assert_eq!(
        boot_with(&mut m, &TIGHT_B),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    assert_eq!(state(&mut m).pending, None);
}
