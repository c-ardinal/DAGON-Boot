//! DAGON lifecycle events and extensions.
//!
//! Every state change in DAGON is a transition of a machine declared with
//! [`state_machine!`]; the transition method is the only way to change the
//! state and it delivers the transition's event to the layer's composed
//! chain before the state changes. The machines' catalogs are the event
//! catalog (`site/src/content/docs/reference/events.md` is generated from them). Code outside DAGON
//! observes events, answers the decision points within their bounds and
//! replaces parts of DAGON through the ports of [`PORTS`], composed in one
//! fixed order ([`compose`]). Async display and log tasks read
//! [`Published`] state.
#![no_std]
// One `unsafe fn`: `Device::from_definition`, whose contract is that only
// generated definition data calls it.
#![deny(unsafe_code)]
#![deny(unused_must_use)]

mod board;
mod catalog;
mod compact;
mod compose;
mod event;
mod extension;
mod machine;
mod ports;
mod published;

pub use board::{BoardBringUp, BusError, Device, Shared};
pub use catalog::{Catalog, Entry, Field, Flags, Layer, Machines, Observe};
pub use compact::Compact;
pub use compose::{
    compose, compose_boot, compose_unpublished, BootComposed, BootExtension, Bounds, Clock,
    Composed, Head, NoPublisher, Sink, Unreadable, Watchdog,
};
pub use event::{Event, EventRecord, Payload, Refused};
pub use extension::{
    Accept, Chain, Ctx, DecisionId, DecisionPoint, Delay, ExtId, Extension, Health, Nop,
    RebootRequest, TrialInfo, UpdateOffer, DECISIONS, DECISION_BOUND_MS,
};
pub use ports::{Port, PortDefault, Service, Trust, PORTS};
pub use published::{Latest, Published, Reader, Recent, Snapshot, WATCHERS};

/// The `event` contract's number: the catalog kinds, their codes and the
/// meaning of their payloads and of [`EventRecord`]. Adding a kind or a
/// payload field keeps it; renaming, removing or changing a meaning moves
/// it. Readers ignore kinds and fields they do not know and refuse a
/// number they do not read.
pub const EVENT_FORMAT: u16 = 1;

/// Bytes of an event payload's owned form ([`EventRecord::payload`]).
pub const PAYLOAD_BYTES: usize = 16;

state_machine! {
    /// Decision calls to extensions and their bound.
    pub machine decisions in App {
        initial Ready;
        states { Ready }
        transitions {
            /// An extension answered a decision point after
            /// `DECISION_BOUND_MS`; its answer was ignored.
            overran(id: ExtId, point: DecisionId, us: u32): Ready => Ready
                emits ExtensionOverran = 0xF0 { id: ExtId, point: DecisionId, us: u32 }
                observe(live);
        }
    }

    /// Boot-stage extension calls and their time limits.
    pub machine boot_extensions in Boot {
        initial Ready;
        states { Ready }
        transitions {
            /// An extension's handler ran longer than its `budget_us`
            /// (the manifest's `max_run_us`); the boot went on with the
            /// same decision.
            overran(id: ExtId, budget_us: u32, us: u32): Ready => Ready
                emits ExtensionOverran = 0xF0 { id: ExtId, budget_us: u32, us: u32 }
                observe(live);
        }
    }
}

/// Traits and types that cannot be named outside this crate: they seal
/// [`Payload`], [`Sink`] and [`Head`], and keep the composed chain's
/// per-extension calls ([`Extension`]'s hidden methods) DAGON's.
mod sealed {
    /// Only payloads [`crate::state_machine!`] generates.
    pub trait SealedPayload {}
    impl<T: crate::__private::Generated> SealedPayload for T {}

    pub trait SealedSink {}
    pub trait SealedHead {}

    /// Proof that the caller is this crate.
    #[derive(Clone, Copy, Debug)]
    pub struct Token(());

    impl Token {
        pub(crate) fn new() -> Self {
            Token(())
        }
    }
}

#[doc(hidden)]
pub mod __private {
    pub use crate::machine::{contains, listed, snake, snake_len, unique_codes};

    /// Implemented by [`crate::state_machine!`] for the payload types it
    /// generates; not for use outside the macro (see [`crate::Payload`]).
    pub trait Generated {}
}
