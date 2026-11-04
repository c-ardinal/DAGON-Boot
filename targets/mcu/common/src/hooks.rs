//! The app layer's composed chain a node's start builds: the published
//! events, and the app's CONFIRM self-check as its extension.

use dagon_events::{
    compose, Bounds, Composed, Ctx, Extension, Health, Layer, Nop, Published, TrialInfo, Watchdog,
};

/// Recent events a node keeps for its display and log tasks.
pub const RECENT: usize = 8;

/// A node's published events.
pub type Events = Published<(), RECENT>;

/// A node's chain: its published events, no board extension, the app's
/// self-check; decisions timed with the MCU package's microsecond clock.
pub type NodeChain = Composed<&'static Events, Nop, SelfCheck, fn() -> u64>;

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

/// The chain a node's start hands to `dagon_agent::Agent::new`.
pub fn node_chain(
    events: &'static mut Events,
    self_check: fn() -> bool,
    now_us: fn() -> u64,
    watchdog: Watchdog,
) -> NodeChain {
    compose(
        Layer::App,
        events,
        Nop,
        SelfCheck(self_check),
        Bounds::new(now_us, watchdog),
    )
}
