//! Seeded scenario runs of every world: the default profile in `cargo
//! test`, the full profile as `--ignored` (run it with `--release`).

use dagon_retained_model::harness::{run_many, run_params, World};
use dagon_retained_model::world_fuse::FuseWorld;
use dagon_retained_model::world_record::RecordWorld;
use dagon_retained_model::world_topology::TopologyWorld;
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
fn record_default() {
    go::<RecordWorld>("record", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn record_full() {
    go::<RecordWorld>("record", FULL);
}

#[test]
fn fuse_default() {
    go::<FuseWorld>("fuse", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn fuse_full() {
    go::<FuseWorld>("fuse", FULL);
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
fn topology_default() {
    go::<TopologyWorld>("topology", DEFAULT);
}

#[test]
#[ignore = "full profile: cargo test --release -- --ignored"]
fn topology_full() {
    go::<TopologyWorld>("topology", FULL);
}
