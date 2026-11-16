//! Re-attach to a host that never resets the bus.
//!
//! A USB device that has turned its pull-up on waits for the host to reset
//! the bus. If that one connect event is lost (a hub or host that missed
//! the change, a port the host gave up on), nothing else will make the host
//! look again, and the device waits for ever. [`Reattach`] decides when to
//! drop the pull-up and turn it on again so the host sees a fresh connect;
//! the MCU package supplies the clock, its bus-reset count and the
//! [`Pullup`] hooks.

/// How long after the pull-up comes on a bus reset may take before the
/// device re-attaches. A host resets the bus about 100 ms after it sees the
/// connect (USB 2.0 section 7.1.7.3, TATTDB).
pub const WAIT_MS: u64 = 1_000;
/// How long the pull-up stays off: long enough for a host or hub still
/// removing the previous device to see this one leave and come back.
pub const DETACH_MS: u64 = 500;
/// Re-attaches after the first attach before the device stops trying.
pub const TRIES: u32 = 3;
/// From the first attach to the last wait's end, when nothing ever resets
/// the bus; an MCU package checks it fits its trial watchdog period.
pub const SPAN_MS: u64 = WAIT_MS + TRIES as u64 * (DETACH_MS + WAIT_MS);

/// The device's D+ (or D-) pull-up, which tells the host it is there.
pub trait Pullup {
    fn attach(&mut self);
    fn detach(&mut self);
}

enum State {
    /// Pull-up on since `at`, with `resets` bus resets counted then.
    Attached {
        at: u64,
        resets: u32,
    },
    Detached {
        at: u64,
    },
    Done,
}

/// The re-attach decision, stepped by the caller with its clock and its
/// count of bus resets seen.
pub struct Reattach {
    state: State,
    tries: u32,
}

impl Reattach {
    /// Starts at the attach made at `now_ms`, when `resets` bus resets had
    /// been counted.
    pub fn new(now_ms: u64, resets: u32) -> Self {
        Reattach {
            state: State::Attached { at: now_ms, resets },
            tries: 0,
        }
    }

    /// Acts on the time and the bus-reset count; returns when to call
    /// again, or `None` once the host has reset the bus or the tries are
    /// used up (the pull-up is then on).
    pub fn step(&mut self, now_ms: u64, resets: u32, pullup: &mut impl Pullup) -> Option<u64> {
        match self.state {
            State::Done => None,
            State::Attached { resets: seen, .. } if resets != seen => {
                self.state = State::Done;
                None
            }
            State::Attached { at, .. } if now_ms < at + WAIT_MS => Some(at + WAIT_MS),
            State::Attached { .. } if self.tries == TRIES => {
                self.state = State::Done;
                None
            }
            State::Attached { .. } => {
                pullup.detach();
                self.tries += 1;
                self.state = State::Detached { at: now_ms };
                Some(now_ms + DETACH_MS)
            }
            State::Detached { at } if now_ms < at + DETACH_MS => Some(at + DETACH_MS),
            // Resets counted while detached (SE0 can read as one) are not
            // the host's answer to this attach.
            State::Detached { .. } => {
                pullup.attach();
                self.state = State::Attached { at: now_ms, resets };
                Some(now_ms + WAIT_MS)
            }
        }
    }
}
