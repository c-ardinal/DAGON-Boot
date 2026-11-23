//! How long a boot core's steps take, measured on the node: a boot core
//! built to measure one step keeps the microseconds it took in a record a
//! debug probe reads by symbol, then stays where it is ([`hold`]) instead
//! of booting, its watchdog fed, so the value is still there when read.
//!
//! - [`DAGON_LISTEN_US`] (boot core feature `listen-time`): from the boot
//!   core's start to the end of its listen window, when no STAY came.
//! - [`DAGON_DECIDE_US`] (boot core feature `decide-time`): the boot
//!   decision, the signature check of the app it starts included.

use core::sync::atomic::{AtomicU32, Ordering};

/// Microseconds from the boot core's start to the end of its listen
/// window; 0 until then.
#[no_mangle]
pub static DAGON_LISTEN_US: AtomicU32 = AtomicU32::new(0);
/// Microseconds the boot decision took; 0 until it is taken.
#[no_mangle]
pub static DAGON_DECIDE_US: AtomicU32 = AtomicU32::new(0);

/// Keeps `us` (at most `u32::MAX`) in `record` and stays, calling `feed`
/// (the watchdog) for ever.
pub fn hold(record: &AtomicU32, us: u64, mut feed: impl FnMut()) -> ! {
    // A store only: the targets include ARMv6-M, and one core runs here.
    record.store(u32::try_from(us).unwrap_or(u32::MAX), Ordering::Relaxed);
    loop {
        feed();
    }
}
