//! Waitable published state for async display and log tasks. The
//! layer's composed chain ([`crate::compose`]) is its only writer.
//!
//! Interrupt masking (the execution model's rule 2): nothing here masks
//! by itself (`clippy.toml` refuses it). The `Channel` and `Watch` of
//! embassy-sync mask for each of their operations, to copy one
//! [`EventRecord`] or one [`Latest`] in or out; no closure and no
//! [`Snapshot::apply`] runs inside one. The counters beside them are
//! atomics written by the one writer.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
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
    /// Records lost and not yet reported. The one producer (the layer's
    /// composed chain) is its only writer, so a load and a store suffice.
    unreported: AtomicU32,
}

impl<const N: usize> Recent<N> {
    const NOT_EMPTY: () = assert!(N > 0, "a Recent ring holds at least one record");

    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::NOT_EMPTY;
        Recent {
            ring: Channel::new(),
            unreported: AtomicU32::new(0),
        }
    }

    /// Each ring operation masks for one record; a reader that takes a
    /// record between two of them only makes room.
    pub(crate) fn push(&self, mut rec: EventRecord) {
        let mut lost = self.unreported.load(Ordering::Relaxed);
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
        self.unreported.store(lost, Ordering::Relaxed);
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
