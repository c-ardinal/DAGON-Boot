//! How long a stage0 takes to check a boot core's signature, measured on
//! the node: [`Timed`] runs a verifier and keeps the longest verify in CPU
//! cycles and the number of verifies in [`DAGON_VERIFY_CYCLES`] and
//! [`DAGON_VERIFY_COUNT`], where a debug probe reads them by symbol.
//! [`tenths_of_a_second`] turns cycles into the code a stage0 built to
//! measure shows on its diagnostic output.
//!
//! The cycle counter is the caller's; on a Cortex-M target with the
//! `verify-time` feature, [`systick`] counts every core clock cycle.

use core::sync::atomic::{AtomicU32, Ordering};

use dagon_format::SigVerifier;

/// The longest verify since reset, in core clock cycles.
#[no_mangle]
pub static DAGON_VERIFY_CYCLES: AtomicU32 = AtomicU32::new(0);
/// The verifies since reset.
#[no_mangle]
pub static DAGON_VERIFY_COUNT: AtomicU32 = AtomicU32::new(0);

/// `verifier`, timed with `cycles` (a free-running count of core clock
/// cycles, read before and after each verify).
pub struct Timed<'a, V: ?Sized> {
    pub verifier: &'a V,
    pub cycles: fn() -> u32,
}

impl<V: SigVerifier + ?Sized> SigVerifier for Timed<'_, V> {
    fn key_id(&self) -> [u8; 8] {
        self.verifier.key_id()
    }

    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        let start = (self.cycles)();
        let ok = self.verifier.verify(domain, msg, sig);
        let spent = (self.cycles)().wrapping_sub(start);
        // Load and store only: the targets include ARMv6-M (no atomic
        // read-modify-write), and stage0 runs on one core.
        let longest = DAGON_VERIFY_CYCLES.load(Ordering::Relaxed);
        DAGON_VERIFY_CYCLES.store(longest.max(spent), Ordering::Relaxed);
        let count = DAGON_VERIFY_COUNT.load(Ordering::Relaxed);
        DAGON_VERIFY_COUNT.store(count.saturating_add(1), Ordering::Relaxed);
        ok
    }
}

/// `cycles` at `hz` in tenths of a second, rounded up, at most 255: the
/// code a stage0 built to measure shows (a 0.39 s verify blinks 4).
pub fn tenths_of_a_second(cycles: u32, hz: u32) -> u8 {
    // 32-bit division only: a 64-bit one costs an ARMv6-M stage0 about a
    // kilobyte.
    let tenths = cycles.div_ceil((hz / 10).max(1));
    u8::try_from(tenths).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU32;

    static NOW: AtomicU32 = AtomicU32::new(0);

    /// Accepts an empty message; each verify takes 100 cycles more than
    /// the one before (100, 200, ...).
    struct Slow;

    impl SigVerifier for Slow {
        fn key_id(&self) -> [u8; 8] {
            [7; 8]
        }
        fn verify(&self, _: &[u8], msg: &[&[u8]], _: &[u8; 64]) -> bool {
            let n = DAGON_VERIFY_COUNT.load(Ordering::Relaxed) + 1;
            NOW.store(NOW.load(Ordering::Relaxed) + 100 * n, Ordering::Relaxed);
            msg.is_empty()
        }
    }

    fn now() -> u32 {
        NOW.load(Ordering::Relaxed)
    }

    /// The timed verifier answers as its verifier does and keeps the
    /// longest verify and the count.
    #[test]
    fn timed_keeps_the_longest_verify_and_the_count() {
        let t = Timed {
            verifier: &Slow,
            cycles: now,
        };
        assert_eq!(t.key_id(), [7; 8]);
        assert!(t.verify(b"d", &[], &[0; 64]));
        assert!(!t.verify(b"d", &[b"x"], &[0; 64]));
        assert!(t.verify(b"d", &[], &[0; 64]));
        assert_eq!(DAGON_VERIFY_CYCLES.load(Ordering::Relaxed), 300);
        assert_eq!(DAGON_VERIFY_COUNT.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn cycles_become_tenths_of_a_second_rounded_up() {
        assert_eq!(tenths_of_a_second(48_750_000, 125_000_000), 4);
        assert_eq!(tenths_of_a_second(12_500_000, 125_000_000), 1);
        assert_eq!(tenths_of_a_second(12_500_001, 125_000_000), 2);
        assert_eq!(tenths_of_a_second(49_000_000, 6_500_000), 76);
        assert_eq!(tenths_of_a_second(u32::MAX, 1_000_000), 255);
    }
}

/// The SysTick timer as a free-running core clock cycle counter: its 24-bit
/// down-count, extended by its wraps (counted by its exception). ARMv6-M
/// and ARMv7-M architecture reference manuals, SysTick (SYST_CSR, SYST_RVR,
/// SYST_CVR at 0xE000E010).
#[cfg(all(feature = "verify-time", target_arch = "arm", target_os = "none"))]
pub mod systick {
    use core::sync::atomic::{AtomicU32, Ordering};

    const SYST_CSR: *mut u32 = 0xE000_E010 as *mut u32;
    const SYST_RVR: *mut u32 = 0xE000_E014 as *mut u32;
    const SYST_CVR: *mut u32 = 0xE000_E018 as *mut u32;
    /// ENABLE, TICKINT, CLKSOURCE = the processor clock.
    const CSR_RUN: u32 = 0b111;
    const TOP: u32 = 0x00FF_FFFF;

    static WRAPS: AtomicU32 = AtomicU32::new(0);

    /// The SysTick exception (cortex-m-rt takes the handler by this name).
    #[no_mangle]
    extern "C" fn SysTick() {
        WRAPS.store(WRAPS.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
    }

    /// Starts counting from 0.
    pub fn start() {
        // SAFETY: the architecture's SysTick registers; nothing else in a
        // stage0 uses SysTick.
        unsafe {
            SYST_RVR.write_volatile(TOP);
            SYST_CVR.write_volatile(0);
            SYST_CSR.write_volatile(CSR_RUN);
        }
    }

    /// Core clock cycles since [`start`] (wraps after 2^32).
    pub fn cycles() -> u32 {
        loop {
            let wraps = WRAPS.load(Ordering::Relaxed);
            // SAFETY: as in `start`; reading CVR has no side effect.
            let down = unsafe { SYST_CVR.read_volatile() } & TOP;
            if WRAPS.load(Ordering::Relaxed) == wraps {
                return (wraps << 24) | (TOP - down);
            }
        }
    }
}
