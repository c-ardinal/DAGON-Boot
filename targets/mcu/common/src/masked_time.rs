//! How long DAGON's own code masks interrupts, measured on the node: an
//! app built with feature `masked-time` times each critical section of the
//! guard's state ([`crate::guard`], the only place DAGON's own code masks
//! interrupts) and keeps the longest in [`DAGON_MASKED_MAX_US`], with the
//! number timed in [`DAGON_MASKED_COUNT`], for a debug probe to read by
//! symbol while the app runs. The value is what `[execution]
//! masked_max_us` of the MCU definition states.
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
        note(u64::MAX);
        assert_eq!(DAGON_MASKED_MAX_US.load(Ordering::Relaxed), u32::MAX);
    }
}
