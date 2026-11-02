//! Watchdog liveness accounting for apps whose tasks run at two levels: a
//! preempting level (an interrupt executor) and a background level (thread
//! mode). Board-independent and pure: times are milliseconds since boot.
//!
//! Each task, while working, holds a deadline. The watchdog may be fed only
//! while no deadline has passed. Background tasks cannot run while a
//! preempting task works, so their deadlines are frozen meanwhile: while
//! any preempting task is busy only preempting deadlines are judged, and
//! when the last one goes idle every background deadline moves on by the
//! time the preempting level was busy.

/// Priority level of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Runs on the interrupt executor and preempts background tasks.
    Preempting,
    /// Runs in thread mode.
    Background,
}

#[derive(Clone, Copy, Debug)]
pub struct Liveness<const N: usize> {
    levels: [Level; N],
    deadline: [Option<u64>; N],
    /// Since when the preempting level has had a busy task.
    preempting_since: Option<u64>,
}

impl<const N: usize> Liveness<N> {
    pub const fn new(levels: [Level; N]) -> Self {
        Liveness {
            levels,
            deadline: [None; N],
            preempting_since: None,
        }
    }

    fn preempting_busy(&self) -> bool {
        (0..N).any(|i| self.levels[i] == Level::Preempting && self.deadline[i].is_some())
    }

    /// Task `t` starts (or renews) a piece of work that must end within
    /// `bound_ms`.
    pub fn busy(&mut self, t: usize, now: u64, bound_ms: u64) {
        if self.levels[t] == Level::Preempting && self.preempting_since.is_none() {
            self.preempting_since = Some(now);
        }
        self.deadline[t] = Some(now + bound_ms);
    }

    /// Task `t` waits for its next piece of work.
    pub fn idle(&mut self, t: usize, now: u64) {
        self.deadline[t] = None;
        if self.levels[t] != Level::Preempting || self.preempting_busy() {
            return;
        }
        if let Some(since) = self.preempting_since.take() {
            let frozen = now.saturating_sub(since);
            for i in 0..N {
                if self.levels[i] == Level::Background {
                    if let Some(d) = self.deadline[i].as_mut() {
                        *d += frozen;
                    }
                }
            }
        }
    }

    /// True while no judged deadline has passed.
    pub fn alive(&self, now: u64) -> bool {
        let only_preempting = self.preempting_busy();
        (0..N)
            .filter(|&i| !only_preempting || self.levels[i] == Level::Preempting)
            .all(|i| self.deadline[i].is_none_or(|d| now < d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PC: usize = 0;
    const LCD: usize = 1;

    fn hub() -> Liveness<2> {
        Liveness::new([Level::Preempting, Level::Background])
    }

    #[test]
    fn long_preempting_work_does_not_expire_background_deadlines() {
        let mut l = hub();
        l.busy(LCD, 0, 5_000);
        // An 11 s erase on the preempting level, renewing its own lease per
        // sector; the LCD cannot run meanwhile.
        for t in (100..11_100).step_by(50) {
            l.busy(PC, t, 5_000);
            assert!(l.alive(t), "at {t} ms");
        }
        l.idle(PC, 11_100);
        assert!(l.alive(11_100));
        // The LCD got its 5 s back, minus the 100 ms it had used.
        assert!(l.alive(15_999));
        // Genuinely stuck afterwards: feeding stops.
        assert!(!l.alive(16_100));
    }

    #[test]
    fn stuck_preempting_task_stops_feeding() {
        let mut l = hub();
        l.busy(PC, 0, 1_000);
        assert!(l.alive(999));
        assert!(!l.alive(1_000));
    }

    #[test]
    fn stuck_background_task_stops_feeding() {
        let mut l = hub();
        l.busy(LCD, 0, 5_000);
        assert!(l.alive(4_999));
        assert!(!l.alive(5_000));
        l.idle(LCD, 6_000);
        assert!(l.alive(6_000));
    }
}
