//! Seeded scenario runner and shrinker shared by the worlds.
//!
//! A world generates a configuration and an operation list from a seed,
//! runs them deterministically and checks its properties after every
//! operation. A failure shrinks to a minimal operation list that fails the
//! same property; its `Debug` form is a Rust expression to paste into
//! `tests/regressions.rs`.

use dagon_sim::SimRng;
use std::fmt::Debug;
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// Property id (P1..P26) or `panic`.
    pub property: &'static str,
    /// Index of the operation during which it failed.
    pub step: usize,
    pub detail: String,
}

impl Violation {
    pub fn new(property: &'static str, step: usize, detail: impl Into<String>) -> Self {
        Violation {
            property,
            step,
            detail: detail.into(),
        }
    }
}

pub trait World {
    type Config: Clone + Debug;
    type Op: Clone + Debug;

    fn generate(rng: &mut SimRng) -> (Self::Config, Vec<Self::Op>);
    /// `seed` drives the faults' own randomness (random bytes after power
    /// loss or an interrupted erase).
    fn run(cfg: &Self::Config, ops: &[Self::Op], seed: u64) -> Result<(), Violation>;
    /// Smaller variants of one operation, simplest first.
    fn simpler(op: &Self::Op) -> Vec<Self::Op>;
}

pub struct Failure<W: World> {
    pub seed: u64,
    pub config: W::Config,
    pub ops: Vec<W::Op>,
    pub violation: Violation,
}

impl<W: World> Debug for Failure<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Failure")
            .field("seed", &format_args!("{:#x}", self.seed))
            .field("violation", &self.violation)
            .field("config", &self.config)
            .field("ops", &self.ops)
            .finish()
    }
}

/// Seed of scenario `i` of a run started from `base` (SplitMix64 step).
pub fn seed_of(base: u64, i: u64) -> u64 {
    SimRng::new(base ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15)).next_u64()
}

pub fn check<W: World>(cfg: &W::Config, ops: &[W::Op], seed: u64) -> Result<(), Violation> {
    catch_unwind(AssertUnwindSafe(|| W::run(cfg, ops, seed))).unwrap_or_else(|p| {
        let msg = p
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        Err(Violation::new("panic", 0, msg))
    })
}

/// Runs `count` scenarios; on the first violation returns it shrunk.
pub fn run_many<W: World>(base: u64, count: u64) -> Result<u64, Box<Failure<W>>> {
    for i in 0..count {
        let seed = seed_of(base, i);
        let (cfg, ops) = W::generate(&mut SimRng::new(seed));
        if let Err(v) = check::<W>(&cfg, &ops, seed) {
            let ops = shrink::<W>(&cfg, ops, seed, v.property);
            let violation = check::<W>(&cfg, &ops, seed).expect_err("shrunk case still fails");
            return Err(Box::new(Failure {
                seed,
                config: cfg,
                ops,
                violation,
            }));
        }
    }
    Ok(count)
}

/// Removes chunks, then simplifies single operations, while the same
/// property still fails.
pub fn shrink<W: World>(
    cfg: &W::Config,
    mut ops: Vec<W::Op>,
    seed: u64,
    property: &'static str,
) -> Vec<W::Op> {
    let fails = |o: &[W::Op]| matches!(check::<W>(cfg, o, seed), Err(v) if v.property == property);
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for _ in 0..8 {
        let before = ops.len();
        let mut chunk = ops.len().max(1);
        while chunk >= 1 {
            let mut i = 0;
            while i < ops.len() {
                let mut t = ops.clone();
                t.drain(i..(i + chunk).min(t.len()));
                if fails(&t) {
                    ops = t;
                } else {
                    i += chunk;
                }
            }
            chunk /= 2;
        }
        let mut changed = false;
        for i in 0..ops.len() {
            for s in W::simpler(&ops[i]) {
                let mut t = ops.clone();
                t[i] = s;
                if fails(&t) {
                    ops = t;
                    changed = true;
                    break;
                }
            }
        }
        if ops.len() == before && !changed {
            break;
        }
    }
    std::panic::set_hook(quiet);
    ops
}

/// Scenario count and base seed for a run: `DAGON_MODEL_SCENARIOS` and
/// `DAGON_MODEL_SEED` override the defaults.
pub fn run_params(default_count: u64, default_seed: u64) -> (u64, u64) {
    let get = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| {
                let v = v.trim();
                v.strip_prefix("0x")
                    .map(|h| u64::from_str_radix(h, 16).ok())
                    .unwrap_or_else(|| v.parse().ok())
            })
            .unwrap_or(d)
    };
    (
        get("DAGON_MODEL_SCENARIOS", default_count),
        get("DAGON_MODEL_SEED", default_seed),
    )
}
