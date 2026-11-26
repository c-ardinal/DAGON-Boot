//! The display as its task drives it: the expander writes of
//! [`crate::shield`] over a bus, each within its limit. The bus and the
//! time are the firmware's to give ([`Bus`], [`Time`]), so the limits and
//! what a held bus does are tested on the host with a bus that never
//! answers.

use core::future::{poll_fn, Future};
use core::pin::pin;
use core::task::Poll;

use crate::seg::DIGITS;
use crate::shield::{self, WRITE_TIMEOUT_MS};

/// The I2C bus the expanders are on.
#[allow(async_fn_in_trait)]
pub trait Bus {
    type Error;
    /// Writes `bytes` to the device at 7-bit address `addr`. It may never
    /// end (a held bus); dropping it gives the write up.
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// The time the limits are kept by.
#[allow(async_fn_in_trait)]
pub trait Time {
    /// Microseconds since some fixed moment.
    fn now_us(&self) -> u64;
    /// Ends `ms` milliseconds from now.
    async fn after_ms(&self, ms: u64);
}

/// Why a showing failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault<E> {
    /// A write took [`WRITE_TIMEOUT_MS`] and was given up.
    Timeout,
    /// The bus refused a write.
    Bus(E),
}

/// The 4-digit display, sensors held in XSHUT.
pub struct Screen<B, T> {
    bus: B,
    time: T,
    ready: bool,
    longest_us: u64,
}

impl<B: Bus, T: Time> Screen<B, T> {
    /// Nothing is written until [`Screen::show`].
    pub fn new(bus: B, time: T) -> Self {
        Screen {
            bus,
            time,
            ready: false,
            longest_us: 0,
        }
    }

    /// Shows `codes` ([`crate::seg`] text). The first call sets the
    /// expanders up ([`shield::INIT`]); a failed set-up is retried on the
    /// next call. A showing ends within [`shield::SHOW_BOUND_MS`], failed
    /// or not: no write after a failed one is made.
    pub async fn show(&mut self, codes: [u8; DIGITS]) -> Result<(), Fault<B::Error>> {
        if !self.ready {
            self.write_all(&shield::INIT).await?;
            self.ready = true;
        }
        self.write_all(&shield::show(&codes)).await
    }

    /// The longest write that ended so far, in microseconds: the measured
    /// side of [`WRITE_TIMEOUT_MS`].
    pub fn longest_us(&self) -> u64 {
        self.longest_us
    }

    async fn write_all(&mut self, writes: &[shield::Write]) -> Result<(), Fault<B::Error>> {
        for (addr, bytes) in writes {
            let started = self.time.now_us();
            let done = within(
                self.bus.write(*addr, bytes),
                self.time.after_ms(WRITE_TIMEOUT_MS),
            )
            .await;
            done.ok_or(Fault::Timeout)?.map_err(Fault::Bus)?;
            let us = self.time.now_us().saturating_sub(started);
            self.longest_us = self.longest_us.max(us);
        }
        Ok(())
    }
}

