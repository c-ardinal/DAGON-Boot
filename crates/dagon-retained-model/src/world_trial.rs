//! Boot core trial cell scenarios: the production codec and the stage0 and
//! boot core rules (`dagon_core::bootcore`) over a two-word retained cell,
//! with a reset or a power loss injectable after every store to the cell
//! (each word is one store, so a fault may land between the two), and
//! after power loss the cell either cleared or random. The stores include
//! those of the production cell check (`bootcore::cell_holds`), run as the
//! MCUs run it: by the started boot core and again by the app. Both MCUs
//! store word 0, then word 1. New cores are set
//! pending, good or broken (a broken core passes stage0's image check but
//! fails its own). Properties C1..C3 of retained-formats.md 7, after every
//! operation:
//!
//! - C1: between two power losses a pending core is started on trial at
//!   most `max` times;
//! - C2: with no further fault, at most `max + 1` boots end the trial: no
//!   core is pending and a core runs;
//! - C3: a broken core is never confirmed.

use crate::harness::{Violation, World};
use crate::rng::Rng;
use dagon_core::bootcore::{cell_holds, choose, settle, tried, Settle, CLEARED};
use dagon_core::log::{LogState, SlotRef};
use dagon_format::statelog::{kind, reason, target, Record};
use dagon_format::Version;
use std::cell::RefCell;

