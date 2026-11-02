//! Trial watchdog: an app on trial never feeds or restarts the RTC WDT, so an
//! image that is not confirmed within the WDT timeout (4 s from start) is
//! reset and the 2nd stage rolls it back (one attempt). Until the trial
//! status is known it does not feed either. Once confirmed it feeds, and with
//! a [`Liveness`] registry attached only while every task is within the
//! deadline it announced.

use dagon_port::{Clock, Watchdog};

use crate::liveness::Liveness;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Not yet read from otadata: treated like a trial.
    Unknown,
    /// Running unconfirmed: no feeding, no restart.
    Trial,
    Confirmed,
}

pub struct TrialWatchdog<W, C> {
    wdt: W,
    clock: C,
    status: Status,
    liveness: Option<&'static Liveness>,
}

impl<W: Watchdog, C: Clock> TrialWatchdog<W, C> {
    pub fn new(wdt: W, clock: C) -> Self {
        Self {
            wdt,
            clock,
            status: Status::Unknown,
            liveness: None,
        }
    }

    /// Feeds only while no task registered in `liveness` is overdue.
    pub fn with_liveness(self, liveness: &'static Liveness) -> Self {
        Self {
            liveness: Some(liveness),
            ..self
        }
    }

    /// Milliseconds since boot, wrapping (the [`Liveness`] time base).
    pub fn now_ms(&self) -> u32 {
        (self.clock.now_us() / 1000) as u32
    }

    /// Running on trial: no feeding until [`TrialWatchdog::end_trial`].
    pub fn begin_trial(&mut self) {
        self.status = Status::Trial;
    }

    /// Confirmed (or found confirmed at start): feed, subject to liveness.
    pub fn end_trial(&mut self) {
        self.status = Status::Confirmed;
    }

    pub fn on_trial(&self) -> bool {
        self.status == Status::Trial
    }

    /// The wrapped watchdog (to stop it before leaving the app).
    pub fn inner(&mut self) -> &mut W {
        &mut self.wdt
    }
}

impl<W: Watchdog, C: Clock> Watchdog for TrialWatchdog<W, C> {
    /// Only once confirmed: a start feeds the watchdog.
    fn start(&mut self, timeout_ms: u32) {
        if self.status == Status::Confirmed {
            self.wdt.start(timeout_ms);
        }
    }

    fn feed(&mut self) {
        if self.status != Status::Confirmed {
            return;
        }
        let now_ms = self.now_ms();
        if self.liveness.is_none_or(|l| !l.overdue(now_ms)) {
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
        fn start(&mut self, _: u32) {
            self.0.set(self.0.get() + 100);
        }
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
    fn no_feeding_on_trial_until_confirmed() {
        let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut w = TrialWatchdog::new(Count(fed.clone()), Fake(now.clone()));
        // Status not read yet: neither feed nor restart.
        w.feed();
        w.start(4000);
        assert_eq!(fed.get(), 0, "fed before the trial status was known");
        w.begin_trial();
        w.start(4000);
        for t in [0, 1_000, 3_999_000] {
            now.set(t);
            w.feed();
        }
        assert_eq!(fed.get(), 0, "fed or restarted on trial");
        w.end_trial();
        w.feed();
        assert_eq!(fed.get(), 1);
    }

    #[test]
    fn an_overdue_task_stops_the_feeding() {
        static L: Liveness = Liveness::new();
        let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut w = TrialWatchdog::new(Count(fed.clone()), Fake(now.clone())).with_liveness(&L);
        w.end_trial();
        let task = L.register().unwrap();
        task.busy(0, 100);
        now.set(99_000);
        w.feed();
        now.set(100_000);
        w.feed();
        assert_eq!(fed.get(), 1, "overdue at 100 ms");
        task.idle();
        w.feed();
        assert_eq!(fed.get(), 2);
    }
}
