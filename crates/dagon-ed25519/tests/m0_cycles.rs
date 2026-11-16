//! The cycles one `Ed25519Small` verify takes on a Cortex-M0+, estimated by
//! running the verify built as the stage0s are (cycles/, thumbv6m, the
//! stage0 profile and SHA-2 rounds) on an ARMv6-M interpreter (armv6m/).
//! A stage0's verify time is about these cycles divided by the clock it
//! runs at (its MCU definition's `[[stage0.clock]]`), plus flash wait
//! states and XIP cache misses, which the estimate leaves out and the
//! stage0 build made to measure (`verify-time`) includes.
//!
//! Ignored by default (it needs the thumbv6m-none-eabi target):
//! `cargo test -p dagon-ed25519 --test m0_cycles -- --ignored --nocapture`.

mod armv6m;

use dagon_ed25519::{Ed25519Compact, Ed25519Small};
use dagon_format::header::SIGN_DOMAIN;
use dagon_format::SigVerifier;

#[allow(dead_code)]
mod vector {
    include!("../cycles/src/vector.rs");
}

/// The most cycles a verify may take before this test fails: the estimate
/// plus about a tenth, so a change that slows the verifier is seen.
const MAX_CYCLES: u64 = 55_000_000;

#[test]
fn the_vector_verifies_on_the_host() {
    let mut flipped = vector::SIG;
    flipped[5] ^= 4;
    let small = Ed25519Small::new(vector::KEY);
    let compact = Ed25519Compact::new(vector::KEY);
    for v in [&small as &dyn SigVerifier, &compact] {
        assert!(v.verify(SIGN_DOMAIN, &[&vector::BODY], &vector::SIG));
        assert!(!v.verify(SIGN_DOMAIN, &[&vector::BODY], &flipped));
    }
}

/// The most cycles a ticking verify may run between two ticks: under
/// 0.6 s at the RP2040 ring oscillator's slowest 1.8 MHz, a small part of
/// the 8 s trial watchdog period a boot core is armed with.
const MAX_CYCLES_BETWEEN_TICKS: u64 = 1_000_000;

#[test]
#[ignore = "builds a thumbv6m-none-eabi program"]
fn a_ticking_verify_ticks_at_most_every_max_cycles_between_ticks() {
    let elf = cycles_program();
    for (what, arg) in [("valid signature", 2), ("one bit flipped", 3)] {
        let r = armv6m::run(&elf, arg);
        println!(
            "{what}, ticking: {} cycles, {} ticks, at most {} cycles between ticks",
            r.cycles, r.peripheral_stores, r.longest_between_stores
        );
        assert_eq!(r.r0, u32::from(arg == 2), "{what}");
        assert!(
            r.longest_between_stores <= MAX_CYCLES_BETWEEN_TICKS,
            "{what}: {} cycles between ticks, over {MAX_CYCLES_BETWEEN_TICKS}",
            r.longest_between_stores
        );
    }
}

/// cycles/ built for the Cortex-M0+, as the stage0s build.
fn cycles_program() -> Vec<u8> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("cycles");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // In its own folder, so its .cargo/config.toml (target, linker script,
    // SHA-2 rounds) applies and nothing of this test's build does.
    let status = std::process::Command::new(cargo)
        .args(["build", "--release", "--locked"])
        .current_dir(&dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("cargo");
    assert!(status.success(), "building cycles/ failed");
    std::fs::read(dir.join("target/thumbv6m-none-eabi/release/dagon-ed25519-cycles"))
        .expect("the built program")
}

#[test]
#[ignore = "builds a thumbv6m-none-eabi program"]
fn a_verify_on_a_cortex_m0plus_takes_at_most_max_cycles() {
    let elf = cycles_program();
    let valid = armv6m::run(&elf, 0);
    let flipped = armv6m::run(&elf, 1);
    for (what, r) in [("valid signature", valid), ("one bit flipped", flipped)] {
        println!(
            "{what}: {} cycles, {} instructions, {} bytes of stack",
            r.cycles, r.instructions, r.stack
        );
    }
    assert_eq!((valid.r0, flipped.r0), (1, 0), "accepts only the valid one");
    let most = valid.cycles.max(flipped.cycles);
    assert!(most <= MAX_CYCLES, "{most} cycles, over {MAX_CYCLES}");
}
