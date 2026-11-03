//! State shared between the relay tasks and a display, each value behind a
//! critical-section mutex (the tasks run on different executors and
//! priorities). Writers: the PC task (sub-node list, the 親機's own state,
//! SHOW progress, quiet time), the link tasks (what each node answered).
//! Reader: a display ([`home`], [`REDRAW`]) or the app.

use core::cell::{Cell, RefCell};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant};

use crate::nodes::{self, Subs};
pub use crate::view::{merge, Home, Node, Progress, Seen, Shade};

/// No own polling (and the progress screen stays) for this long after a
/// RELAY or SHOW: the PC drives the nodes.
const QUIET_AFTER_PC: Duration = Duration::from_secs(10);
/// Topology node indexes are below 8.
const MAX_INDEX: usize = 8;

static SUBS: Mutex<Cs, RefCell<Subs>> = Mutex::new(RefCell::new(Subs::new()));
static SEEN: Mutex<Cs, Cell<[Seen; MAX_INDEX]>> = Mutex::new(Cell::new(
    [Seen {
        version: None,
        shade: Shade::Unknown,
    }; MAX_INDEX],
));
static HUB: Mutex<Cs, Cell<Seen>> = Mutex::new(Cell::new(Seen {
    version: None,
    shade: Shade::Unknown,
}));
static PROGRESS: Mutex<Cs, RefCell<Option<Progress>>> = Mutex::new(RefCell::new(None));
static QUIET_UNTIL: Mutex<Cs, Cell<Instant>> = Mutex::new(Cell::new(Instant::from_ticks(0)));

/// Something on the home screen changed.
pub static REDRAW: Signal<Cs, ()> = Signal::new();

pub fn set_subs(subs: Subs) {
    SUBS.lock(|s| *s.borrow_mut() = subs);
    REDRAW.signal(());
}

pub fn subs() -> Subs {
    SUBS.lock(|s| s.borrow().clone())
}

/// What the node at `link`/`addr` answered (a relayed HELLO or STATUS).
pub fn set_seen_at(link: u8, addr: u8, seen: Seen) {
    let index = SUBS.lock(|s| {
        s.borrow()
            .iter()
            .find(|n| n.link == link && n.addr == addr)
            .map(|n| n.index)
    });
    if let Some(i) = index {
        set_seen(i, seen);
    }
}

pub fn set_seen(index: u8, seen: Seen) {
    let changed = SEEN.lock(|s| {
        let mut all = s.get();
        let slot = all.get_mut(index as usize);
        let mut changed = false;
        if let Some(old) = slot {
            let new = merge(*old, seen);
            changed = *old != new;
            *old = new;
        }
        s.set(all);
        changed
    });
    if changed {
        REDRAW.signal(());
    }
}

pub fn set_hub(seen: Seen) {
    if HUB.lock(|h| h.replace(seen)) != seen {
        REDRAW.signal(());
    }
}

/// A RELAY or SHOW arrived: the PC drives the nodes for a while.
pub fn pc_active() {
    QUIET_UNTIL.lock(|q| q.set(Instant::now() + QUIET_AFTER_PC));
}

pub fn pc_quiet() -> bool {
    Instant::now() >= QUIET_UNTIL.lock(|q| q.get())
}

pub fn set_progress(p: Progress) {
    PROGRESS.lock(|s| *s.borrow_mut() = Some(p));
    REDRAW.signal(());
}

/// Drops the progress screen once the PC has been quiet; true if it did.
pub fn expire_progress() -> bool {
    pc_quiet() && PROGRESS.lock(|s| s.borrow_mut().take()).is_some()
}

/// What the home screen shows now.
pub fn home() -> Home {
    let seen = SEEN.lock(|s| s.get());
    let hub = HUB.lock(|h| h.get());
    let mut home = Home {
        hub: Node {
            node: nodes::HUB,
            version: hub.version,
            shade: hub.shade,
        },
        progress: PROGRESS.lock(|p| p.borrow().clone()),
        ..Default::default()
    };
    for sub in subs() {
        let s = seen.get(sub.index as usize).copied().unwrap_or_default();
        let _ = home.subs.push(Node {
            node: sub,
            version: s.version,
            shade: s.shade,
        });
    }
    home
}
