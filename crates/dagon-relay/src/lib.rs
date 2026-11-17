//! The relay: how a node reaches the nodes linked to it, independent of
//! any MCU.
//!
//! - [`serve`]: answers the requests of the PC (or whoever drives this
//!   node) over a [`dagon_port::Link`]: RELAY to a node over one of this
//!   node's links, SUBCTL and I2C_RAW for the ROM recovery path
//!   ([`Recovery`]), everything else to this node's own agent (SHOW
//!   included).
//! - [`run_link`]: one worker per link ([`dagon_port::NodeLink`]): it
//!   carries the requests [`serve`] hands it through a [`Port`]. A port and
//!   its worker each hold one frame of their link's largest payload, sized
//!   at build time ([`fits_link`]); longer data is refused with E_ARG.
//! - [`cmd`]: the argument rules of the relay commands (RELAY, I2C_RAW).
//!
//! The MCU package states the links its relay reaches nodes over, as its
//! definition's [`RelayLinkDef`] list (generated from `[[relay_link]]`),
//! its clock ([`RelayClock`]), its watchdog guard ([`Liveness`]) and this
//! node's own agent ([`OwnAgent`]); this crate names no link id or topology
//! interface of its own. Embassy task functions cannot be generic, so the
//! MCU package runs [`serve`] and each [`run_link`] inside its own tasks
//! with its concrete types.
#![no_std]
#![allow(async_fn_in_trait)]

pub mod cmd;
mod pc;
mod port;

use dagon_port::RelayLinkDef;

pub use pc::{serve, Recovery, Relay};
pub use port::{fits_link, run_link, LinkRoute, Port};

/// The relay's settings. The defaults are the relay's own values; an MCU
/// package or app changes only what its wiring needs.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// I2C address I2C_RAW talks to: the ROM boot loader of the node on
    /// the recovery lines (`0x39`, the STM32 system memory loader).
    pub rom_addr: u8,
    /// Bound on one I2C_RAW request at the link worker.
    pub raw_timeout_ms: u16,
    /// How much longer than a request's own timeout [`serve`] waits for
    /// the link worker's answer.
    pub port_margin_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            rom_addr: 0x39,
            raw_timeout_ms: 1_000,
            port_margin_ms: 1_000,
        }
    }
}

/// Time as the relay sees it: the MCU package's monotonic clock and
/// timer.
pub trait RelayClock {
    /// Milliseconds on a monotonic clock.
    fn now_ms(&self) -> u64;
    /// Returns once [`RelayClock::now_ms`] has reached `at_ms` (at once if it
    /// has).
    async fn wait_until(&self, at_ms: u64);
}

/// The watchdog guard's view of the relay's tasks: each announces its
/// work, so a stuck task stops the watchdog being fed.
pub trait Liveness {
    /// `task` started a piece of work with the default bound.
    fn busy(&self, task: usize);
    /// `task` started a piece of work that ends within `bound_ms`.
    fn busy_for(&self, task: usize, bound_ms: u64);
    /// `task` waits for something with no deadline of its own.
    fn idle(&self, task: usize);
    /// `task` made progress inside long work: renew its deadline and feed
    /// the watchdog.
    fn progress(&self, task: usize);
    /// Resets the node.
    fn reboot(&self) -> !;
}

/// This node's own agent as [`serve`] drives it: the requests the relay
/// does not answer itself, and what the agent reports about the PC link.
/// The MCU package implements it over its `dagon_agent::Agent`.
pub trait OwnAgent {
    /// The uplink of kind `link` (its code) came up; `addr` is this node's
    /// own address on it.
    fn link_up(&mut self, link: u8, addr: u8);
    /// That uplink went down.
    fn link_down(&mut self, link: u8, addr: u8);
    /// A request frame arrived over the uplink of kind `via`.
    fn request(&mut self, frame: &[u8], via: u8);
    /// The reply `frame` to that request left, `ms` after it arrived.
    fn replied(&mut self, frame: &[u8], ms: u32);
    /// Answers `request` for this node: the reply frame (none: no answer)
    /// and, when a reboot follows the reply, its delay in ms.
    fn handle(&mut self, request: &[u8]) -> Option<(&[u8], Option<u32>)>;
    /// The reset after a reboot request is next.
    fn before_reboot(&mut self);
}

/// The guard tasks of the relay. All of them run preempting the app's
/// tasks; the app's tasks are numbered from [`count`](task::count).
pub mod task {
    /// The task that runs [`serve`](crate::serve).
    pub const PC: usize = 0;

    /// The worker of the link at `index` in the node's link list.
    pub const fn link(index: usize) -> usize {
        PC + 1 + index
    }

    /// Guard tasks a relay over `links` links uses.
    pub const fn count(links: usize) -> usize {
        link(links)
    }
}

/// `f`, or `None` once `clock` reaches `deadline_ms` first.
async fn until<T: RelayClock, R>(
    clock: &T,
    deadline_ms: u64,
    f: impl core::future::Future<Output = R>,
) -> Option<R> {
    use embassy_futures::select::{select, Either};
    match select(f, clock.wait_until(deadline_ms)).await {
        Either::First(r) => Some(r),
        Either::Second(()) => None,
    }
}
