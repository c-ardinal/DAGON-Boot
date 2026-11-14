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

#[test]
#[ignore = "builds a thumbv6m-none-eabi program"]
fn a_verify_on_a_cortex_m0plus_takes_at_most_max_cycles() {
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
    let elf = std::fs::read(dir.join("target/thumbv6m-none-eabi/release/dagon-ed25519-cycles"))
        .expect("the built program");
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
