//! The boot decision and the sandbox's observable contract, as the fuse
//! world drives them.
//!
//! `plan` and `commit` see only the state log (P8: their signatures are
//! asserted below, so neither can take the record or the fuse). The
//! sandbox model enforces a protection map: an extension's access outside
//! its declared windows faults without effect, a bus write to an undeclared
//! device is refused, a call past its budget is abandoned, and a handler
//! that faults or overruns `EXT_FAULT_LIMIT` times is disabled for the run.

/// The persistent image state the boot core decides from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogState {
    pub pending: Option<u8>,
    pub confirmed: Option<u8>,
    pub attempts: u8,
    pub max_attempts: u8,
    /// Signature and digest checks per slot with the default verifier.
    pub image_ok: [bool; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Trial(u8),
    Boot(u8),
    Rollback { from: u8, to: Option<u8> },
    NoImage,
}

/// The boot decision, with the image check as a port (the default
/// verifier, or a trusted replacement).
pub fn plan_with(log: &LogState, ok: &dyn Fn(u8) -> bool) -> Decision {
    let good = |s: Option<u8>| s.filter(|&s| ok(s));
    match log.pending {
        Some(p) if log.attempts < log.max_attempts && ok(p) => Decision::Trial(p),
        Some(p) => Decision::Rollback {
            from: p,
            to: good(log.confirmed),
        },
        None => good(log.confirmed).map_or(Decision::NoImage, Decision::Boot),
    }
}

pub fn plan(log: &LogState) -> Decision {
    plan_with(log, &|s| log.image_ok[s as usize])
}

pub fn commit(log: &LogState, d: Decision) -> LogState {
    match d {
        Decision::Trial(_) => LogState {
            attempts: log.attempts + 1,
            ..*log
        },
        Decision::Rollback { .. } => LogState {
            pending: None,
            attempts: 0,
            ..*log
        },
        Decision::Boot(_) | Decision::NoImage => *log,
    }
}

/// The app's confirm on trial.
pub fn confirm(log: &LogState) -> LogState {
    match log.pending {
        Some(p) => LogState {
            pending: None,
            confirmed: Some(p),
            attempts: 0,
            ..*log
        },
        None => *log,
    }
}

// P8: the decision functions take the state log and nothing else.
const _: fn(&LogState) -> Decision = plan;
const _: fn(&LogState, Decision) -> LogState = commit;
const _: fn(&LogState) -> LogState = confirm;

/// Faults or overruns before a handler is disabled for the run.
pub const EXT_FAULT_LIMIT: u8 = 3;
/// Time the unwind of an abandoned call takes.
pub const UNWIND_US: u32 = 20;

/// Boot steps at which extensions run (from `WatchdogArmed` on).
pub const STEPS: usize = 6;

/// What an extension tries to touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    FlashController,
    StateLog,
    Watchdog,
    DagonRam,
    BootRecord,
    Fuse,
    OtherExtension,
    OwnRam,
    /// A capability window by index.
    Window(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Return {
        us: u32,
    },
    /// Never returns; the sandbox timer ends it.
    Overrun,
    Access(Target),
    BusWrite {
        device: u8,
        register: u8,
    },
    /// Moves its stack out of its region: the core locks up and only the
    /// watchdog ends it.
    Lockup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub budget_us: u32,
    pub windows: Vec<u8>,
    pub devices: Vec<(u8, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ran { us: u32 },
    Faulted(Target),
    Overran,
    Refused,
    LockedUp,
}

/// DAGON's state an extension must not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Protected {
    pub flash: u32,
    pub log: u32,
    pub watchdog: u32,
    pub dagon_ram: u32,
    pub record: u32,
    pub fuse: u32,
    pub other_ext: u32,
}

/// One sandboxed call. Allowed accesses touch only the extension's own
/// memory; denied ones fault before any effect.
pub fn call(m: &Manifest, b: Behaviour, _p: &mut Protected) -> (Outcome, u32) {
    match b {
        Behaviour::Return { us } if us <= m.budget_us => (Outcome::Ran { us }, us),
        Behaviour::Return { .. } | Behaviour::Overrun => {
            (Outcome::Overran, m.budget_us + UNWIND_US)
        }
        Behaviour::Access(Target::OwnRam) => (Outcome::Ran { us: 1 }, 1),
        Behaviour::Access(Target::Window(w)) if m.windows.contains(&w) => {
            (Outcome::Ran { us: 1 }, 1)
        }
        Behaviour::Access(t) => (Outcome::Faulted(t), UNWIND_US),
        Behaviour::BusWrite { device, register } if m.devices.contains(&(device, register)) => {
            (Outcome::Ran { us: 5 }, 5)
        }
        Behaviour::BusWrite { .. } => (Outcome::Refused, 1),
        Behaviour::Lockup => (Outcome::LockedUp, 0),
    }
}

/// A trusted replacement of the image verifier (4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifierReplacement {
    /// Its id is listed in the project's trusted list.
    pub declared: bool,
    /// The linked artifact's hash equals the listed one.
    pub hash_matches: bool,
    /// The image carrying it is signed with the project key.
    pub image_signed: bool,
    /// What it answers for every slot.
    pub verdict: bool,
}

impl VerifierReplacement {
    pub fn accepted(&self) -> bool {
        self.declared && self.hash_matches && self.image_signed
    }
}

/// The decision the boot reaches: the replacement's verdict only when it
/// is accepted, else the default verifier.
pub fn expected(log: &LogState, v: Option<VerifierReplacement>) -> Decision {
    match v.filter(VerifierReplacement::accepted) {
        Some(r) => plan_with(log, &|_| r.verdict),
        None => plan(log),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_targets_fault_and_declared_windows_run() {
        let m = Manifest {
            budget_us: 100,
            windows: vec![1],
            devices: vec![(0x20, 3)],
        };
        let mut p = Protected::default();
        let at = |b| call(&m, b, &mut Protected::default()).0;
        assert_eq!(
            at(Behaviour::Access(Target::Window(1))),
            Outcome::Ran { us: 1 }
        );
        assert_eq!(
            at(Behaviour::Access(Target::Window(2))),
            Outcome::Faulted(Target::Window(2))
        );
        assert_eq!(
            at(Behaviour::BusWrite {
                device: 0x20,
                register: 4
            }),
            Outcome::Refused
        );
        assert_eq!(call(&m, Behaviour::Overrun, &mut p).1, 100 + UNWIND_US);
        assert_eq!(p, Protected::default());
    }

    #[test]
    fn untrusted_verifier_never_changes_the_decision() {
        let log = LogState {
            pending: Some(1),
            confirmed: Some(0),
            attempts: 0,
            max_attempts: 3,
            image_ok: [true, false],
        };
        let rogue = VerifierReplacement {
            declared: true,
            hash_matches: false,
            image_signed: true,
            verdict: true,
        };
        assert_eq!(expected(&log, Some(rogue)), plan(&log));
    }
}
