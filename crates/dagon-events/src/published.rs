//! Waitable published state for async display and log tasks. The
//! layer's composed chain ([`crate::compose`]) is its only writer.
//!
//! Interrupt masking (the execution model's rule 2): `clippy.toml`
//! refuses it here, with one reviewed exemption: [`Recent::push`] holds
//! one critical section over its ring operations (at most three, each a
//! copy of one [`EventRecord`]), so that a reader cannot take a record
//! between the check for a full ring and the removal of its oldest
//! record. Beside it, the `Channel` and `Watch` of embassy-sync mask for
//! each of their operations, to copy one [`EventRecord`] or one
//! [`Latest`] in or out; no closure of a caller and no
//! [`Snapshot::apply`] runs inside one. The event count is an atomic
//! written by the one writer.

use core::cell::Cell;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::watch::{Receiver, Watch};

use crate::event::{Event, EventRecord};

/// What a layer folds events into for its display tasks (the app's phase
/// and progress, the relay's children). `()` keeps nothing beyond
/// [`Latest::last`].
pub trait Snapshot: Clone + Default {
    fn apply(&mut self, _e: &Event<'_>) {}
}

impl Snapshot for () {}

/// The latest state: the layer's snapshot and the last event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Latest<S> {
    pub state: S,
    /// The last event's record; `seq` counts every event published.
    pub last: EventRecord,
}

/// Receivers [`Published::latest`] can hand out.
pub const WATCHERS: usize = 4;

/// The last `N` events as owned records, oldest first (`N` > 0). A full
/// ring drops its oldest record; the records pushed after it report the
/// loss in `dropped`, up to 255 each, carrying the rest to the next ones,
/// so the `dropped` values a reader sees add up to every record lost.
/// The producer never waits. A count of lost records beyond `u32::MAX`
/// stays there.
pub struct Recent<const N: usize> {
    ring: Channel<Cs, EventRecord, N>,
    /// Records lost and not yet reported; its lock is the critical
    /// section of [`Recent::push`].
    unreported: Mutex<Cs, Cell<u32>>,
    /// Times the critical section of [`Recent::push`] ([`Timing`]).
    #[cfg(any(test, feature = "masked-time"))]
    timing: Option<Timing>,
}

/// How a measuring build times the critical section of [`Recent::push`]:
/// `now_us` gives microseconds since boot, `note` takes each section's
/// length. Both run with interrupts masked, so they only read a clock and
/// store a few words.
#[cfg(any(test, feature = "masked-time"))]
#[derive(Clone, Copy)]
pub struct Timing {
    pub now_us: fn() -> u64,
    pub note: fn(u64),
}

impl<const N: usize> Recent<N> {
    const NOT_EMPTY: () = assert!(N > 0, "a Recent ring holds at least one record");

    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::NOT_EMPTY;
        Recent {
            ring: Channel::new(),
            unreported: Mutex::new(Cell::new(0)),
            #[cfg(any(test, feature = "masked-time"))]
            timing: None,
        }
    }

    /// [`Recent::new`] whose critical section is timed with `timing`.
    #[cfg(any(test, feature = "masked-time"))]
    pub const fn timed(timing: Timing) -> Self {
        let mut ring = Self::new();
        ring.timing = Some(timing);
        ring
    }

    /// One critical section over the check for a full ring, the removal
    /// of its oldest record and the send: a reader that preempted between
    /// them would make room and still lose the next record. Inside it: at
    /// most three ring operations, each a copy of one [`EventRecord`],
    /// and a counter; no caller's code but, in a measuring build, the
    /// two functions of its [`Timing`] around them.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn push(&self, mut rec: EventRecord) {
        self.unreported.lock(|u| {
            #[cfg(any(test, feature = "masked-time"))]
            let started = self.timing.map(|t| (t.now_us)());
            let mut lost = u.get();
            if self.ring.is_full() {
                if let Ok(oldest) = self.ring.try_receive() {
                    // The dropped record's own report is lost with it.
                    lost = lost.saturating_add(1 + u32::from(oldest.dropped));
                }
            }
            let report = lost.min(u32::from(u8::MAX));
            rec.dropped = report as u8;
            if self.ring.try_send(rec).is_ok() {
                lost -= report;
            } else {
                lost = lost.saturating_add(1);
            }
            u.set(lost);
            #[cfg(any(test, feature = "masked-time"))]
            if let (Some(t), Some(started)) = (self.timing, started) {
                (t.note)((t.now_us)().saturating_sub(started));
            }
        });
    }

    /// The oldest record, if any.
    pub fn try_take(&self) -> Option<EventRecord> {
        self.ring.try_receive().ok()
    }

    /// Waits for the oldest record.
    pub async fn take(&self) -> EventRecord {
        self.ring.receive().await
    }
}

