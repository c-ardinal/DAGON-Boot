//! The boot core's sequence: every step from the armed watchdog to the
//! jump or the halt is a transition of [`sequence`], delivered to the boot
//! core's chain (`dagon_events::compose_boot`, built from the node's
//! generated `boot_wiring.rs`). [`BootRun`] runs it around dagon-core's
//! decision, which the chain only observes.
#![no_std]
#![forbid(unsafe_code)]

use dagon_core::boot::{decide, BootConfig, Decision, SlotLayout};
use dagon_core::log::{Log, LogLayout};
use dagon_events::{Refused, Sink};
use dagon_port::Flash;

// Links the standard library's critical section on hosted targets.
#[cfg(not(target_os = "none"))]
use critical_section as _;

dagon_events::state_machine! {
    /// The boot core's sequence from the armed watchdog to the jump or the
    /// halt. Its events reach the boot core's extensions as they happen;
    /// nothing an extension does changes the decision.
    pub machine sequence in Boot {
        initial Started;
        states { Started, WatchdogArmed, LogRead, BeforeDecision, Chosen, NoImage, Jumping, Halted }
        transitions {
            /// The watchdog runs with the trial period; from here
            /// extensions run.
            watchdog_armed(period_ms: u32): Started => WatchdogArmed
                emits WatchdogArmed = 0x01 { period_ms: u32 }
                observe(live);
            /// The state log was read (`ok: false`: no valid face).
            log_read(ok: bool, pending: bool, confirmed: bool, attempts: u8): WatchdogArmed => LogRead
                emits LogRead = 0x02 { ok: bool, pending: bool, confirmed: bool, attempts: u8 }
                observe(live);
            /// The last event before the decision is made.
            before_decision(pending: bool, confirmed: bool, attempts: u8): LogRead => BeforeDecision
                emits BeforeDecision = 0x03 { pending: bool, confirmed: bool, attempts: u8 }
                observe(live);
            /// The slot to start is final (`trial`: the pending slot on a
            /// counted attempt).
            slot_chosen(slot: u8, trial: bool): BeforeDecision => Chosen
                emits SlotChosen = 0x04 { slot: u8, trial: bool }
                observe(live);
            /// No slot can be started.
            no_image(): BeforeDecision => NoImage
                emits NoImage = 0x05 {}
                observe(live);
            /// The last event before control leaves DAGON.
            before_jump(slot: u8, trial: bool): Chosen => Jumping
                emits BeforeJump = 0x06 { slot: u8, trial: bool }
                observe(live);
            /// The boot core stays here for good.
            halt(): NoImage => Halted
                emits Halt = 0x07 {}
                observe(live);
        }
    }
}

/// One boot of a boot core, from the armed watchdog to the jump or the
/// halt. The decision is [`decide`]'s alone.
pub struct BootRun {
    machine: sequence::Machine,
    decision: Decision,
}

/// A transition taken in the order this module calls them.
fn taken(r: Result<(), Refused>) {
    debug_assert!(r.is_ok(), "{r:?}");
}

impl BootRun {
    /// Runs the boot up to the decision. Call it right after the watchdog
    /// was armed with `watchdog_ms` (the trial period), before anything
    /// else reads flash.
    pub fn start<F: Flash, S: Sink>(
        chain: &mut S,
        flash: &mut F,
        log: &LogLayout,
        slots: &SlotLayout,
        cfg: &BootConfig,
        watchdog_ms: u32,
    ) -> BootRun {
        let mut m = sequence::Machine::new();
        taken(m.watchdog_armed(chain, watchdog_ms));
        let loaded = Log::load(flash, log);
        let state = loaded.as_ref().map(|l| l.state).unwrap_or_default();
        let (pending, confirmed) = (state.pending.is_some(), state.confirmed.is_some());
        taken(m.log_read(chain, loaded.is_ok(), pending, confirmed, state.attempts));
        taken(m.before_decision(chain, pending, confirmed, state.attempts));
        let decision = match loaded {
            Ok(mut l) => decide(flash, &mut l, slots, cfg),
            Err(_) => Decision::NoImage,
        };
        match decision {
            Decision::Boot { slot, trial, .. } => taken(m.slot_chosen(chain, slot, trial)),
            Decision::NoImage => taken(m.no_image(chain)),
        }
        BootRun {
            machine: m,
            decision,
        }
    }

    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// The last event before the jump (a started slot only).
    pub fn before_jump<S: Sink>(&mut self, chain: &mut S) {
        if let Decision::Boot { slot, trial, .. } = self.decision {
            taken(self.machine.before_jump(chain, slot, trial));
        }
    }

    /// The boot core halts for good (no startable slot only).
    pub fn halt<S: Sink>(&mut self, chain: &mut S) {
        if self.decision == Decision::NoImage {
            taken(self.machine.halt(chain));
        }
    }
}
