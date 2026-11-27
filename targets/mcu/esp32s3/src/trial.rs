//! Trial watchdog: an app on trial never feeds or restarts the RTC WDT, so an
//! image that is not confirmed within the trial period (`[watchdog]
//! trial_ms`, armed at start) is reset and the 2nd stage rolls it back (one
//! attempt). Until the trial status is known it does not feed either. Once
//! confirmed it restarts the watchdog with the confirmed period (`[watchdog]
//! confirmed_ms`) and feeds, and with a [`Liveness`] registry attached only
//! while every task is within the deadline it announced.

use dagon_events::Clock;
use dagon_port::Watchdog;

use crate::liveness::{Liveness, Renewal};

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
    /// The period a confirmed image runs, ms.
    confirmed_ms: u32,
}

impl<W: Watchdog, C: Clock> TrialWatchdog<W, C> {
    /// `confirmed_ms`: the period a confirmed image runs.
    pub fn new(wdt: W, clock: C, confirmed_ms: u32) -> Self {
        Self {
            wdt,
            clock,
            status: Status::Unknown,
            liveness: None,
            confirmed_ms,
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
    pub fn now_ms(&mut self) -> u32 {
        (self.clock.now_us() / 1000) as u32
    }

    /// One planned erase of at most `max_ms` starts inside the request
    /// `lease`'s task serves: its deadline moves to the end of that erase plus
    /// `after_ms` (what the rest of the request may take), then a feed
    /// through the gate. A request's own budget cannot hold a slot's whole
    /// erase at the flash part's worst case (`unit_max_ms` a sector, 3 MiB
    /// of them), so each erase extends it; a request that stops making
    /// progress is still overdue `after_ms` after its last erase.
    pub fn erasing(&mut self, lease: Renewal, max_ms: u32, after_ms: u32) {
        lease.extend(self.now_ms(), max_ms.saturating_add(after_ms));
        self.feed();
    }

    /// Running on trial: no feeding until [`TrialWatchdog::end_trial`].
    pub fn begin_trial(&mut self) {
        self.status = Status::Trial;
    }

    /// Confirmed (or found confirmed at start): the confirmed period from
    /// now on, then feed, subject to liveness.
    pub fn end_trial(&mut self) {
        if self.status != Status::Confirmed {
            self.status = Status::Confirmed;
            self.wdt.start(self.confirmed_ms);
        }
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
        fn now_us(&mut self) -> u64 {
            self.0.get()
        }
    }

    #[test]
    fn no_feeding_on_trial_until_confirmed() {
        let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut w = TrialWatchdog::new(Count(fed.clone()), Fake(now.clone()), 4000);
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
        assert_eq!(fed.get(), 100, "restarted with the confirmed period");
        w.feed();
        assert_eq!(fed.get(), 101);
    }

    struct Starts(Rc<std::cell::RefCell<Vec<u32>>>);
    impl Watchdog for Starts {
        fn start(&mut self, ms: u32) {
            self.0.borrow_mut().push(ms);
        }
        fn feed(&mut self) {}
    }

    /// The trial period runs until the image is confirmed; from then on the
    /// confirmed period, once, whatever the two are.
    #[test]
    fn a_confirmed_image_runs_the_confirmed_period() {
        let starts = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut w = TrialWatchdog::new(Starts(starts.clone()), Fake(Rc::new(Cell::new(0))), 9_000);
        w.begin_trial();
        assert!(
            starts.borrow().is_empty(),
            "the trial period is the one armed at start"
        );
        w.end_trial();
        w.end_trial();
        assert_eq!(*starts.borrow(), [9_000]);
        // Found confirmed at start: straight to the confirmed period.
        let starts = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut w = TrialWatchdog::new(Starts(starts.clone()), Fake(Rc::new(Cell::new(0))), 7_000);
        w.end_trial();
        assert_eq!(*starts.borrow(), [7_000]);
    }

    #[test]
    fn an_overdue_task_stops_the_feeding() {
        static L: Liveness = Liveness::new();
        let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut w =
            TrialWatchdog::new(Count(fed.clone()), Fake(now.clone()), 4000).with_liveness(&L);
        w.end_trial();
        let task = L.register().unwrap();
        task.busy(0, 100);
        now.set(99_000);
        w.feed();
        now.set(100_000);
        w.feed();
        assert_eq!(fed.get(), 101, "overdue at 100 ms");
        task.idle();
        w.feed();
        assert_eq!(fed.get(), 102);
    }

    /// A 3 MiB slot of 4 KiB sectors, each taking the W25Q128JV's worst
    /// 400 ms, is 307 s of erasing: over the 120 s one request has. Fed
    /// only against the request's own deadline the watchdog would reset the
    /// chip mid-erase; each planned erase extending the deadline keeps it
    /// fed, and a request that then hangs is overdue one budget later.
    #[test]
    fn a_slot_erase_at_the_parts_worst_case_stays_fed() {
        const SECTORS: u32 = 769;
        const SECTOR_MS: u32 = 400;
        const BUDGET_MS: u32 = 120_000;
        const { assert!(SECTORS * SECTOR_MS > BUDGET_MS) };
        static PLAIN: Liveness = Liveness::new();
        static EXTENDED: Liveness = Liveness::new();
        let run = |l: &'static Liveness, extend: bool| {
            let (fed, now) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
            let mut w =
                TrialWatchdog::new(Count(fed.clone()), Fake(now.clone()), 4000).with_liveness(l);
            w.end_trial();
            let lease = l.register_preempting().unwrap();
            lease.busy(0, BUDGET_MS);
            for _ in 0..SECTORS {
                if extend {
                    w.erasing(lease.renewal(), SECTOR_MS, BUDGET_MS);
                } else {
                    w.feed();
                }
                now.set(now.get() + u64::from(SECTOR_MS) * 1000);
            }
            (fed.get() - 100, lease, now)
        };
        let (fed, ..) = run(&PLAIN, false);
        assert_eq!(
            fed,
            BUDGET_MS / SECTOR_MS,
            "fed past the request's deadline"
        );
        let (fed, lease, now) = run(&EXTENDED, true);
        assert_eq!(fed, SECTORS, "an erase went unfed");
        // The request hangs after its last erase: overdue one budget later.
        let end = now.get() / 1000;
        assert!(!EXTENDED.overdue(end as u32 + BUDGET_MS - 1));
        assert!(EXTENDED.overdue(end as u32 + BUDGET_MS));
        // Outside a request an erase's word changes nothing.
        lease.idle();
        lease.renewal().extend(0, 1);
        assert!(!EXTENDED.overdue(u32::MAX / 2));
    }
}
