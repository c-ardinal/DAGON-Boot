//! Trial watchdog: an app on trial feeds the RTC WDT only for a bounded
//! window, so an image that never gets confirmed is reset and the 2nd stage
//! rolls it back.

use dagon_port::{Clock, Watchdog};

/// Self-feed window of an app on trial. A CONFIRM must arrive within it;
/// after it the WDT runs out and the 2nd stage falls back. Window, WDT and
/// two boots stay inside the PC's 30 s wait for the rolled-back HELLO.
pub const TRIAL_FEED_MS: u32 = 15_000;

pub struct TrialWatchdog<W, C> {
    wdt: W,
    clock: C,
    /// Feeding stops at this time (µs); `None` once confirmed.
    deadline_us: Option<u64>,
}

impl<W: Watchdog, C: Clock> TrialWatchdog<W, C> {
    pub fn new(wdt: W, clock: C) -> Self {
        Self {
            wdt,
            clock,
            deadline_us: None,
        }
    }

    /// Starts the self-feed window, `window_ms` from now.
    pub fn begin_trial(&mut self, window_ms: u32) {
        self.deadline_us = Some(self.clock.now_us() + u64::from(window_ms) * 1000);
    }

    /// Confirmed: feed for good.
    pub fn end_trial(&mut self) {
        self.deadline_us = None;
    }

    pub fn on_trial(&self) -> bool {
        self.deadline_us.is_some()
    }
}

impl<W: Watchdog, C: Clock> Watchdog for TrialWatchdog<W, C> {
    fn start(&mut self, timeout_ms: u32) {
        self.wdt.start(timeout_ms);
    }

    fn feed(&mut self) {
        if self.deadline_us.is_none_or(|d| self.clock.now_us() < d) {
            self.wdt.feed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Count(Rc<Cell<u32>>);
    impl Watchdog for Count {
        fn start(&mut self, _: u32) {}
        fn feed(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    struct Fake(Rc<Cell<u64>>);
    impl Clock for Fake {
        fn now_us(&self) -> u64 {
            self.0.get()
        }
        fn delay_us(&mut self, _: u32) {}
    }

    #[test]
    fn feeds_until_the_window_ends_unless_confirmed() {
        let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut w = TrialWatchdog::new(Count(fed.clone()), Fake(now.clone()));
        w.feed();
        w.begin_trial(1000);
        now.set(999_999);
        w.feed();
        now.set(1_000_000);
        w.feed();
        assert_eq!(fed.get(), 2);
        w.end_trial();
        w.feed();
        assert_eq!(fed.get(), 3);
    }
}
