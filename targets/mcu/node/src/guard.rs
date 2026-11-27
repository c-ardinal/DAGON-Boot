//! Liveness-gated watchdog guard. The app declares one `static` [`Guard`]
//! with the [`Level`] of each of its tasks; each task reports
//! [`Guard::busy`] when it starts a piece of work and [`Guard::idle`] when
//! it waits for the next one. The watchdog is fed only through
//! [`Guard::feed`], which feeds only while every task is within the
//! deadline of its current work (see [`dagon_mcu_common::liveness`]: background
//! deadlines are frozen while a preempting task works). A stuck task
//! therefore ends in a reset instead of a silent hang.
//!
//! On trial (an unconfirmed image) the [`Trial`] policy decides: keep
//! feeding for a while after boot, or never feed. Either way an image that
//! is never confirmed resets and the boot core counts the attempt.

use core::cell::RefCell;

use critical_section::Mutex;

use dagon_mcu_common::liveness::{Level, Liveness};
use dagon_port::{Reset, Watchdog};

/// Longest a task may stay inside one piece of work it did not bound.
pub const BUSY_LIMIT_MS: u64 = 5_000;
/// Slack added to a declared bound ([`Guard::busy_for`]).
pub const BUSY_MARGIN_MS: u64 = 2_000;

/// Levels for a guard whose first `preempting` tasks run on DAGON's
/// high-priority executor and the rest in thread mode.
pub const fn levels<const N: usize>(preempting: usize) -> [Level; N] {
    let mut l = [Level::Background; N];
    let mut i = 0;
    while i < preempting && i < N {
        l[i] = Level::Preempting;
        i += 1;
    }
    l
}

/// The MCU's watchdog peripheral, as the guard drives it.
pub trait Dog {
    /// Restart the watchdog countdown.
    fn feed(&mut self);
    /// Reset the chip now.
    fn reset(&mut self);
    /// The running image is (no longer) on trial: a watchdog whose period
    /// differs between a trial and a confirmed image runs the one for
    /// `on_trial` from the next feed on.
    fn trial(&mut self, _on_trial: bool) {}
}

/// A [`Dog`] over the MCU's [`Watchdog`] and [`Reset`] ports: it runs the
/// trial period while the image is on trial and the confirmed period once
/// it is confirmed (the MCU definition's `[watchdog] trial_ms` and
/// `confirmed_ms`). The watchdog is started with the period at the first
/// feed and when the period changes (a start on a running watchdog
/// restarts the countdown with the new period).
pub struct PeriodDog<H> {
    pub hw: H,
    trial_ms: u32,
    confirmed_ms: u32,
    on_trial: bool,
    running_ms: Option<u32>,
}

impl<H> PeriodDog<H> {
    pub const fn new(hw: H, trial_ms: u32, confirmed_ms: u32) -> Self {
        PeriodDog {
            hw,
            trial_ms,
            confirmed_ms,
            on_trial: true,
            running_ms: None,
        }
    }
}

impl<H: Watchdog + Reset> Dog for PeriodDog<H> {
    fn feed(&mut self) {
        let ms = if self.on_trial {
            self.trial_ms
        } else {
            self.confirmed_ms
        };
        if self.running_ms != Some(ms) {
            self.hw.start(ms);
            self.running_ms = Some(ms);
        }
        self.hw.feed();
    }

    fn reset(&mut self) {
        self.hw.reset();
    }

    fn trial(&mut self, on_trial: bool) {
        self.on_trial = on_trial;
    }
}

/// What an image on trial does with the watchdog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trial {
    /// Feed (gated by liveness) until this many ms after boot, then stop.
    FeedUntilMs(u64),
    /// Never feed while on trial.
    NeverFeed,
}

