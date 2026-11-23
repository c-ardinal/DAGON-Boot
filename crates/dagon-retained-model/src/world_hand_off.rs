//! App hand-off scenarios (ESP32-S3: the boot core asks stage0 to start
//! the app by reset): the production cell codec and rules
//! (`dagon_core::bootcore`: `take_request`, `app_result`, `hand_off`,
//! `cell_holds`, and `choose`'s cell for the confirmed core) over the
//! two-word retained cell, with a reset or a power loss injectable after
//! every store to the cell (the boot core's request, stage0's take, its
//! result, the confirmed core's start, the cell checks and the boot core's
//! clearing of a result). The app is good, absent or broken, and changes
//! between boots as an update in recovery mode changes it. Properties,
//! after every operation:
//!
//! - H1: stage0 runs the app decision only on a request the boot core
//!   wrote whole, and at most once for each;
//! - H2: with no further fault, one boot ends with the app running when
//!   it is good, and otherwise with the boot core in recovery mode, after
//!   at most two decisions;
//! - H3: the boot core stays in recovery only on a result stage0 wrote
//!   for the app as it is.

use crate::harness::{Violation, World};
use crate::rng::Rng;
use dagon_core::bootcore::{
    app_result, cell_holds, choose, hand_off, take_request, HandOff, Refusal, CLEARED, START_APP,
};
use dagon_core::log::{LogState, SlotRef};
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

/// The app the log names, as stage0's decision finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Good,
    /// The log names none.
    Absent,
    /// It fails its checks.
    Broken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// An update in recovery mode leaves this app.
    SetApp(App),
    /// From a reset: stage0 and the boot core, as many resets as the
    /// hand-off takes, cut by `fault`.
    Boot { fault: Option<Fault> },
}

/// How a boot ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    AppRuns,
    Recovery,
}

/// Ends a boot early (the fault that cut it).
struct Cut;

#[derive(Clone)]
struct Device {
    cell: [u32; 2],
    app: App,
    /// The boot core wrote a whole request that no decision took yet.
    armed: bool,
    /// The result in the cell (if any) was written for the app as it is.
    result_fresh: bool,
    decisions: u32,
    rng: Rng,
}

/// The boot core's log: one confirmed core, nothing pending.
fn core_log() -> LogState {
    LogState {
        confirmed: Some(SlotRef {
            slot: 0,
            version: Version::new(1, 0, 0),
            digest16: [0xA0; 16],
        }),
        ..LogState::default()
    }
}

impl Device {
    fn store(&mut self, value: [u32; 2], n: &mut u8, fault: Option<Fault>) -> Result<(), Cut> {
        for (i, w) in value.into_iter().enumerate() {
            self.cell[i] = w;
            self.after_store(n, fault)?;
        }
        Ok(())
    }

    fn after_store(&mut self, n: &mut u8, fault: Option<Fault>) -> Result<(), Cut> {
        let at = *n;
        *n += 1;
        match fault {
            Some(Fault::Reset { at: f }) if f == at => Err(Cut),
            Some(Fault::PowerLoss { at: f, random }) if f == at => {
                self.cell = if random {
                    [self.rng.next_u64() as u32, self.rng.next_u64() as u32]
                } else {
                    CLEARED
                };
                self.armed = false;
                Err(Cut)
            }
            _ => Ok(()),
        }
    }

    /// A production rule that stores through `write(index, word)`, each
    /// store counted and a fault after one cutting the boot there.
    fn through<T>(
        &mut self,
        n: &mut u8,
        fault: Option<Fault>,
        rule: impl FnOnce(&dyn Fn() -> [u32; 2], &mut dyn FnMut(usize, u32)) -> T,
    ) -> Result<T, Cut> {
        let me = RefCell::new((&mut *self, *n, false));
        let out = rule(&|| me.borrow().0.cell, &mut |i, w| {
            let (d, n, cut) = &mut *me.borrow_mut();
            if !*cut {
                d.cell[i] = w;
                *cut = d.after_store(n, fault).is_err();
            }
        });
        let (_, at, cut) = me.into_inner();
        *n = at;
        if cut {
            return Err(Cut);
        }
        Ok(out)
    }

