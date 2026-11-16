//! One `Ed25519Small` verify on a Cortex-M0+, for the cycle estimate of
//! dagon-ed25519's tests/m0_cycles.rs: `run(0)` verifies the vector
//! (`src/vector.rs`), `run(1)` the same with one signature bit flipped;
//! each returns 1 when the verifier accepts. With bit 1 of the argument
//! set the verifier ticks, each tick a store to [`TICK`], as a boot stage
//! feeds its watchdog.
#![no_std]
#![no_main]

use dagon_ed25519::Ed25519Small;
use dagon_format::header::SIGN_DOMAIN;
use dagon_format::SigVerifier;

include!("vector.rs");

#[no_mangle]
pub extern "C" fn run(flip: u32) -> u32 {
    // Read through volatile loads so the verify runs on data the compiler
    // cannot fold.
    // SAFETY: plain reads of statics.
    let key = unsafe { core::ptr::read_volatile(&KEY) };
    let mut sig = unsafe { core::ptr::read_volatile(&SIG) };
    let body = unsafe { core::ptr::read_volatile(&BODY_STATIC) };
    sig[5] ^= (flip & 1) as u8 * 4;
    if flip & 2 == 0 {
        return Ed25519Small::new(key).verify(SIGN_DOMAIN, &[&body], &sig) as u32;
    }
    // SAFETY: a store to the address the emulator records, as to a
    // watchdog's reload register.
    let tick = || unsafe { core::ptr::write_volatile(TICK as *mut u32, 1) };
    Ed25519Small::with_tick(key, tick).verify(SIGN_DOMAIN, &[&body], &sig) as u32
}

/// Where a tick stores (a peripheral address; the emulator records the
/// cycle count of each store there).
const TICK: usize = 0x4000_0000;

static BODY_STATIC: [u8; 448] = BODY;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