/// The guard as DAGON's own tasks and drivers use it, without knowing the
/// app's task count or the watchdog type.
pub trait Watch: Sync {
    /// See [`Guard::busy`].
    fn busy(&self, task: usize);
    /// See [`Guard::busy_for`].
    fn busy_for(&self, task: usize, bound_ms: u64);
    /// See [`Guard::idle`].
    fn idle(&self, task: usize);
    /// See [`Guard::feed`].
    fn feed(&self);
    /// `task` made progress inside long work (an erase of many sectors):
    /// renew its deadline and feed through the gate.
    fn progress(&self, task: usize) {
        self.busy(task);
        self.feed();
    }
    /// The running image is (no longer) on trial.
    fn set_trial(&self, on_trial: bool);
    /// Resets the chip through the watchdog.
    fn reboot(&self) -> !;
}

/// One task's handle on the guard. A task announces each piece of work
/// with [`Lease::busy`] and waits with [`Lease::idle`]: if it stays busy
/// past its budget, the watchdog stops being fed and resets the chip (an
/// image on trial then rolls back).
#[derive(Clone, Copy)]
pub struct Lease {
    pub watch: &'static dyn Watch,
    pub task: usize,
}

impl Lease {
    /// A piece of work starts that ends within `budget_ms` (plus
    /// [`BUSY_MARGIN_MS`] slack).
    pub fn busy(&self, budget_ms: u64) {
        self.watch.busy_for(self.task, budget_ms);
    }

    /// The task waits for something with no deadline of its own.
    pub fn idle(&self) {
        self.watch.idle(self.task);
    }
}

struct State<D, const N: usize> {
    dog: Option<D>,
    live: Liveness<N>,
    on_trial: bool,
    /// Microseconds since boot, timing each section ([`Guard::time_masking`]).
    #[cfg(any(test, feature = "masked-time"))]
    clock: Option<fn() -> u64>,
}

/// Watchdog guard for `N` tasks (indexes `0..N`).
pub struct Guard<D, const N: usize> {
    state: Mutex<RefCell<State<D, N>>>,
    trial: Trial,
    now_ms: fn() -> u64,
}

impl<D: Dog, const N: usize> Guard<D, N> {
    /// `levels[i]`: whether task `i` runs on the preempting executor;
    /// `now_ms`: milliseconds since boot.
    pub const fn new(levels: [Level; N], trial: Trial, now_ms: fn() -> u64) -> Self {
        Guard {
            state: Mutex::new(RefCell::new(State {
                dog: None,
                live: Liveness::new(levels),
                on_trial: true,
                #[cfg(any(test, feature = "masked-time"))]
                clock: None,
            })),
            trial,
            now_ms,
        }
    }

    /// What the guard does while the image is on trial, as
    /// `app.watchdog_taken` reports it.
    pub const fn trial_policy(&self) -> dagon_agent::TrialPolicy {
        match self.trial {
            Trial::NeverFeed => dagon_agent::TrialPolicy::NeverFeed,
            Trial::FeedUntilMs(ms) => {
                dagon_agent::TrialPolicy::FeedUntilMs(if ms > u32::MAX as u64 {
                    u32::MAX
                } else {
                    ms as u32
                })
            }
        }
    }

    /// Takes the running watchdog (started by the boot core or the app).
    pub fn install(&self, dog: D, on_trial: bool) {
        self.with_state(|s| {
            let mut dog = dog;
            dog.trial(on_trial);
            s.dog = Some(dog);
            s.on_trial = on_trial;
        });
    }

    /// The running image is (no longer) on trial.
    pub fn set_trial(&self, on_trial: bool) {
        self.with_state(|s| {
            s.on_trial = on_trial;
            if let Some(d) = s.dog.as_mut() {
                d.trial(on_trial);
            }
        });
    }

    /// `task` starts (or makes progress in) a piece of work.
    pub fn busy(&self, task: usize) {
        self.busy_ms(task, BUSY_LIMIT_MS);
    }

    /// `task` starts a piece of work bounded by `bound_ms` (a bus request
    /// with its own timeout); the deadline adds [`BUSY_MARGIN_MS`].
    pub fn busy_for(&self, task: usize, bound_ms: u64) {
        self.busy_ms(task, bound_ms + BUSY_MARGIN_MS);
    }

    fn busy_ms(&self, task: usize, ms: u64) {
        self.with_state(|s| s.live.busy(task, (self.now_ms)(), ms));
    }

