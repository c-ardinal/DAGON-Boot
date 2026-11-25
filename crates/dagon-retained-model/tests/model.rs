//! Seeded scenario runs of every world: the default profile in `cargo
//! test`, the full profile as `--ignored` (run it with `--release`).

use dagon_retained_model::codec::{CodecWorld, TopologyCodec, TrialCellCodec};
use dagon_retained_model::harness::{run_many, run_params, World};
use dagon_retained_model::identity::{IdentityWorld, Production};
use dagon_retained_model::world_hand_off::HandOffWorld;
use dagon_retained_model::world_trial::TrialWorld;

const DEFAULT: u64 = 10_000;
const FULL: u64 = 1_000_000;
const DEFAULT_SEED: u64 = 0x5EED_D460;

fn go<W: World>(name: &str, default_count: u64) {
    let (count, base) = run_params(default_count, DEFAULT_SEED);
    match run_many::<W>(base, count) {
        Ok(n) => println!("{name}: {n} scenarios from base seed {base:#x}, no violation"),
        Err(f) => panic!("{name}: {f:#?}"),
    }
}

#[test]
fn trial_default() {
    go::<TrialWorld>("trial", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn trial_full() {
    go::<TrialWorld>("trial", FULL);
}

#[test]
fn hand_off_default() {
    go::<HandOffWorld>("hand_off", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn hand_off_full() {
    go::<HandOffWorld>("hand_off", FULL);
}

#[test]
fn identity_default() {
    go::<IdentityWorld<Production>>("identity", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn identity_full() {
    go::<IdentityWorld<Production>>("identity", FULL);
}

#[test]
fn topology_codec_default() {
    go::<CodecWorld<TopologyCodec>>("topology codec", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn topology_codec_full() {
    go::<CodecWorld<TopologyCodec>>("topology codec", FULL);
}

#[test]
fn trial_cell_codec_default() {
    go::<CodecWorld<TrialCellCodec>>("trial cell codec", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn trial_cell_codec_full() {
    go::<CodecWorld<TrialCellCodec>>("trial cell codec", FULL);
}