/// A fault after the cell store number `at` (0-based) of one boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A reset: the cell keeps what was stored.
    Reset { at: u8 },
    /// A power loss: the cell reads cleared or random afterwards.
    PowerLoss { at: u8, random: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// The agent sets a new core pending in the idle slot.
    SetPending { good: bool },
    /// One boot (stage0, then the core it starts), cut by `fault`.
    Boot { fault: Option<Fault> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Starts a pending core gets.
    pub max: u8,
}

#[derive(Clone, Copy)]
struct Core {
    at: SlotRef,
    good: bool,
}

#[derive(Clone)]
struct Device {
    log: LogState,
    seq: u32,
    cores: [Option<Core>; 2],
    cell: [u32; 2],
    /// Trial starts of the pending core since the last power loss.
    trial_starts: u32,
    next_version: u16,
    rng: Rng,
}

/// Ends a boot early (the fault that cut it).
struct Cut;

impl Device {
    fn record(&mut self, k: u8, at: SlotRef, why: u8) {
        self.seq += 1;
        let r = Record {
            kind: k,
            target: target::BOOTCORE,
            slot: at.slot,
            reason: why,
            seq: self.seq,
            version: at.version,
            digest16: at.digest16,
        };
        self.log = self.log.apply_core(&r);
    }

    /// The stores to the cell, one word each; `*n` counts them in this
    /// boot, and the fault lands after the store it names.
    fn store(&mut self, value: [u32; 2], n: &mut u8, fault: Option<Fault>) -> Result<(), Cut> {
        for (i, w) in value.into_iter().enumerate() {
            self.cell[i] = w;
            self.after_store(n, fault)?;
        }
        Ok(())
    }

    /// Counts a store and applies the fault that lands after it.
    fn after_store(&mut self, n: &mut u8, fault: Option<Fault>) -> Result<(), Cut> {
        let at = *n;
        *n += 1;
        match fault {
            Some(Fault::Reset { at: f }) if f == at => Err(Cut),
            Some(Fault::PowerLoss { at: f, random }) if f == at => {
                self.power_loss(random);
                Err(Cut)
            }
            _ => Ok(()),
        }
    }

    /// The production cell check over the device's cell; a fault after one
    /// of its stores cuts the boot there (no later store runs).
    fn check_cell(&mut self, n: &mut u8, fault: Option<Fault>) -> Result<bool, Cut> {
        let me = RefCell::new((&mut *self, *n, false));
        let held = cell_holds(
            || me.borrow().0.cell,
            |i, w| {
                let (d, n, cut) = &mut *me.borrow_mut();
                if !*cut {
                    d.cell[i] = w;
                    *cut = d.after_store(n, fault).is_err();
                }
            },
        );
        let (_, at, cut) = me.into_inner();
        *n = at;
        if cut {
            return Err(Cut);
        }
        Ok(held)
    }

    fn power_loss(&mut self, random: bool) {
        self.cell = if random {
            [self.rng.next_u64() as u32, self.rng.next_u64() as u32]
        } else {
            CLEARED
        };
        self.trial_starts = 0;
    }

    fn set_pending(&mut self, good: bool) {
        let running = self.log.confirmed.map_or(0, |c| c.slot);
        let slot = 1 - running;
        self.next_version += 1;
        let at = SlotRef {
            slot,
            version: Version::new(1, 0, self.next_version),
            digest16: [self.next_version as u8; 16],
        };
        self.cores[usize::from(slot)] = Some(Core { at, good });
        self.record(kind::SLOT_PENDING, at, reason::NONE);
    }

    /// Stage0 and the started core; `Err` when the fault cut the boot.
    fn boot(&mut self, max: u8, fault: Option<Fault>) -> Result<u8, Cut> {
        let mut n = 0u8;
        let cores = self.cores;
        let start = choose(&self.log, self.cell, max, |s, r| {
            match cores[usize::from(s)] {
                Some(c) if r.is_none_or(|r| *r == c.at) => Ok(()),
                _ => Err(()),
            }
        })
        .expect("a core always checks out here");
        if start.trial {
            self.trial_starts += 1;
        }
        self.store(start.cell, &mut n, fault)?;
        let core = cores[usize::from(start.slot)].expect("started");
        // The boot core reads the cell, then checks it holds.
        let p = self.log.pending.map_or(0, |p| tried(self.cell, &p));
        assert!(self.check_cell(&mut n, fault)?, "the modelled cell holds");
        match settle(&self.log, start.slot, p, max) {
            Settle::Nothing => {}
            Settle::Confirm(p) => {
                if !core.good {
                    // It fails its own check and resets.
                    return Err(Cut);
                }
                self.record(kind::CONFIRMED, p, reason::NONE);
                self.store(CLEARED, &mut n, fault)?;
            }
            Settle::Rollback { back, reason } => {
                self.record(kind::ROLLBACK, back, reason);
                self.store(CLEARED, &mut n, fault)?;
            }
        }
        // The app's store checks the cell before it takes a boot core.
        self.check_cell(&mut n, fault)?;
        Ok(start.slot)
    }

    fn check(&self, max: u8, step: usize) -> Result<(), Violation> {
        if self.trial_starts > u32::from(max) {
            return Err(Violation::new(
                "C1",
                step,
                format!("{} trial starts, max {max}", self.trial_starts),
            ));
        }
        let broken_confirmed = self
            .log
            .confirmed
            .and_then(|c| self.cores[usize::from(c.slot)].filter(|x| x.at == c))
            .is_some_and(|c| !c.good);
        if broken_confirmed {
            return Err(Violation::new("C3", step, "a broken core was confirmed"));
        }
        Ok(())
    }
}

pub struct TrialWorld;

impl World for TrialWorld {
    type Config = Config;
    type Op = Op;

    fn generate(rng: &mut Rng) -> (Config, Vec<Op>) {
        let cfg = Config {
            max: 1 + rng.below(5) as u8,
        };
        let ops = (0..rng.below(40))
            .map(|_| {
                if rng.chance(0.2) {
                    Op::SetPending {
                        good: rng.chance(0.5),
                    }
                } else {
                    let at = rng.below(12) as u8;
                    let fault = match rng.below(4) {
                        0 => Some(Fault::Reset { at }),
                        1 => Some(Fault::PowerLoss {
                            at,
                            random: rng.chance(0.5),
                        }),
                        _ => None,
                    };
                    Op::Boot { fault }
                }
            })
            .collect();
        (cfg, ops)
    }

    fn run(cfg: &Config, ops: &[Op], seed: u64) -> Result<(), Violation> {
        let a = SlotRef {
            slot: 0,
            version: Version::new(1, 0, 0),
            digest16: [0xA0; 16],
        };
        let mut d = Device {
            log: LogState::default(),
            seq: 0,
            cores: [Some(Core { at: a, good: true }), None],
            cell: CLEARED,
            trial_starts: 0,
            next_version: 0,
            rng: Rng::new(seed),
        };
        d.record(kind::PROVISION, a, reason::NONE);
        for (step, op) in ops.iter().enumerate() {
            match *op {
                Op::SetPending { good } => {
                    if d.log.pending.is_none() {
                        d.set_pending(good);
                        d.trial_starts = 0;
                    }
                }
                Op::Boot { fault } => {
                    let _ = d.boot(cfg.max, fault);
                }
            }
            d.check(cfg.max, step)?;
            // C2: from here, with no fault, max + 1 boots settle the trial.
            let mut quiet = Device {
                rng: Rng::new(seed ^ step as u64),
                ..d.clone()
            };
            let mut ran = None;
            for _ in 0..=cfg.max {
                if let Ok(s) = quiet.boot(cfg.max, None) {
                    ran = Some(s);
                    if quiet.log.pending.is_none() {
                        break;
                    }
                }
            }
            if quiet.log.pending.is_some() || ran.is_none() {
                return Err(Violation::new(
                    "C2",
                    step,
                    format!(
                        "pending {:?} after {} boots",
                        quiet.log.pending,
                        cfg.max + 1
                    ),
                ));
            }
            quiet.check(cfg.max, step)?;
        }
        Ok(())
    }

    fn simpler(op: &Op) -> Vec<Op> {
        match *op {
            Op::Boot { fault: Some(_) } => vec![Op::Boot { fault: None }],
            _ => Vec::new(),
        }
    }
}