    fn check_cell(&mut self, n: &mut u8, fault: Option<Fault>) -> Result<(), Cut> {
        let held = self.through(n, fault, |r, w| cell_holds(r, w))?;
        assert!(held, "the modelled cell holds");
        Ok(())
    }

    /// One boot from a reset; `step` names the operation for violations.
    fn boot(&mut self, fault: Option<Fault>, step: usize) -> Result<Result<End, Cut>, Violation> {
        let mut n = 0u8;
        // Stage0 passes: each request ends in a reset back to stage0.
        for _ in 0..4 {
            let taken = match self.through(&mut n, fault, |r, w| take_request(r, w)) {
                Ok(t) => t,
                Err(c) => return Ok(Err(c)),
            };
            if taken {
                if !self.armed {
                    return Err(Violation::new(
                        "H1",
                        step,
                        "the app decision ran with no whole request",
                    ));
                }
                self.armed = false;
                self.decisions += 1;
                let refusal = match self.app {
                    App::Good => {
                        // The app runs; its store checks the cell.
                        return Ok(self.check_cell(&mut n, fault).map(|()| End::AppRuns));
                    }
                    App::Absent => Refusal::NoImage,
                    App::Broken => Refusal::Rejected,
                };
                self.result_fresh = true;
                if let Err(c) = self.store(app_result(refusal), &mut n, fault) {
                    return Ok(Err(c));
                }
            } else {
                // The confirmed core, as `choose` leaves the cell for it.
                let start = choose(&core_log(), self.cell, 3, 1, |_, _| Ok::<(), ()>(()))
                    .expect("the confirmed core checks out");
                if let Err(c) = self.store(start.cell, &mut n, fault) {
                    return Ok(Err(c));
                }
            }
            // The boot core reads the cell, checks it holds, then hands off.
            let seen = self.cell;
            if let Err(c) = self.check_cell(&mut n, fault) {
                return Ok(Err(c));
            }
            match hand_off(seen) {
                HandOff::Stay(_) => {
                    if !self.result_fresh {
                        return Err(Violation::new(
                            "H3",
                            step,
                            format!("stayed on {seen:x?} not written for this app"),
                        ));
                    }
                    return Ok(self.store(CLEARED, &mut n, fault).map(|()| End::Recovery));
                }
                HandOff::Request => {
                    let stored = self.store(START_APP, &mut n, fault);
                    // Whole once both words landed, even if a reset
                    // follows the second.
                    self.armed = self.cell == START_APP;
                    if let Err(c) = stored {
                        return Ok(Err(c));
                    }
                }
            }
        }
        Err(Violation::new("H2", step, "the hand-off did not end"))
    }
}

pub struct HandOffWorld;

impl World for HandOffWorld {
    type Config = ();
    type Op = Op;

    fn generate(rng: &mut Rng) -> ((), Vec<Op>) {
        let ops = (0..rng.below(40))
            .map(|_| {
                if rng.chance(0.2) {
                    Op::SetApp(match rng.below(3) {
                        0 => App::Good,
                        1 => App::Absent,
                        _ => App::Broken,
                    })
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
        ((), ops)
    }

    fn run(_: &(), ops: &[Op], seed: u64) -> Result<(), Violation> {
        let mut d = Device {
            cell: CLEARED,
            app: App::Good,
            armed: false,
            result_fresh: false,
            decisions: 0,
            rng: Rng::new(seed),
        };
        for (step, op) in ops.iter().enumerate() {
            match *op {
                Op::SetApp(a) => {
                    d.app = a;
                    d.result_fresh = false;
                }
                Op::Boot { fault } => {
                    let _ = d.boot(fault, step)?;
                }
            }
            // H2: from here, with no fault, one boot ends as the app says.
            let mut quiet = Device {
                rng: Rng::new(seed ^ step as u64),
                decisions: 0,
                ..d.clone()
            };
            let end = quiet.boot(None, step)?;
            let want = if d.app == App::Good {
                End::AppRuns
            } else {
                End::Recovery
            };
            if !matches!(end, Ok(e) if e == want) || quiet.decisions > 2 {
                return Err(Violation::new(
                    "H2",
                    step,
                    format!(
                        "app {:?}: ended {:?} after {} decisions",
                        d.app,
                        end.ok(),
                        quiet.decisions
                    ),
                ));
            }
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
