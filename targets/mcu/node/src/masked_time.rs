//! How long DAGON's own code masks interrupts, measured on the node: an
//! app built with feature `masked-time` times each critical section of the
//! guard's state ([`crate::guard`]) and keeps the longest in [`DAGON_MASKED_MAX_US`],
//! with the number timed in [`DAGON_MASKED_COUNT`], for a debug probe to read by
//! symbol while the app runs. The timing covers the work inside each
//! section, not masking, the guard's borrow and unmasking around it, and
//! the clock counts whole microseconds: the definition's `[execution]
//! masked_max_us` is the worst value plus a margin (samples/hardware-test
//! measure.py gives it).
//!
//! The push of a record into the ring of recent events
//! (`dagon_events::Recent`, the other reviewed function that masks: at
//! most three copies of one record) is timed the same way, by a ring the
//! MCU package makes with [`ring_timing`]: an MCU package without the
//! guard (the ESP32-S3) times that section alone.
//!
//! Not timed: the embassy-sync `Channel` and
//! `Watch` operations of the published events and the relay port, which
//! mask to copy one bounded value each and run no closure (`clippy.toml`
//! in `targets/mcu`, `crates/dagon-relay` and `crates/dagon-events`
//! refuses the ones that would; `dagon-agent` bounds the size of the
//! value copied).
//!
//! A board with no debug probe reads it off its `[boot] led` light
//! instead: [`READOUT_AFTER_S`] after the start, the MCU package blinks
//! the longest as decimal digit groups ([`digits`]).
//!
//! The MCU package gives the guard its clock
//! ([`Guard::time_masking`](crate::guard::Guard::time_masking)) when the
//! node starts; sections before that are not timed.

use core::sync::atomic::{AtomicU32, Ordering};

/// The longest critical section timed, microseconds; 0 until one is.
#[no_mangle]
pub static DAGON_MASKED_MAX_US: AtomicU32 = AtomicU32::new(0);
/// How many critical sections were timed.
#[no_mangle]
pub static DAGON_MASKED_COUNT: AtomicU32 = AtomicU32::new(0);

/// Seconds after the node starts at which a board with no debug probe
/// starts blinking the longest section on its `[boot] led` light: the time
/// the user has to drive the node first.
pub const READOUT_AFTER_S: u64 = 180;

/// `n` as blink groups: its decimal digits, most significant first, 10
/// for a 0 (a group of no blinks would not be seen); the groups are the
/// first `len` of the array.
pub fn digits(mut n: u32) -> ([u8; 10], usize) {
    let mut out = [0u8; 10];
    let mut len = 0;
    loop {
        let d = (n % 10) as u8;
        out[len] = if d == 0 { 10 } else { d };
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out[..len].reverse();
    (out, len)
}

/// How the ring of recent events times its critical section into the
/// same records as the guard's: `now_us` gives microseconds since boot.
#[cfg(feature = "masked-time")]
pub fn ring_timing(now_us: fn() -> u64) -> dagon_events::Timing {
    dagon_events::Timing { now_us, note }
}

/// Records one section of `us` microseconds. Loads and stores only: the
/// targets include ARMv6-M, and this runs with interrupts masked.
pub(crate) fn note(us: u64) {
    let us = u32::try_from(us).unwrap_or(u32::MAX);
    if us > DAGON_MASKED_MAX_US.load(Ordering::Relaxed) {
        DAGON_MASKED_MAX_US.store(us, Ordering::Relaxed);
    }
    let n = DAGON_MASKED_COUNT.load(Ordering::Relaxed);
    DAGON_MASKED_COUNT.store(n.saturating_add(1), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::AtomicU64;

    use super::*;
    use crate::guard::{levels, Dog, Guard, Trial};

    static NOW: AtomicU64 = AtomicU64::new(0);

    fn now() -> u64 {
        NOW.load(Ordering::Relaxed)
    }

    struct NoDog;

    impl Dog for NoDog {
        fn feed(&mut self) {}
        fn reset(&mut self) {}
    }

    // One test: the records are global.
    #[test]
    fn a_guard_with_a_clock_keeps_its_longest_section_and_counts_each() {
        let g: Guard<NoDog, 1> = Guard::new(levels(0), Trial::NeverFeed, now);
        g.install(NoDog, false);
        // Not timed before the clock is given.
        g.with_dog(|_| NOW.fetch_add(500, Ordering::Relaxed));
        assert_eq!(DAGON_MASKED_COUNT.load(Ordering::Relaxed), 0);
        g.time_masking(now);
        for step in [7, 31, 12] {
            g.with_dog(|_| NOW.fetch_add(step, Ordering::Relaxed));
        }
        assert_eq!(DAGON_MASKED_COUNT.load(Ordering::Relaxed), 3);
        assert_eq!(DAGON_MASKED_MAX_US.load(Ordering::Relaxed), 31);
        let groups = |n| {
            let (d, len) = digits(n);
            d[..len].to_vec()
        };
        assert_eq!(groups(38), [3, 8]);
        assert_eq!(groups(0), [10]);
        assert_eq!(groups(1_205), [1, 2, 10, 5]);
        assert_eq!(groups(u32::MAX), [4, 2, 9, 4, 9, 6, 7, 2, 9, 5]);
        note(u64::MAX);
        assert_eq!(DAGON_MASKED_MAX_US.load(Ordering::Relaxed), u32::MAX);
    }
}