/// What `work` gives, or `None` once `limit` ended first (`work` is
/// dropped then). `work` is asked first: one that ends in the same turn as
/// its limit counts as ended.
async fn within<W: Future, L: Future>(work: W, limit: L) -> Option<W::Output> {
    let mut work = pin!(work);
    let mut limit = pin!(limit);
    poll_fn(|cx| {
        if let Poll::Ready(out) = work.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        limit.as_mut().poll(cx).map(|_| None)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seg::BLANK;
    use crate::shield::{INIT, SHOW_BOUND_MS, SHOW_WRITES};
    use core::cell::{Cell, RefCell};
    use core::future::pending;
    use core::task::{Context, Waker};
    use std::rc::Rc;

    /// What the bus does with one write.
    #[derive(Clone, Copy)]
    enum Does {
        /// Ends after this many microseconds.
        Takes(u64),
        /// Refuses.
        Nacks,
        /// Never ends: something holds the bus.
        Held,
    }

    #[derive(Default)]
    struct World {
        now_us: Cell<u64>,
        /// What each write does, in order; `Takes(380)` once it runs out.
        plan: RefCell<Vec<Does>>,
        written: RefCell<Vec<shield::Write>>,
    }

    struct SimBus(Rc<World>);
    struct SimTime(Rc<World>);

    impl Bus for SimBus {
        type Error = ();
        async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), ()> {
            let w = &self.0;
            w.written
                .borrow_mut()
                .push((addr, bytes.try_into().unwrap()));
            let does = {
                let mut plan = w.plan.borrow_mut();
                if plan.is_empty() {
                    Does::Takes(380)
                } else {
                    plan.remove(0)
                }
            };
            match does {
                Does::Takes(us) => {
                    w.now_us.set(w.now_us.get() + us);
                    Ok(())
                }
                Does::Nacks => Err(()),
                Does::Held => pending().await,
            }
        }
    }

    impl Time for SimTime {
        fn now_us(&self) -> u64 {
            self.0.now_us.get()
        }
        /// Asked only while the write is held: the time passes at once.
        async fn after_ms(&self, ms: u64) {
            self.0.now_us.set(self.0.now_us.get() + ms * 1_000);
        }
    }

    fn screen(plan: &[Does]) -> (Rc<World>, Screen<SimBus, SimTime>) {
        let w = Rc::new(World::default());
        *w.plan.borrow_mut() = plan.to_vec();
        (w.clone(), Screen::new(SimBus(w.clone()), SimTime(w)))
    }

    /// Nothing here waits for a wake: a future that is not ready at its
    /// first turn would hang the firmware's task too.
    fn run<F: Future>(f: F) -> F::Output {
        let mut f = pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(out) => out,
            Poll::Pending => panic!("the showing did not end"),
        }
    }

    const TEXT: [u8; DIGITS] = BLANK;

    #[test]
    fn the_first_showing_sets_up_and_later_ones_write_the_digits_only() {
        let (w, mut s) = screen(&[]);
        assert_eq!(run(s.show(TEXT)), Ok(()));
        let first = w.written.borrow().clone();
        assert_eq!(first[..INIT.len()], INIT);
        assert_eq!(first[INIT.len()..], shield::show(&TEXT));
        assert_eq!(run(s.show(TEXT)), Ok(()));
        assert_eq!(w.written.borrow().len(), first.len() + SHOW_WRITES);
        assert_eq!(s.longest_us(), 380);
    }

    #[test]
    fn a_held_bus_fails_the_write_at_its_limit_and_no_later_write_is_made() {
        let (w, mut s) = screen(&[Does::Takes(380), Does::Held]);
        assert_eq!(run(s.show(TEXT)), Err(Fault::Timeout));
        assert_eq!(w.written.borrow().len(), 2);
        assert_eq!(w.now_us.get(), 380 + WRITE_TIMEOUT_MS * 1_000);
        // A write that was given up is no measurement.
        assert_eq!(s.longest_us(), 380);
    }

    #[test]
    fn a_failed_set_up_is_made_again_at_the_next_showing() {
        for fails in [Does::Held, Does::Nacks] {
            let (w, mut s) = screen(&[Does::Takes(380), Does::Takes(380), fails]);
            let fault = run(s.show(TEXT)).unwrap_err();
            assert_eq!(matches!(fails, Does::Held), fault == Fault::Timeout);
            w.written.borrow_mut().clear();
            assert_eq!(run(s.show(TEXT)), Ok(()));
            assert_eq!(w.written.borrow()[..INIT.len()], INIT);
        }
    }

    #[test]
    fn a_failure_after_the_set_up_keeps_the_set_up() {
        let mut plan = vec![Does::Takes(380); INIT.len()];
        plan.push(Does::Held);
        let (w, mut s) = screen(&plan);
        assert_eq!(run(s.show(TEXT)), Err(Fault::Timeout));
        w.written.borrow_mut().clear();
        assert_eq!(run(s.show(TEXT)), Ok(()));
        assert_eq!(w.written.borrow()[..], shield::show(&TEXT));
    }

    /// The bound the display task gives the watchdog guard holds for the
    /// slowest showing there is: every write just within its limit, and
    /// the last one held.
    #[test]
    fn no_showing_takes_longer_than_its_bound() {
        let slow = Does::Takes(WRITE_TIMEOUT_MS * 1_000 - 1);
        let writes = INIT.len() + SHOW_WRITES;
        for held_at in 0..=writes {
            let mut plan = vec![slow; writes];
            if held_at < writes {
                plan[held_at] = Does::Held;
            }
            let (w, mut s) = screen(&plan);
            let _ = run(s.show(TEXT));
            assert!(w.now_us.get() <= SHOW_BOUND_MS * 1_000, "{held_at}");
        }
        assert_eq!(writes as u64 * WRITE_TIMEOUT_MS, SHOW_BOUND_MS);
    }

    /// A write that ends in the same turn as its limit counts as written.
    #[test]
    fn a_write_that_ends_with_its_limit_is_not_given_up() {
        let (_, mut s) = screen(&[Does::Takes(WRITE_TIMEOUT_MS * 1_000)]);
        assert_eq!(run(s.show(TEXT)), Ok(()));
        assert_eq!(s.longest_us(), WRITE_TIMEOUT_MS * 1_000);
    }
}