impl<const N: usize> Default for Recent<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// A layer's published events: the latest state, waitable by several
/// tasks, and the ring of recent records.
///
/// The layer package (the MCU package's start, the host) creates it,
/// keeps it private and hands it to [`crate::compose`] by exclusive
/// reference, so the composed chain is its only writer. Everyone else,
/// application tasks included, gets a [`Reader`] from the chain
/// ([`crate::Composed::reader`]), which only reads. It is not an
/// extension.
///
/// ```compile_fail,E0277
/// use dagon_events::{extensions, Nop, Published};
/// static EVENTS: Published<(), 4> = Published::new();
/// let user = extensions![Nop, &EVENTS];
/// ```
///
/// ```compile_fail,E0599
/// use dagon_events::Published;
/// static EVENTS: Published<(), 4> = Published::new();
/// let rx = EVENTS.receiver();
/// ```
///
/// ```compile_fail,E0080
/// static EMPTY: dagon_events::Recent<0> = dagon_events::Recent::new();
/// ```
pub struct Published<S: Snapshot, const N: usize> {
    latest: Watch<Cs, Latest<S>, WATCHERS>,
    recent: Recent<N>,
    /// Events published; the one writer's (see [`Recent`]).
    seq: AtomicU32,
}

impl<S: Snapshot, const N: usize> Published<S, N> {
    pub const fn new() -> Self {
        Published {
            latest: Watch::new(),
            recent: Recent::new(),
            seq: AtomicU32::new(0),
        }
    }

    /// [`Published::new`] whose ring of recent events times its critical
    /// section with `timing` ([`Recent::timed`]).
    #[cfg(any(test, feature = "masked-time"))]
    pub const fn timed(timing: Timing) -> Self {
        Published {
            latest: Watch::new(),
            recent: Recent::timed(timing),
            seq: AtomicU32::new(0),
        }
    }

    /// The snapshot is folded outside the watch: taken out (a copy),
    /// applied, sent back (a move), so [`Snapshot::apply`] never runs with
    /// interrupts masked. The one writer loses no update by it.
    pub(crate) fn publish(&self, e: &Event<'_>) {
        let seq = self.seq.load(Ordering::Relaxed).wrapping_add(1);
        self.seq.store(seq, Ordering::Relaxed);
        let rec = e.record(seq);
        let mut l = self.latest.try_get().unwrap_or_default();
        l.state.apply(e);
        l.last = rec;
        self.latest.sender().send(l);
        self.recent.push(rec);
    }
}

impl<S: Snapshot, const N: usize> Default for Published<S, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Read access to a layer's [`Published`] events: what display, log and
/// application tasks get. It cannot write, compose or replay.
///
/// ```compile_fail,E0624
/// fn forge(r: dagon_events::Reader<(), 4>) {
///     r.recent().push(dagon_events::EventRecord::default());
/// }
/// ```
pub struct Reader<S: Snapshot + 'static, const N: usize>(&'static Published<S, N>);

impl<S: Snapshot + 'static, const N: usize> Clone for Reader<S, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Snapshot + 'static, const N: usize> Copy for Reader<S, N> {}

