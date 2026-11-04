//! Waitable published state for async display and log tasks, and the
//! fixed composition order that puts its publisher first.

use core::cell::Cell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::watch::Watch;

use crate::event::{Event, EventRecord};
use crate::extension::{Chain, Ctx, Extension};

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

/// The last `N` events as owned records, oldest first. A full ring drops
/// its oldest record and the next record pushed counts it in `dropped`;
/// the producer never waits.
pub struct Recent<const N: usize> {
    ring: Channel<Cs, EventRecord, N>,
    /// Records dropped since the last one pushed.
    dropped: Mutex<Cs, Cell<u32>>,
}

impl<const N: usize> Recent<N> {
    pub const fn new() -> Self {
        Recent {
            ring: Channel::new(),
            dropped: Mutex::new(Cell::new(0)),
        }
    }

    pub fn push(&self, mut rec: EventRecord) {
        self.dropped.lock(|d| {
            if self.ring.is_full() {
                if let Ok(oldest) = self.ring.try_receive() {
                    // What the dropped record reported is lost with it.
                    d.set(d.get().saturating_add(1 + u32::from(oldest.dropped)));
                }
            }
            rec.dropped = u8::try_from(d.get()).unwrap_or(u8::MAX);
            if self.ring.try_send(rec).is_ok() {
                d.set(0);
            } else {
                d.set(d.get().saturating_add(1));
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
/// tasks, and the ring of recent records. One `static` per image, handed
/// to the layer's start.
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

    /// The latest state: `receiver()` to wait for changes,
    /// `try_get()` to read it.
    pub fn latest(&self) -> &Watch<Cs, Latest<S>, WATCHERS> {
        &self.latest
    }

    pub fn recent(&self) -> &Recent<N> {
        &self.recent
    }

    fn publish(&self, e: &Event<'_>) {
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

/// The extension that publishes every event. Made only by [`compose`],
/// so it is always first and appears once.
pub struct Publisher<S: Snapshot + 'static, const N: usize>(&'static Published<S, N>);

impl<S: Snapshot + 'static, const N: usize> Extension for Publisher<S, N> {
    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        self.0.publish(e);
    }
}

/// The layer's chain in the one order every layer uses: the publisher,
/// then the board package, then the user's extensions (libraries, then
/// the app's own; [`crate::extensions!`]). Layers call this in their
/// start; users never order the chain themselves.
pub fn compose<S: Snapshot + 'static, const N: usize, B: Extension, U: Extension>(
    events: &'static Published<S, N>,
    board: B,
    user: U,
) -> Chain<Publisher<S, N>, Chain<B, U>> {
    Chain(Publisher(events), Chain(board, user))
}
