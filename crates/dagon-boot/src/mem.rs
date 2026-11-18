//! Memory copy and fill for a boot core image (feature `small-mem`): one
//! byte at a time, in place of the compiler's word-at-a-time routines,
//! which take about 2 KB of a boot core's slot. A boot core copies and
//! fills headers, frames and buffers of at most a few hundred bytes, where
//! the word-wide routines' speed is not needed. The results are the same
//! bytes; only the time differs.
//!
//! On the host (its tests) they are plain functions and export nothing.
//!
//! These are the C names and the ARM EABI helpers (`__aeabi_mem*`, RTABI
//! section 4.3.4) the compiler calls; the linker takes them in place of
//! `compiler_builtins`' weak ones. Each byte moves by a volatile access, so
//! the optimizer cannot turn a loop back into a call to these routines.
//!
//! Every entry point runs one of the private loops below and none calls
//! another entry point by name: the compiler knows `memcpy`, `memmove` and
//! `memset` as library calls and, on ARM, lowers a call to one of them to
//! its `__aeabi_mem*` helper, so a helper that called `memcpy` would call
//! itself (scripts/codec_budget.py fails an image with such a function).
#![allow(unsafe_code)]
// The C and EABI signatures, kept as the standards spell them.
#![allow(clippy::missing_safety_doc)]

use core::ffi::c_int;

/// Copies `n` bytes from `src` to `dest`, lowest first.
///
/// # Safety
/// `n` readable bytes at `src`, `n` writable bytes at `dest`; where they
/// overlap, `dest` is not above `src`.
#[inline(never)]
unsafe fn forward(dest: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        // SAFETY: the caller's contract; copying forward reads each source
        // byte before an overlapping destination byte above it is written.
        unsafe { dest.add(i).write_volatile(src.add(i).read_volatile()) };
        i += 1;
    }
}

/// Copies `n` bytes from `src` to `dest` the way C memmove does: forward,
/// or highest first when `dest` lies above `src`.
///
/// # Safety
/// `n` readable bytes at `src` and `n` writable bytes at `dest`.
#[inline(never)]
unsafe fn moved(dest: *mut u8, src: *const u8, n: usize) {
    if (dest as usize) <= (src as usize) {
        // SAFETY: the caller's contract, with `dest` not above `src`.
        return unsafe { forward(dest, src, n) };
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        // SAFETY: the caller's contract; copying backward reads each source
        // byte before an overlapping destination byte below it is written.
        unsafe { dest.add(i).write_volatile(src.add(i).read_volatile()) };
    }
}

/// Writes `c` to `n` bytes at `dest`.
///
/// # Safety
/// `n` writable bytes at `dest`.
#[inline(never)]
unsafe fn fill(dest: *mut u8, c: u8, n: usize) {
    let mut i = 0;
    while i < n {
        // SAFETY: the caller's contract.
        unsafe { dest.add(i).write_volatile(c) };
        i += 1;
    }
}

#[cfg_attr(target_os = "none", no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // SAFETY: C memcpy's contract: `n` bytes each, not overlapping.
    unsafe { forward(dest, src, n) };
    dest
}

#[cfg_attr(target_os = "none", no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // SAFETY: C memmove's contract: `n` bytes each.
    unsafe { moved(dest, src, n) };
    dest
}

/// The fill is `c` converted to a byte, as C specifies.
#[cfg_attr(target_os = "none", no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut u8, c: c_int, n: usize) -> *mut u8 {
    // SAFETY: C memset's contract: `n` writable bytes.
    unsafe { fill(dest, c as u8, n) };
    dest
}

macro_rules! aeabi {
    ($($copy:ident, $mv:ident, $set:ident, $clr:ident);+ $(;)?) => {$(
        #[cfg_attr(target_os = "none", no_mangle)]
        pub unsafe extern "C" fn $copy(dest: *mut u8, src: *const u8, n: usize) {
            // SAFETY: the EABI helper's contract is memcpy's.
            unsafe { forward(dest, src, n) };
        }

        #[cfg_attr(target_os = "none", no_mangle)]
        pub unsafe extern "C" fn $mv(dest: *mut u8, src: *const u8, n: usize) {
            // SAFETY: the EABI helper's contract is memmove's.
            unsafe { moved(dest, src, n) };
        }

        /// The EABI fill takes the length before the value.
        #[cfg_attr(target_os = "none", no_mangle)]
        pub unsafe extern "C" fn $set(dest: *mut u8, n: usize, c: c_int) {
            // SAFETY: the EABI helper's contract is memset's.
            unsafe { fill(dest, c as u8, n) };
        }

        #[cfg_attr(target_os = "none", no_mangle)]
        pub unsafe extern "C" fn $clr(dest: *mut u8, n: usize) {
            // SAFETY: the EABI helper's contract is memset's with 0.
            unsafe { fill(dest, 0, n) };
        }
    )+};
}

