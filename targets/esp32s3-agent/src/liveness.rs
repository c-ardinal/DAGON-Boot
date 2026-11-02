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
//! the link task, which runs above the others and blocks them while it works
//! (a slot erase takes up to about 30 s). While it is busy only its own
//! deadline counts, and when it goes idle every other busy deadline moves
//! later by the time it held the CPU, so starved tasks are not blamed.

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

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
    /// When the preempting task last went busy (ms, wrapping).
    preempt_since: AtomicU32,
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
            preempt_since: AtomicU32::new(0),
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
    /// preempting task is busy, only its deadline is checked.
    pub fn overdue(&self, now_ms: u32) -> bool {
        let late = |i: usize| {
            self.state[i].load(Ordering::Acquire) == BUSY
                && now_ms.wrapping_sub(self.deadline[i].load(Ordering::Acquire)) as i32 >= 0
        };
        match self.preempting_busy() {
            Some(p) => late(p),
            None => (0..MAX_TASKS).any(late),
        }
    }

    fn preempting_busy(&self) -> Option<usize> {
        let p = self.preempting.load(Ordering::Acquire);
        (p != NONE && self.state[p as usize].load(Ordering::Acquire) == BUSY).then_some(p as usize)
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
        if self.is_preempting() && o.state[self.slot].load(Ordering::Acquire) != BUSY {
            o.preempt_since.store(now_ms, Ordering::Release);
        }
        o.deadline[self.slot].store(now_ms.wrapping_add(budget_ms), Ordering::Release);
        o.state[self.slot].store(BUSY, Ordering::Release);
    }

    /// Back to waiting for an event at `now_ms`: no deadline. For the
    /// preempting task, the other busy deadlines move later by the time it
    /// was busy.
    pub fn idle(&self, now_ms: u32) {
        let o = self.owner;
        let was_busy = o.state[self.slot].swap(IDLE, Ordering::AcqRel) == BUSY;
        if !(was_busy && self.is_preempting()) {
            return;
        }
        let held = now_ms.wrapping_sub(o.preempt_since.load(Ordering::Acquire));
        for i in (0..MAX_TASKS).filter(|&i| i != self.slot) {
            if o.state[i].load(Ordering::Acquire) == BUSY {
                o.deadline[i].fetch_add(held, Ordering::AcqRel);
            }
        }
    }

    fn is_preempting(&self) -> bool {
        self.owner.preempting.load(Ordering::Acquire) as usize == self.slot
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.is_preempting() {
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
        a.idle(1_600);
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
    fn a_long_preempting_task_does_not_starve_the_others() {
        static L: Liveness = Liveness::new();
        let link = L.register_preempting().unwrap();
        let demo = L.register().unwrap();
        assert!(L.register_preempting().is_none());
        // Demo starts 500 ms of work, then the link erases for 5 s and the
        // demo cannot run meanwhile.
        demo.busy(1_000, 500);
        link.busy(1_100, 30_000);
        assert!(!L.overdue(1_600));
        assert!(!L.overdue(6_000));
        link.idle(6_100);
        // The demo's deadline moved by the 5 s the link held the CPU.
        assert!(!L.overdue(6_100));
        assert!(!L.overdue(6_499));
        // Genuinely stuck after the link went idle: overdue, feeding stops.
        assert!(L.overdue(6_500));
        // A stuck link is caught on its own deadline.
        demo.idle(6_500);
        link.busy(7_000, 1_000);
        assert!(L.overdue(8_000));
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
