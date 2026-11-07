//! Waitable published state for async display and log tasks. The
//! layer's composed chain ([`crate::compose`]) is its only writer.

use core::cell::Cell;

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
/// The producer never waits.
pub struct Recent<const N: usize> {
    ring: Channel<Cs, EventRecord, N>,
    /// Records lost and not yet reported.
    unreported: Mutex<Cs, Cell<u64>>,
}

impl<const N: usize> Recent<N> {
    const NOT_EMPTY: () = assert!(N > 0, "a Recent ring holds at least one record");

    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::NOT_EMPTY;
        Recent {
            ring: Channel::new(),
            unreported: Mutex::new(Cell::new(0)),
        }
    }

    pub(crate) fn push(&self, mut rec: EventRecord) {
        self.unreported.lock(|u| {
            let mut lost = u.get();
            if self.ring.is_full() {
                if let Ok(oldest) = self.ring.try_receive() {
                    // The dropped record's own report is lost with it.
                    lost = lost.saturating_add(1 + u64::from(oldest.dropped));
                }
            }
            let report = lost.min(u64::from(u8::MAX));
            rec.dropped = report as u8;
            if self.ring.try_send(rec).is_ok() {
                lost -= report;
            } else {
                lost = lost.saturating_add(1);
            }
            u.set(lost);
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
    seq: Mutex<Cs, Cell<u32>>,
}

impl<S: Snapshot, const N: usize> Published<S, N> {
    pub const fn new() -> Self {
        Published {
            latest: Watch::new(),
            recent: Recent::new(),
            seq: Mutex::new(Cell::new(0)),
        }
    }

    pub(crate) fn publish(&self, e: &Event<'_>) {
        let seq = self.seq.lock(|s| {
            let n = s.get().wrapping_add(1);
            s.set(n);
            n
        });
        let rec = e.record(seq);
        self.latest.sender().send_modify(|v| {
            let l = v.get_or_insert_with(Latest::default);
            l.state.apply(e);
            l.last = rec;
        });
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
