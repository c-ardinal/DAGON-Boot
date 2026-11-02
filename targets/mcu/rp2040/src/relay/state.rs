//! State shared between the relay tasks and a display, each value behind a
//! critical-section mutex (the tasks run on different executors and
//! priorities). Writers: the PC task (sub-node list, the 親機's own state,
//! SHOW progress, quiet time), the link tasks (what each node answered).
//! Reader: a display ([`home`], [`REDRAW`]) or the app.

use core::cell::{Cell, RefCell};

use dagon_format::Version;
use dagon_proto::payload::{Show, SHOW_TEXT_MAX};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant};

use heapless::String;

use super::nodes::{self, SubNode, Subs, MAX_SUBS};

/// How a node is doing, as far as the 親機 knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shade {
    #[default]
    Unknown,
    Confirmed,
    Trial,
    NoAnswer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub node: SubNode,
    pub version: Option<Version>,
    pub shade: Shade,
}

/// The last SHOW.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub node: u8,
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
    /// Printable ASCII of the SHOW text.
    pub text: String<SHOW_TEXT_MAX>,
}

impl From<&Show<'_>> for Progress {
    fn from(s: &Show<'_>) -> Self {
        let mut text = String::new();
        if s.text.iter().all(|b| (0x20..0x7F).contains(b)) {
            s.text.iter().for_each(|&b| {
                let _ = text.push(b as char);
            });
        }
        Progress {
            node: s.node,
            phase: s.phase,
            percent: s.percent.min(100),
            version: s.version,
            text,
        }
    }
}

/// Everything a home screen shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    pub hub: Node,
    pub subs: heapless::Vec<Node, MAX_SUBS>,
    pub progress: Option<Progress>,
}

/// No own polling (and the progress screen stays) for this long after a
/// RELAY or SHOW: the PC drives the nodes.
const QUIET_AFTER_PC: Duration = Duration::from_secs(10);
/// Topology node indexes are below 8.
const MAX_INDEX: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub version: Option<Version>,
    pub shade: Shade,
}

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

pub fn set_seen(index: u8, seen: Seen) {
    let changed = SEEN.lock(|s| {
        let mut all = s.get();
        let slot = all.get_mut(index as usize);
        let changed = slot.as_ref().is_some_and(|old| **old != seen);
        if let Some(old) = slot {
            *old = seen;
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
