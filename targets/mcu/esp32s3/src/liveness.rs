//! Liveness of the node's tasks: the watchdog is fed only while no task has
//! overrun the deadline it announced.
//!
//! A task takes a [`Lease`], calls [`Lease::busy`] with a time budget before
//! each piece of work and [`Lease::idle`] when it goes back to waiting for an
//! event. A task stuck in its work misses the deadline, the feeding stops and
//! the hardware watchdog resets the chip instead of leaving it silently hung.
//! Lock-free (atomics only), so any task or interrupt priority may use it.
//!
//! One task may register as preempting ([`Liveness::register_preempting`]):
//! the link task, which runs above the others. Around each synchronous
//! section in which it keeps the CPU (agent request handling, flash erase,
//! write and verify; a slot erase takes up to about 30 s) it calls
//! [`Lease::hold_cpu`] and [`Lease::release_cpu`]. During such a section its
//! own deadline counts against the time and the others' against the start
//! of the section (one already overdue stays overdue); afterwards every
//! other deadline moves later by exactly the section's length. Time it spends awaiting (reply
//! transmission, waiting for input) is not excused: lower tasks can run then.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

/// Tasks that can register.
pub const MAX_TASKS: usize = 4;

const FREE: u8 = 0;
const IDLE: u8 = 1;
const BUSY: u8 = 2;
const NONE: u8 = u8::MAX;

pub struct Liveness {
    state: [AtomicU8; MAX_TASKS],
    /// Milliseconds since boot (wrapping) at which a busy task is overdue.
    deadline: [AtomicU32; MAX_TASKS],
    /// Slot of the preempting task, or `NONE`.
    preempting: AtomicU8,
    /// The preempting task is inside a synchronous section.
    holding: AtomicBool,
    /// Start of that section (ms, wrapping).
    hold_since: AtomicU32,
}

/// One task's registration.
pub struct Lease {
    owner: &'static Liveness,
    slot: usize,
}

impl Liveness {
    pub const fn new() -> Self {
        Self {
            state: [const { AtomicU8::new(FREE) }; MAX_TASKS],
            deadline: [const { AtomicU32::new(0) }; MAX_TASKS],
            preempting: AtomicU8::new(NONE),
            holding: AtomicBool::new(false),
            hold_since: AtomicU32::new(0),
        }
    }

    /// Registers a task (idle). `None` when all places are taken.
    pub fn register(&'static self) -> Option<Lease> {
        let slot = self.state.iter().position(|s| {
            s.compare_exchange(FREE, IDLE, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        })?;
        Some(Lease { owner: self, slot })
    }

    /// Registers the task that preempts all others (at most one; `None` if
    /// there already is one or all places are taken).
    pub fn register_preempting(&'static self) -> Option<Lease> {
        if self.preempting.load(Ordering::Acquire) != NONE {
            return None;
        }
        let lease = self.register()?;
        self.preempting.store(lease.slot as u8, Ordering::Release);
        Some(lease)
    }

    /// Whether a busy task is past its deadline at `now_ms`. While the
    /// preempting task holds the CPU the other tasks' clocks stand at the
    /// start of the section: one already overdue then stays overdue.
    pub fn overdue(&self, now_ms: u32) -> bool {
        let p = self.preempting.load(Ordering::Acquire);
        let frozen_at = (p != NONE && self.holding.load(Ordering::Acquire))
            .then(|| self.hold_since.load(Ordering::Acquire));
        (0..MAX_TASKS).any(|i| {
            let at = match frozen_at {
                Some(since) if i != p as usize => since,
                _ => now_ms,
            };
            self.state[i].load(Ordering::Acquire) == BUSY
                && at.wrapping_sub(self.deadline[i].load(Ordering::Acquire)) as i32 >= 0
        })
    }
}

impl Default for Liveness {
    fn default() -> Self {
        Self::new()
    }
}

impl Lease {
    /// Starts work that must finish within `budget_ms` of `now_ms`.
    pub fn busy(&self, now_ms: u32, budget_ms: u32) {
        let o = self.owner;
        o.deadline[self.slot].store(now_ms.wrapping_add(budget_ms), Ordering::Release);
        o.state[self.slot].store(BUSY, Ordering::Release);
    }

    /// Back to waiting for an event: no deadline.
    pub fn idle(&self) {
        self.owner.state[self.slot].store(IDLE, Ordering::Release);
    }

    /// Preempting task only: from `now_ms` on it keeps the CPU without
    /// yielding (no effect for other leases).
    pub fn hold_cpu(&self, now_ms: u32) {
        let o = self.owner;
        if self.is_preempting() && !o.holding.load(Ordering::Acquire) {
            o.hold_since.store(now_ms, Ordering::Release);
            o.holding.store(true, Ordering::Release);
        }
    }

