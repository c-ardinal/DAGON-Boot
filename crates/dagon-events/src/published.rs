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
/// tasks, and the ring of recent records. One `static` per image, handed
/// to the layer's start; tasks only read it. It is not an extension (only
/// [`crate::compose`] puts it in a chain, first), and nothing outside this
/// crate writes it.
///
/// ```compile_fail,E0277
/// use dagon_events::{extensions, Nop, Published};
/// static EVENTS: Published<(), 4> = Published::new();
/// let user = extensions![Nop, &EVENTS];
/// ```
///
/// ```compile_fail,E0624
/// use dagon_events::{EventRecord, Published};
/// static EVENTS: Published<(), 4> = Published::new();
/// EVENTS.recent().push(EventRecord::default());
/// ```
///
/// ```compile_fail,E0080
/// static EMPTY: dagon_events::Recent<0> = dagon_events::Recent::new();
/// ```
pub struct Published<S: Snapshot, const N: usize> {
    latest: Watch<Cs, Latest<S>, WATCHERS>,
    recent: Recent<N>,
    seq: Mutex<Cs, Cell<u32>>,
    /// Taken by the one chain composed with it.
    claimed: Mutex<Cs, Cell<bool>>,
}

impl<S: Snapshot, const N: usize> Published<S, N> {
    pub const fn new() -> Self {
        Published {
            latest: Watch::new(),
            recent: Recent::new(),
            seq: Mutex::new(Cell::new(0)),
            claimed: Mutex::new(Cell::new(false)),
        }
    }

    /// A receiver that waits for the next latest state (`changed()`);
    /// `None` once [`WATCHERS`] receivers exist.
    pub fn receiver(&self) -> Option<Receiver<'_, Cs, Latest<S>, WATCHERS>> {
        self.latest.receiver()
    }

    /// The latest state, if any event was published.
    pub fn latest(&self) -> Option<Latest<S>> {
        self.latest.try_get()
    }

    pub fn recent(&self) -> &Recent<N> {
        &self.recent
    }

    /// `true` for the first caller only.
    pub(crate) fn claim(&self) -> bool {
        self.claimed.lock(|c| !c.replace(true))
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
