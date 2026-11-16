//! A check with `Ed25519Small` calls its tick through every step, so the
//! caller can keep a watchdog fed through a check that takes seconds on a
//! slow clock; the tick never changes what the check accepts.

use std::cell::Cell;

use dagon_ed25519::Ed25519Small;
use dagon_format::header::SIGN_DOMAIN;
use dagon_format::SigVerifier;

#[allow(dead_code)]
mod vector {
    include!("../cycles/src/vector.rs");
}

/// The fewest ticks a check gives: the two exponentiations (about 250
/// squarings each) and the 253 doublings of the double scalar
/// multiplication each tick once per step.
const MIN_TICKS: u32 = 700;

#[test]
fn a_check_ticks_through_every_step_and_accepts_the_same() {
    let mut flipped = vector::SIG;
    flipped[5] ^= 4;
    for (sig, accepted) in [(vector::SIG, true), (flipped, false)] {
        let ticks = Cell::new(0u32);
        let ticking = Ed25519Small::with_tick(vector::KEY, || ticks.set(ticks.get() + 1));
        assert_eq!(
            ticking.verify(SIGN_DOMAIN, &[&vector::BODY], &sig),
            accepted
        );
        let plain = Ed25519Small::new(vector::KEY);
        assert_eq!(plain.verify(SIGN_DOMAIN, &[&vector::BODY], &sig), accepted);
        assert!(ticks.get() >= MIN_TICKS, "{} ticks", ticks.get());
    }
}