    /// Preempting task only: the synchronous section ends at `now_ms`; every
    /// other busy deadline moves later by its length. Returns that length.
    pub fn release_cpu(&self, now_ms: u32) -> u32 {
        let o = self.owner;
        if !self.is_preempting() || !o.holding.load(Ordering::Acquire) {
            return 0;
        }
        let held = now_ms.wrapping_sub(o.hold_since.load(Ordering::Acquire));
        // Every slot, not only busy ones: a task preempted inside `busy`
        // between storing its deadline and its state gets the shift too
        // (an idle or free slot's deadline is rewritten before it counts).
        for i in (0..MAX_TASKS).filter(|&i| i != self.slot) {
            o.deadline[i].fetch_add(held, Ordering::AcqRel);
        }
        o.holding.store(false, Ordering::Release);
        held
    }

    fn is_preempting(&self) -> bool {
        self.owner.preempting.load(Ordering::Acquire) as usize == self.slot
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.is_preempting() {
            self.owner.holding.store(false, Ordering::Release);
            self.owner.preempting.store(NONE, Ordering::Release);
        }
        self.owner.state[self.slot].store(FREE, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overdue_only_while_busy_past_the_deadline() {
        static L: Liveness = Liveness::new();
        let a = L.register().unwrap();
        let b = L.register().unwrap();
        assert!(!L.overdue(1_000));
        a.busy(1_000, 500);
        assert!(!L.overdue(1_499));
        assert!(L.overdue(1_500));
        a.idle();
        assert!(!L.overdue(9_999));
        // Deadlines across the u32 wrap.
        b.busy(u32::MAX - 10, 100);
        assert!(!L.overdue(u32::MAX));
        assert!(!L.overdue(50));
        assert!(L.overdue(89));
        drop(b);
        assert!(!L.overdue(89));
    }

    #[test]
    fn a_long_synchronous_section_does_not_starve_the_others() {
        static L: Liveness = Liveness::new();
        let link = L.register_preempting().unwrap();
        let demo = L.register().unwrap();
        assert!(L.register_preempting().is_none());
        // Demo starts 500 ms of work, then the link erases for 5 s without
        // yielding: the demo cannot run meanwhile.
        demo.busy(1_000, 500);
        link.busy(1_100, 30_000);
        link.hold_cpu(1_100);
        assert!(!L.overdue(1_600));
        assert!(!L.overdue(6_000));
        assert_eq!(link.release_cpu(6_100), 5_000);
        // The demo's deadline moved by the 5 s the link held the CPU.
        assert!(!L.overdue(6_100));
        assert!(!L.overdue(6_499));
        // Genuinely stuck after the section: overdue, feeding stops.
        assert!(L.overdue(6_500));
        link.idle();
        // A link stuck inside a section is caught on its own deadline.
        demo.idle();
        link.busy(7_000, 1_000);
        link.hold_cpu(7_000);
        assert!(L.overdue(8_000));
        link.release_cpu(8_000);
    }

    #[test]
    fn a_section_does_not_excuse_a_task_already_overdue() {
        static L: Liveness = Liveness::new();
        let link = L.register_preempting().unwrap();
        let demo = L.register().unwrap();
        demo.busy(1_000, 500);
        assert!(L.overdue(1_600));
        // The link starts a long erase after the demo is already stuck.
        link.busy(1_600, 30_000);
        link.hold_cpu(1_600);
        assert!(L.overdue(1_700), "excused while the link holds the CPU");
        assert!(L.overdue(10_000));
        link.release_cpu(10_000);
        assert!(L.overdue(10_000));
    }

    #[test]
    fn a_deadline_stored_just_before_a_section_moves_with_it() {
        static L: Liveness = Liveness::new();
        let link = L.register_preempting().unwrap();
        let demo = L.register().unwrap();
        // The demo was preempted inside `busy`: deadline stored, state
        // still idle when the section ran.
        L.deadline[demo.slot].store(1_500, Ordering::Release);
        link.hold_cpu(1_000);
        link.release_cpu(6_000);
        L.state[demo.slot].store(BUSY, Ordering::Release);
        assert!(!L.overdue(6_499));
        assert!(L.overdue(6_500));
    }

    #[test]
    fn awaiting_the_reply_does_not_excuse_a_stuck_task() {
        static L: Liveness = Liveness::new();
        let link = L.register_preempting().unwrap();
        let demo = L.register().unwrap();
        demo.busy(1_000, 500);
        // Short synchronous handling, then 5 s waiting for the reply to
        // leave: the demo could run but does not.
        link.busy(1_000, 30_000);
        link.hold_cpu(1_000);
        assert_eq!(link.release_cpu(1_050), 50);
        assert!(!L.overdue(1_549));
        assert!(L.overdue(1_550));
        assert!(L.overdue(6_000));
        link.idle();
        assert!(L.overdue(6_000), "the link going idle excuses nothing");
    }

    #[test]
    fn places_are_limited_and_reused() {
        static L: Liveness = Liveness::new();
        let all: Vec<Lease> = (0..MAX_TASKS).map(|_| L.register().unwrap()).collect();
        assert!(L.register().is_none());
        drop(all);
        assert!(L.register().is_some());
    }
}
