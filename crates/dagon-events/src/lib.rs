//! DAGON lifecycle events and extensions.
//!
//! Every state change in DAGON is a transition of a machine declared with
//! [`state_machine!`]; the transition method is the only way to change the
//! state and it returns the transition's event. The machines' catalogs are
//! the event catalog (`docs/events.md` is generated from them). Code outside
//! DAGON observes events, answers the decision points and replaces parts of
//! DAGON through the ports of [`PORTS`], composed in one fixed order
//! ([`compose`]). Async display and log tasks read [`Published`] state.
#![no_std]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]

mod catalog;
mod compact;
mod event;
mod extension;
mod machine;
mod ports;
mod published;

pub use catalog::{Catalog, Entry, Field, Flags, Layer, Observe};
pub use compact::Compact;
pub use event::{Event, EventRecord, Payload, Refused};
pub use extension::{
    decide, Accept, Chain, Ctx, DecisionPoint, Delay, Extension, Health, Nop, RebootRequest,
    TrialInfo, UpdateOffer, DECISIONS, DECISION_BOUND_MS,
};
pub use ports::{Port, PortDefault, Service, Trust, PORTS};
pub use published::{compose, Latest, Published, Publisher, Recent, Snapshot, WATCHERS};

/// The `event` contract's number: the catalog kinds, their codes and the
/// meaning of their payloads and of [`EventRecord`]. Adding a kind or a
/// payload field keeps it; renaming, removing or changing a meaning moves
/// it. Readers ignore kinds and fields they do not know and refuse a
/// number they do not read.
pub const EVENT_FORMAT: u16 = 1;

/// Bytes of an event payload's owned form ([`EventRecord::payload`]).
pub const PAYLOAD_BYTES: usize = 16;

#[doc(hidden)]
pub mod __private {
    pub use crate::machine::{contains, snake, snake_len, unique_codes};
}
