//! What the relay knows, shared between its tasks and a display. The tasks
//! may run on different executors and priorities, so every value sits
//! behind one critical-section mutex. Writers: [`serve`](crate::serve) (the
//! linked nodes, this node's own state, SHOW progress, the PC's quiet
//! time) and the link workers (what each node answered). Readers: a display
//! ([`State::home`], [`State::changed`]) or the app.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

use crate::nodes::{LinkedNode, LinkedNodes, Loaded, NO_INDEX};
use crate::view::{merge, Home, Node, Progress, Seen, Shade, HOME_ROWS};

/// Topology node indexes are below this.
const MAX_INDEX: usize = dagon_format::topology::MAX_NODES;

const UNSEEN: Seen = Seen {
    version: None,
    shade: Shade::Unknown,
};

struct Values {
    /// This node.
    node: LinkedNode,
    linked: LinkedNodes,
    seen: [Seen; MAX_INDEX],
    own: Seen,
    progress: Option<Progress>,
    /// The PC drives the nodes until this time (ms).
    quiet_from: u64,
}

/// The relay's state: one instance, owned by the caller and shared by `&`
/// with the relay's tasks and a display.
pub struct State {
    values: Mutex<Cs, RefCell<Values>>,
    changed: Signal<Cs, ()>,
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

impl State {
    pub const fn new() -> Self {
        State {
            values: Mutex::new(RefCell::new(Values {
                node: LinkedNode {
                    index: NO_INDEX,
                    name: [0; 16],
                    link: 0,
                    addr: 0,
                },
                linked: LinkedNodes::new(),
                seen: [UNSEEN; MAX_INDEX],
                own: UNSEEN,
                progress: None,
                quiet_from: 0,
            })),
            changed: Signal::new(),
        }
    }

    /// The values are read and written only here. The one place the relay
    /// masks interrupts: each closure copies or replaces a small value (one
    /// node's entry, the node list of at most `MAX_NODES` entries of 19
    /// bytes, the home screen's few entries), well inside `[execution]
    /// masked_max_us`; none reaches the flash or waits.
    #[allow(clippy::disallowed_methods)]
    fn locked<R>(&self, f: impl FnOnce(&mut Values) -> R) -> R {
        self.values.lock(|v| f(&mut v.borrow_mut()))
    }

    /// Returns once something on the home screen changed since the last
    /// call.
    pub async fn changed(&self) {
        self.changed.wait().await
    }

    /// This node and every node linked to it.
    pub(crate) fn set_nodes(&self, nodes: Loaded) {
        self.locked(|v| {
            v.node = nodes.own;
            v.linked = nodes.linked;
        });
        self.changed.signal(());
    }

    /// Every node linked to this one.
    pub fn linked(&self) -> LinkedNodes {
        self.locked(|v| v.linked.clone())
    }

    /// The `i`-th linked node.
    pub(crate) fn linked_at(&self, i: usize) -> Option<LinkedNode> {
        self.locked(|v| v.linked.get(i).copied())
    }

    /// What the node at `link`/`addr` answered (a relayed HELLO or STATUS).
    pub(crate) fn set_seen_at(&self, link: u8, addr: u8, seen: Seen) {
        let index = self.locked(|v| {
            v.linked
                .iter()
                .find(|n| n.link == link && n.addr == addr)
                .map(|n| n.index)
        });
        if let Some(i) = index {
            self.set_seen(i, seen);
        }
    }

    pub(crate) fn set_seen(&self, index: u8, seen: Seen) {
        let changed = self.locked(|v| {
            let Some(old) = v.seen.get_mut(index as usize) else {
                return false;
            };
            let new = merge(*old, seen);
            let changed = *old != new;
            *old = new;
            changed
        });
        if changed {
            self.changed.signal(());
        }
    }

    pub(crate) fn set_own(&self, seen: Seen) {
        if self.locked(|v| core::mem::replace(&mut v.own, seen)) != seen {
            self.changed.signal(());
        }
    }

    /// A RELAY or SHOW arrived at `now_ms`: the PC drives the nodes for
    /// `quiet_ms`.
    pub(crate) fn pc_active(&self, now_ms: u64, quiet_ms: u32) {
        self.locked(|v| v.quiet_from = now_ms + u64::from(quiet_ms));
    }

    /// Whether the PC has been quiet long enough at `now_ms`.
    pub fn pc_quiet(&self, now_ms: u64) -> bool {
        now_ms >= self.locked(|v| v.quiet_from)
    }

    pub(crate) fn set_progress(&self, p: Progress) {
        self.locked(|v| v.progress = Some(p));
        self.changed.signal(());
    }

    /// Drops the progress screen once the PC has been quiet at `now_ms`;
    /// true if it did.
    pub fn expire_progress(&self, now_ms: u64) -> bool {
        self.pc_quiet(now_ms) && self.locked(|v| v.progress.take()).is_some()
    }

    /// What the home screen shows now: this node and the first
    /// [`HOME_ROWS`] linked nodes.
    pub fn home(&self) -> Home {
        self.locked(|v| {
            let mut home = Home {
                own: Node {
                    node: v.node,
                    version: v.own.version,
                    shade: v.own.shade,
                },
                progress: v.progress.clone(),
                ..Default::default()
            };
            for n in v.linked.iter().take(HOME_ROWS) {
                let s = v.seen.get(n.index as usize).copied().unwrap_or_default();
                let _ = home.linked.push(Node {
                    node: *n,
                    version: s.version,
                    shade: s.shade,
                });
            }
            home
        })
    }
}
