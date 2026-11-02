//! Liveness-gated watchdog guard. The app declares one `static` [`Guard`]
//! with the [`Level`] of each of its tasks; each task reports
//! [`Guard::busy`] when it starts a piece of work and [`Guard::idle`] when
//! it waits for the next one. The watchdog is fed only through
//! [`Guard::feed`], which feeds only while every task is within the
//! deadline of its current work (see [`crate::liveness`]: background
//! deadlines are frozen while a preempting task works). A stuck task
//! therefore ends in a reset instead of a silent hang.
//!
//! On trial (an unconfirmed image) the [`Trial`] policy decides: keep
//! feeding for a while after boot, or never feed. Either way an image that
//! is never confirmed resets and the boot core counts the attempt.

use core::cell::RefCell;

use critical_section::Mutex;

use crate::liveness::{Level, Liveness};

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
            })),
            trial,
            now_ms,
        }
    }

    /// Takes the running watchdog (started by the boot core or the app).
    pub fn install(&self, dog: D, on_trial: bool) {
        critical_section::with(|cs| {
            let mut s = self.state.borrow_ref_mut(cs);
            s.dog = Some(dog);
            s.on_trial = on_trial;
        });
    }

    /// The running image is (no longer) on trial.
    pub fn set_trial(&self, on_trial: bool) {
        critical_section::with(|cs| self.state.borrow_ref_mut(cs).on_trial = on_trial);
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
        let now = (self.now_ms)();
        critical_section::with(|cs| self.state.borrow_ref_mut(cs).live.busy(task, now, ms));
    }

    /// `task` waits for its next piece of work.
    pub fn idle(&self, task: usize) {
        let now = (self.now_ms)();
        critical_section::with(|cs| self.state.borrow_ref_mut(cs).live.idle(task, now));
    }

    /// Feeds if every task is alive and the trial policy allows it. The
    /// only way the watchdog is fed.
    pub fn feed(&self) {
        let now = (self.now_ms)();
        critical_section::with(|cs| {
            let mut s = self.state.borrow_ref_mut(cs);
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
        critical_section::with(|cs| self.state.borrow_ref_mut(cs).dog.as_mut().map(f))
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
}
