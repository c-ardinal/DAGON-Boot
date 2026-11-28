//! How long a boot core's steps take, measured on the node: a boot core
//! built to measure one step keeps the microseconds it took in a record a
//! debug probe reads by symbol, then stays where it is ([`hold`]) instead
//! of booting, its watchdog fed, so the value is still there when read
//! and the probe can still reach the core (a pause between feeds: a loop
//! that only stores to the watchdog keeps the core on its system bus, and
//! an STM32F401's SWD access then waits for ever, `SwdDpWait`).
//!
//! - [`DAGON_LISTEN_US`] (boot core feature `listen-time`): from the boot
//!   core's start to the end of its listen window, when no STAY came.
//! - [`DAGON_DECIDE_US`] (boot core feature `decide-time`): the boot
//!   decision, the signature check of the app it starts included.
//!
//! A node with no debug probe reads the value off its boot LED instead:
//! [`ms_digits`] gives the milliseconds as decimal digits, one blink group
//! each, ten blinks for a 0.

use core::sync::atomic::{AtomicU32, Ordering};

/// Microseconds from the boot core's start to the end of its listen
/// window; 0 until then.
#[no_mangle]
pub static DAGON_LISTEN_US: AtomicU32 = AtomicU32::new(0);
/// Microseconds the boot decision took; 0 until it is taken.
#[no_mangle]
pub static DAGON_DECIDE_US: AtomicU32 = AtomicU32::new(0);

/// Keeps `us` (at most `u32::MAX`) in `record`.
pub fn keep(record: &AtomicU32, us: u64) {
    // A store only: the targets include ARMv6-M, and one core runs here.
    record.store(u32::try_from(us).unwrap_or(u32::MAX), Ordering::Relaxed);
}

/// Keeps `us` in `record` and stays, calling `feed` (the watchdog) then
/// `pause` (a wait well within the watchdog's period that leaves the bus
/// to the debug probe, such as a cycle-counted delay) for ever.
pub fn hold(record: &AtomicU32, us: u64, mut feed: impl FnMut(), mut pause: impl FnMut()) -> ! {
    keep(record, us);
    loop {
        feed();
        pause();
    }
}

/// `us` in milliseconds, rounded up, as blink groups: its decimal digits,
/// most significant first, 10 for a 0 (a group of no blinks would not be
/// seen); the groups are the first `len` of the array.
pub fn ms_digits(us: u64) -> ([u8; 10], usize) {
    let mut ms = u32::try_from(us.div_ceil(1000)).unwrap_or(u32::MAX);
    let mut out = [0u8; 10];
    let mut len = 0;
    loop {
        // 32-bit division only (no 64-bit divider on ARMv6-M).
        let d = (ms % 10) as u8;
        out[len] = if d == 0 { 10 } else { d };
        len += 1;
        ms /= 10;
        if ms == 0 {
            break;
        }
    }
    out[..len].reverse();
    (out, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is(us: u64, want: &[u8]) {
        let (d, n) = ms_digits(us);
        assert_eq!(&d[..n], want, "{us}");
    }

    #[test]
    fn milliseconds_rounded_up_as_digit_groups() {
        is(52_000, &[5, 2]);
        is(52_001, &[5, 3]);
        is(400, &[1]);
        is(0, &[10]);
        is(1_205_000, &[1, 2, 10, 5]);
        is(u64::MAX, &[4, 2, 9, 4, 9, 6, 7, 2, 9, 5]);
    }

    #[test]
    fn keep_stores_and_saturates() {
        let r = AtomicU32::new(0);
        keep(&r, 61_000);
        assert_eq!(r.load(Ordering::Relaxed), 61_000);
        keep(&r, u64::MAX);
        assert_eq!(r.load(Ordering::Relaxed), u32::MAX);
    }
}