    /// `task` waits for its next piece of work.
    pub fn idle(&self, task: usize) {
        self.with_state(|s| s.live.idle(task, (self.now_ms)()));
    }

    /// Feeds if every task is alive and the trial policy allows it. The
    /// only way the watchdog is fed.
    pub fn feed(&self) {
        self.with_state(|s| {
            let now = (self.now_ms)();
            let allowed = !s.on_trial
                || match self.trial {
                    Trial::FeedUntilMs(end) => now < end,
                    Trial::NeverFeed => false,
                };
            if allowed && s.live.alive(now) {
                if let Some(d) = s.dog.as_mut() {
                    d.feed();
                }
            }
        });
    }

    /// A [`Lease`] for `task` of this (static) guard.
    pub fn lease(&'static self, task: usize) -> Lease
    where
        D: Send,
    {
        Lease { watch: self, task }
    }

    /// Runs `f` on the watchdog (if installed), e.g. to force a reset.
    pub fn with_dog<R>(&self, f: impl FnOnce(&mut D) -> R) -> Option<R> {
        self.with_state(|s| s.dog.as_mut().map(f))
    }

    /// Times each critical section of the guard from now on with `now_us`,
    /// microseconds since boot, keeping the longest
    /// ([`crate::masked_time`]): what an app built with `masked-time` does.
    #[cfg(any(test, feature = "masked-time"))]
    pub fn time_masking(&self, now_us: fn() -> u64) {
        self.with_state(|s| s.clock = Some(now_us));
    }

    /// The guard's state, shared by DAGON's tasks and the app's. The one
    /// place the MCU packages' own code masks interrupts (the published
    /// events' ring is the other reviewed one): the closures above
    /// read the clock (inside the section, so that no preempting task's
    /// work falls between a reading and the update it stamps), update a
    /// few fields or feed the watchdog (a register write), well inside
    /// `[execution] masked_max_us`; none reaches the flash. An app built
    /// with `masked-time` times each ([`crate::masked_time`]).
    #[allow(clippy::disallowed_methods)]
    fn with_state<R>(&self, f: impl FnOnce(&mut State<D, N>) -> R) -> R {
        critical_section::with(|cs| {
            let mut s = self.state.borrow_ref_mut(cs);
            #[cfg(any(test, feature = "masked-time"))]
            if let Some(now) = s.clock {
                let started = now();
                let r = f(&mut s);
                crate::masked_time::note(now().saturating_sub(started));
                return r;
            }
            f(&mut s)
        })
    }
}

impl<D: Dog + Send, const N: usize> Watch for Guard<D, N> {
    fn busy(&self, task: usize) {
        Guard::busy(self, task);
    }
    fn busy_for(&self, task: usize, bound_ms: u64) {
        Guard::busy_for(self, task, bound_ms);
    }
    fn idle(&self, task: usize) {
        Guard::idle(self, task);
    }
    fn feed(&self) {
        Guard::feed(self);
    }
    fn set_trial(&self, on_trial: bool) {
        Guard::set_trial(self, on_trial);
    }
    fn reboot(&self) -> ! {
        self.with_dog(|d| d.reset());
        #[allow(clippy::empty_loop)]
        loop {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU64, Ordering};

    static NOW: AtomicU64 = AtomicU64::new(0);
    fn now() -> u64 {
        NOW.load(Ordering::Relaxed)
    }

    #[derive(Default)]
    struct Count(u32);
    impl Dog for Count {
        fn feed(&mut self) {
            self.0 += 1;
        }
        fn reset(&mut self) {}
    }

    fn fed<const N: usize>(g: &Guard<Count, N>) -> u32 {
        g.with_dog(|d| d.0).unwrap()
    }

    #[test]
    fn trial_policies_and_liveness_gate_feeding() {
        NOW.store(0, Ordering::Relaxed);
        let hub = Guard::new([Level::Preempting], Trial::FeedUntilMs(60_000), now);
        hub.install(Count::default(), true);
        hub.feed();
        assert_eq!(fed(&hub), 1, "trial window open");
        hub.busy(0);
        NOW.store(BUSY_LIMIT_MS + 1, Ordering::Relaxed);
        hub.feed();
        assert_eq!(fed(&hub), 1, "task past its deadline");
        hub.idle(0);
        NOW.store(60_000, Ordering::Relaxed);
        hub.feed();
        assert_eq!(fed(&hub), 1, "trial window closed");
        hub.set_trial(false);
        hub.feed();
        assert_eq!(fed(&hub), 2, "confirmed");

        let node = Guard::new([Level::Preempting], Trial::NeverFeed, now);
        node.install(Count::default(), true);
        node.feed();
        assert_eq!(fed(&node), 0, "never on trial");
        node.progress(0);
        assert_eq!(fed(&node), 0);
        node.set_trial(false);
        node.progress(0);
        assert_eq!(fed(&node), 1);
    }

    /// The watchdog learns whether the image is on trial, at install and on
    /// every change, so it runs the trial or the confirmed period.
    #[test]
    fn the_watchdog_is_told_the_trial_state() {
        extern crate std;
        use std::vec::Vec;
        #[derive(Default)]
        struct Periods(Vec<bool>);
        impl Dog for Periods {
            fn feed(&mut self) {}
            fn reset(&mut self) {}
            fn trial(&mut self, on_trial: bool) {
                self.0.push(on_trial);
            }
        }
        let g = Guard::new([Level::Preempting], Trial::NeverFeed, now);
        g.install(Periods::default(), true);
        g.set_trial(false);
        assert_eq!(g.with_dog(|d| d.0.clone()).unwrap(), [true, false]);
        let g = Guard::new([Level::Preempting], Trial::NeverFeed, now);
        g.install(Periods::default(), false);
        assert_eq!(g.with_dog(|d| d.0.clone()).unwrap(), [false]);
    }

    /// A watchdog with a period runs the trial period while the image is
    /// on trial and the confirmed one from its confirmation on, without a
    /// restart; its period is set only when it changes, not on every feed.
    #[test]
    fn a_period_dog_switches_to_the_confirmed_period_on_confirmation() {
        extern crate std;
        use std::vec::Vec;
        #[derive(Debug, PartialEq)]
        enum Hw {
            Set(u32),
            Feed,
        }
        #[derive(Default)]
        struct Log(Vec<Hw>);
        impl Watchdog for Log {
            fn start(&mut self, timeout_ms: u32) {
                self.0.push(Hw::Set(timeout_ms));
            }
            fn feed(&mut self) {
                self.0.push(Hw::Feed);
            }
        }
        impl Reset for Log {
            fn reset(&mut self) -> ! {
                unreachable!("the test never resets")
            }
        }
        // Its own clock: the shared one is moved by other tests.
        let g = Guard::new([Level::Preempting], Trial::FeedUntilMs(60_000), || 0);
        g.install(PeriodDog::new(Log::default(), 4_000, 9_000), true);
        g.feed();
        g.feed();
        g.set_trial(false);
        g.feed();
        g.feed();
        use Hw::{Feed, Set};
        assert_eq!(
            g.with_dog(|d| d.hw.0.drain(..).collect::<Vec<_>>())
                .unwrap(),
            [Set(4_000), Feed, Feed, Set(9_000), Feed, Feed]
        );
        // Found confirmed at start: the confirmed period from the first feed.
        let g = Guard::new([Level::Preempting], Trial::NeverFeed, || 0);
        g.install(PeriodDog::new(Log::default(), 4_000, 9_000), false);
        g.feed();
        g.feed();
        assert_eq!(
            g.with_dog(|d| d.hw.0.drain(..).collect::<Vec<_>>())
                .unwrap(),
            [Set(9_000), Feed, Feed]
        );
    }

    /// A task's clock reading and the update it stamps are one critical
    /// section. Were the clock read before it, a preempting task that
    /// worked in between would leave the reading stale: a background
    /// task's deadline would start that much in the past, and a feed
    /// would be withheld from a task still inside its limit.
    #[test]
    fn every_operation_reads_the_clock_inside_its_critical_section() {
        extern crate std;
        use core::sync::atomic::AtomicU32;
        use std::sync::mpsc;
        use std::time::Duration;
        // The hosted critical section is one lock: while the guard holds
        // it, another thread's call into the guard does not return.
        static G: Guard<Count, 2> = Guard::new(
            [Level::Preempting, Level::Background],
            Trial::NeverFeed,
            || {
                let (done, wait) = mpsc::channel();
                std::thread::spawn(move || {
                    G.with_dog(|_| ());
                    let _ = done.send(());
                });
                if wait.recv_timeout(Duration::from_millis(100)).is_ok() {
                    OPEN.fetch_add(1, Ordering::Relaxed);
                } else {
                    MASKED.fetch_add(1, Ordering::Relaxed);
                }
                0
            },
        );
        static MASKED: AtomicU32 = AtomicU32::new(0);
        static OPEN: AtomicU32 = AtomicU32::new(0);
        G.install(Count::default(), false);
        G.busy(1);
        G.busy_for(1, 10);
        G.progress(1);
        G.idle(1);
        G.feed();
        assert_eq!(OPEN.load(Ordering::Relaxed), 0, "a clock reading a preemption can make stale");
        assert!(MASKED.load(Ordering::Relaxed) >= 5);
    }

    /// Every way a preempting task's requests fall into a background
    /// task's work, by real time: a background task that reports within
    /// its limit of its own running time is never the reason a feed is
    /// withheld, however long the preempting work between its reports;
    /// one that stops reporting stops the feeding once its limit of its
    /// own running time has passed.
    #[test]
    fn a_background_task_is_judged_by_its_own_running_time_in_every_schedule() {
        extern crate std;
        use std::cell::Cell;
        use std::vec::Vec;
        std::thread_local! {
            static T: Cell<u64> = const { Cell::new(0) };
        }
        const PC: usize = 0;
        const LCD: usize = 1;
        // One step of the schedule: the display works for `Lcd(ms)` and
        // reports, or a request preempts it for `Pc(ms)` (the serving
        // task renews its own deadline in pieces within its limit).
        #[derive(Clone, Copy, Debug)]
        enum Step {
            Lcd(u64),
            Pc(u64),
        }
        let lcd = [1, BUSY_LIMIT_MS / 2, BUSY_LIMIT_MS - 1];
        let pc = [0, 1, BUSY_LIMIT_MS - 1, 3 * BUSY_LIMIT_MS];
        let mut steps = Vec::new();
        steps.extend(lcd.map(Step::Lcd));
        steps.extend(pc.map(Step::Pc));
        const LEN: u32 = 5;
        for schedule in 0..steps.len().pow(LEN) {
            for stuck in [false, true] {
                T.set(0);
                let g = Guard::new(
                    [Level::Preempting, Level::Background],
                    Trial::NeverFeed,
                    || T.get(),
                );
                g.install(Count::default(), false);
                let feeds = |g: &Guard<Count, 2>| {
                    let before = fed(g);
                    g.feed();
                    fed(g) > before
                };
                g.busy(LCD);
                // The display's own running time since its last report.
                let mut ran = 0;
                let mut plan = schedule;
                for _ in 0..LEN {
                    let step = steps[plan % steps.len()];
                    plan /= steps.len();
                    let why = std::format!("schedule {schedule}, stuck {stuck}, at {step:?}");
                    match step {
                        Step::Lcd(ms) => {
                            T.set(T.get() + ms);
                            ran += ms;
                            if !stuck {
                                assert!(feeds(&g), "{why}: within its limit");
                                g.busy(LCD);
                                ran = 0;
                            }
                        }
                        Step::Pc(ms) => {
                            g.busy(PC);
                            let mut left = ms;
                            while left > 0 {
                                let piece = left.min(BUSY_LIMIT_MS - 1);
                                T.set(T.get() + piece);
                                left -= piece;
                                // The pre-request feed of the serving task.
                                assert_eq!(feeds(&g), ran < BUSY_LIMIT_MS, "{why}: while serving");
                                g.busy(PC);
                            }
                            g.idle(PC);
                        }
                    }
                    assert_eq!(feeds(&g), ran < BUSY_LIMIT_MS, "{why}: ran {ran} ms");
                }
            }
        }
    }
}