impl<S: Snapshot + 'static, const N: usize> Reader<S, N> {
    pub(crate) fn new(p: &'static Published<S, N>) -> Self {
        Reader(p)
    }

    /// A receiver that waits for the next latest state (`changed()`);
    /// `None` once [`WATCHERS`] receivers exist.
    pub fn receiver(&self) -> Option<Receiver<'static, Cs, Latest<S>, WATCHERS>> {
        self.0.latest.receiver()
    }

    /// The latest state, if any event was published.
    pub fn latest(&self) -> Option<Latest<S>> {
        self.0.latest.try_get()
    }

    /// The ring of recent records, to take from.
    pub fn recent(&self) -> &'static Recent<N> {
        &self.0.recent
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::*;

    const PUSHES: usize = 8;
    const RING: usize = 2;

    /// A ring made with a [`Timing`] hands the length of each push's
    /// critical section to `note`, full ring or not; one made without
    /// times nothing.
    #[test]
    fn a_timed_ring_notes_the_length_of_each_push() {
        use core::sync::atomic::AtomicU64;
        static NOW: AtomicU64 = AtomicU64::new(0);
        static COUNT: AtomicU32 = AtomicU32::new(0);
        static LAST: AtomicU64 = AtomicU64::new(0);
        let noted = || (COUNT.load(Ordering::Relaxed), LAST.load(Ordering::Relaxed));
        let timing = Timing {
            // Each reading of the clock is 7 us after the one before.
            now_us: || NOW.fetch_add(7, Ordering::Relaxed),
            note: |us| {
                COUNT.fetch_add(1, Ordering::Relaxed);
                LAST.store(us, Ordering::Relaxed);
            },
        };
        let plain: Recent<RING> = Recent::new();
        plain.push(EventRecord::default());
        assert_eq!(noted(), (0, 0));
        let ring: Recent<RING> = Recent::timed(timing);
        for _ in 0..RING + 1 {
            ring.push(EventRecord::default());
        }
        assert_eq!(noted(), (RING as u32 + 1, 7));
        assert_eq!(ring.try_take().map(|r| r.dropped), Some(0));
        assert_eq!(ring.try_take().map(|r| r.dropped), Some(1));
    }

    /// A reader that runs between any two pushes (a push is one critical
    /// section, so nowhere inside one), taking any number of records
    /// there, never loses count: what it takes is in publishing order,
    /// and the losses the records report, with the ones not yet reported,
    /// are exactly the records it never got. Every schedule of up to
    /// `RING` takes after each of `PUSHES` pushes into a ring of `RING`.
    #[test]
    fn a_reader_between_any_pushes_keeps_the_count_of_lost_records() {
        let takes = RING + 1;
        for schedule in 0..takes.pow(PUSHES as u32) {
            let ring: Recent<RING> = Recent::new();
            let mut taken: Vec<EventRecord> = Vec::new();
            let mut plan = schedule;
            for seq in 1..=PUSHES as u32 {
                ring.push(EventRecord {
                    seq,
                    ..EventRecord::default()
                });
                for _ in 0..plan % takes {
                    taken.extend(ring.try_take());
                }
                plan /= takes;
            }
            while let Some(r) = ring.try_take() {
                taken.push(r);
            }
            assert!(
                taken.windows(2).all(|w| w[0].seq < w[1].seq),
                "schedule {schedule}: out of order {taken:?}"
            );
            let reported: u32 = taken.iter().map(|r| u32::from(r.dropped)).sum();
            // One more record into the emptied ring reports what was not
            // reported yet.
            ring.push(EventRecord::default());
            let unreported = u32::from(ring.try_take().expect("room").dropped);
            assert_eq!(
                (reported + unreported) as usize,
                PUSHES - taken.len(),
                "schedule {schedule}: {taken:?}, {unreported} unreported"
            );
            // A record is lost only to a full ring: the reader gets every
            // record it had room for.
            assert!(
                taken.len() >= RING.min(PUSHES),
                "schedule {schedule}: {taken:?}"
            );
            // A record never reports a loss that has not happened yet: the
            // reports up to it count records published before it that the
            // reader did not get.
            let mut reports = 0;
            for (got, r) in taken.iter().enumerate() {
                reports += u32::from(r.dropped);
                assert!(
                    reports <= r.seq - 1 - got as u32,
                    "schedule {schedule}: {r:?} reports a loss not before it in {taken:?}"
                );
            }
        }
    }
}