// The alignment variants (4, 8) may be served by the unaligned routine.
aeabi! {
    __aeabi_memcpy, __aeabi_memmove, __aeabi_memset, __aeabi_memclr;
    __aeabi_memcpy4, __aeabi_memmove4, __aeabi_memset4, __aeabi_memclr4;
    __aeabi_memcpy8, __aeabi_memmove8, __aeabi_memset8, __aeabi_memclr8;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every routine leaves the bytes the definition gives, overlapping
    /// moves in both directions included.
    #[test]
    fn copy_move_and_fill_give_the_defined_bytes() {
        let src: [u8; 40] = core::array::from_fn(|i| (i as u8).wrapping_mul(7) | 1);
        for n in 0..=src.len() {
            let mut d = [0xAAu8; 40];
            // SAFETY: both buffers hold `n` bytes and do not overlap.
            unsafe { __aeabi_memcpy4(d.as_mut_ptr(), src.as_ptr(), n) };
            assert_eq!(d[..n], src[..n]);
            assert!(d[n..].iter().all(|&b| b == 0xAA));
            // SAFETY: `n` writable bytes.
            unsafe { __aeabi_memset(d.as_mut_ptr(), n, 0x1_5C) };
            assert!(d[..n].iter().all(|&b| b == 0x5C), "the fill is c as a byte");
            // SAFETY: `n` writable bytes.
            unsafe { __aeabi_memclr8(d.as_mut_ptr(), n) };
            assert!(d[..n].iter().all(|&b| b == 0));
        }
        for shift in 0..8usize {
            for n in 0..32usize {
                let (mut up, mut down) = (src, src);
                let (mut want_up, mut want_down) = (src, src);
                want_up.copy_within(0..n, shift);
                want_down.copy_within(shift..shift + n, 0);
                let p = up.as_mut_ptr();
                // SAFETY: `n` bytes from `p` and from `p + shift` lie in `up`.
                unsafe { memmove(p.add(shift), p, n) };
                let q = down.as_mut_ptr();
                // SAFETY: as above, in `down`.
                unsafe { __aeabi_memmove(q, q.add(shift), n) };
                assert_eq!((up, down), (want_up, want_down), "shift {shift}, {n} bytes");
            }
        }
    }

    /// The C entry points give the same bytes at any alignment and return
    /// `dest`; a zero length touches nothing.
    #[test]
    fn the_c_routines_work_at_any_alignment() {
        let src: [u8; 24] = core::array::from_fn(|i| i as u8 + 1);
        for (at, from) in [(0, 0), (1, 0), (3, 1), (2, 3)] {
            for n in [0, 1, 7, 16] {
                let mut d = [0xEEu8; 24];
                let p = d.as_mut_ptr();
                // SAFETY: `n` bytes from `at` and from `from` lie in each array.
                let back = unsafe { memcpy(p.add(at), src.as_ptr().add(from), n) };
                assert_eq!(back, p.wrapping_add(at));
                assert_eq!(d[at..at + n], src[from..from + n]);
                assert!(d[..at].iter().chain(&d[at + n..]).all(|&b| b == 0xEE));
                // SAFETY: as above.
                let back = unsafe { memset(p.add(at), 0x2A, n) };
                assert_eq!(back, p.wrapping_add(at));
                assert!(d[at..at + n].iter().all(|&b| b == 0x2A));
                // SAFETY: `n` bytes from `at` and from `from` lie in `d`.
                unsafe { memmove(p.add(from), p.add(at), n) };
                assert!(d[from..from + n].iter().all(|&b| b == 0x2A));
            }
        }
    }
}
