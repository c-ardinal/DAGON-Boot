//! The app layer's composed chain a node's start builds (publisher, the
//! board package's extension, the application's extensions, its
//! self-check function), what the application hands that start ([`App`])
//! and gets back ([`AppEvents`]: the published [`AppSnapshot`] and recent
//! events), and the start's last steps ([`finish_start`]): the board's
//! bring-up and confirming the app's own trial. The agent itself stays
//! the start's and its serving task's.

use dagon_agent::snapshot::AppSnapshot;
use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_events::{
    compose, BoardBringUp, Bounds, Chain, Composed, Ctx, Extension, Health, Layer, Published,
    Reader, TrialInfo, Watchdog,
};

/// Recent events a node keeps for its display and log tasks.
pub const RECENT: usize = 8;

/// A node's published events.
pub type Events = Published<AppSnapshot, RECENT>;

/// Read access to a node's published events, which the start returns to
/// the application: the latest [`AppSnapshot`] with the last event
/// (waitable) and the recent events.
pub type AppEvents = Reader<AppSnapshot, RECENT>;

/// The application's extensions as the start takes them: one value in a
/// `static` (`StaticCell::init(extensions![..])`), sent to DAGON's serving
/// task, which owns it from then on.
pub type UserExtensions = &'static mut (dyn Extension + Send);

/// The board package's extension as the start takes it from the board
/// definition: like [`UserExtensions`], one value in a `static`.
pub type BoardExtension = &'static mut (dyn Extension + Send);

/// The user part of a node's chain: the application's extensions, then its
/// self-check function.
pub type UserPart = Chain<Option<UserExtensions>, SelfCheck>;

/// A node's chain: its published events, the board package's extension
/// (if the board has one), the user part; decisions timed with the MCU
/// package's microsecond clock.
pub type NodeChain = Composed<&'static Events, Option<BoardExtension>, UserPart, fn() -> u64>;

/// The app's `fn() -> bool` as the `self_check` decision (`false` keeps
/// the version on trial, which then rolls back). Every other decision is
/// DAGON's default.
#[derive(Clone, Copy)]
pub struct SelfCheck(pub fn() -> bool);

impl Extension for SelfCheck {
    fn id(&self) -> &'static str {
        "app-self-check"
    }

    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        Some(if (self.0)() {
            Health::Healthy
        } else {
            Health::Unhealthy
        })
    }
}

/// Always confirm (dagon-agent's default).
pub fn always() -> bool {
    true
}

/// Who confirms a version on trial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Confirm {
    /// The host, with CONFIRM over the link.
    #[default]
    ByHost,
    /// The start itself, after the board's bring-up and before the link
    /// serves (`Agent::confirm_own`: the `self_check` decision, then the
    /// `confirmed` transition). For a node used without a host. The host's
    /// CONFIRM still works.
    Own,
}

/// What the application hands its node's start.
pub struct App {
    /// The `self_check` decision as a function; it answers after `exts`.
    pub self_check: fn() -> bool,
    /// The application's extensions (`extensions![..]`: libraries, then
    /// its own), after the board package in the chain: they observe every
    /// `app.*` event and answer the decision points.
    pub exts: Option<UserExtensions>,
    pub confirm: Confirm,
}

impl App {
    /// An application with a self-check only: no extensions, the host
    /// confirms.
    pub const fn new(self_check: fn() -> bool) -> Self {
        App {
            self_check,
            exts: None,
            confirm: Confirm::ByHost,
        }
    }
}

/// The chain a node's start hands to `dagon_agent::Agent::new`, and the
/// application's read access to its published events. The order is the
/// one every layer uses: publisher, board package, then the user part.
/// `SelfCheck` comes after the application's extensions because it never
/// abstains: first, it would hide their `self_check` answers.
pub fn node_chain(
    events: &'static mut Events,
    board: Option<BoardExtension>,
    exts: Option<UserExtensions>,
    self_check: fn() -> bool,
    now_us: fn() -> u64,
    watchdog: Watchdog,
) -> (NodeChain, AppEvents) {
    let chain = compose(
        Layer::App,
        events,
        board,
        Chain::new(exts, SelfCheck(self_check)),
        Bounds::new(now_us, watchdog),
    );
    let reader = chain.reader();
    (chain, reader)
}

/// The last steps of a node's start, after `app.watchdog_taken` and
/// before its links serve (design 1.3): the board's `BoardBringUp` port,
/// then `app.bring_up` naming the board, so the application's own steps
/// (extensions on `app.bring_up`) run after the board's; a board without
/// one runs nothing and emits nothing. Then, with [`Confirm::Own`] and the
/// image on trial, the trial is confirmed. Returns whether the image is
/// still on trial (for the watchdog guard).
pub fn finish_start<S: SlotStore, X: AppChain>(
    agent: &mut Agent<S, X>,
    board: Option<(&str, &mut dyn BoardBringUp)>,
    confirm: Confirm,
) -> bool {
    if let Some((id, port)) = board {
        port.bring_up();
        agent.bring_up(id);
    }
    if confirm == Confirm::Own && agent.images().on_trial() {
        // A refusal (unhealthy) is `app.confirm_refused`; the trial goes on.
        let _ = agent.confirm_own();
    }
    agent.images().on_trial()
}
