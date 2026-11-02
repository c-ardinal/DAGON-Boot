//! Liveness of the node's tasks: the watchdog is fed only while no task has
//! overrun the deadline it announced.
//!
//! A task takes a [`Lease`], calls [`Lease::busy`] with a time budget before
//! each piece of work and [`Lease::idle`] when it goes back to waiting for an
//! event. A task stuck in its work misses the deadline, the feeding stops and
//! the hardware watchdog resets the chip instead of leaving it silently hung.
//! Lock-free (atomics only), so any task or interrupt priority may use it.

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Tasks that can register.
pub const MAX_TASKS: usize = 4;

const FREE: u8 = 0;
const IDLE: u8 = 1;
const BUSY: u8 = 2;

pub struct Liveness {
    state: [AtomicU8; MAX_TASKS],
    /// Milliseconds since boot (wrapping) at which a busy task is overdue.
    deadline: [AtomicU32; MAX_TASKS],
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

    /// Whether a busy task is past its deadline at `now_ms`.
    pub fn overdue(&self, now_ms: u32) -> bool {
        self.state.iter().zip(&self.deadline).any(|(s, d)| {
            s.load(Ordering::Acquire) == BUSY
                && now_ms.wrapping_sub(d.load(Ordering::Acquire)) as i32 >= 0
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
        let d = &self.owner.deadline[self.slot];
        d.store(now_ms.wrapping_add(budget_ms), Ordering::Release);
        self.owner.state[self.slot].store(BUSY, Ordering::Release);
    }

    /// Back to waiting for an event: no deadline.
    pub fn idle(&self) {
        self.owner.state[self.slot].store(IDLE, Ordering::Release);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
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
    fn places_are_limited_and_reused() {
        static L: Liveness = Liveness::new();
        let all: Vec<Lease> = (0..MAX_TASKS).map(|_| L.register().unwrap()).collect();
        assert!(L.register().is_none());
        drop(all);
        assert!(L.register().is_some());
    }
}
