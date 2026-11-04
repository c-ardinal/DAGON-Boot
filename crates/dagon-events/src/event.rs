//! Events as observers see them, and their owned record form.

use core::any::Any;
use core::fmt;

use crate::catalog::{Entry, Layer};
use crate::PAYLOAD_BYTES;

/// An event payload: one of the payload types [`crate::state_machine!`]
/// generates. The trait is sealed (its supertrait cannot be named outside
/// this crate) and a payload is built only in the crate that declares its
/// machine. What reaches a layer's extensions and published state is
/// what that layer's chain delivers: its machines' transitions and its
/// replay, both held by the layer start ([`crate::Composed`]). The macro
/// marks its payloads through a hidden path that any crate invoking the
/// macro can also name; such code is in the trusted base, and an
/// extension that did so would still hold no chain to deliver into.
///
/// It has no `Debug` supertrait: an `&dyn Payload` would carry the
/// formatter in its vtable into every image that delivers events, the boot
/// cores included, whose flash budget has no room for it. Each generated
/// payload type is `Debug` on its own.
///
/// ```compile_fail,E0277
/// #[derive(Debug)]
/// struct Forged;
/// impl dagon_events::Payload for Forged {
///     fn entry(&self) -> &'static dagon_events::Entry {
///         &dagon_events::decisions::ExtensionOverran::ENTRY
///     }
///     fn compact(&self) -> [u8; 16] {
///         [0; 16]
///     }
/// }
/// ```
pub trait Payload: crate::sealed::SealedPayload + Any {
    /// The catalog entry of the transition that emits it.
    fn entry(&self) -> &'static Entry;
    /// The owned form ([`EventRecord::payload`]): the fields in order.
    fn compact(&self) -> [u8; PAYLOAD_BYTES];
}

/// An event as extensions observe it: a borrowed payload and whether it
/// happened now or is replayed from the boot record or the state log.
/// Only the layer's composed chain makes one, from a delivered payload.
#[derive(Clone, Copy)]
pub struct Event<'a> {
    payload: &'a dyn Payload,
    replayed: bool,
}

impl fmt::Debug for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = self.entry();
        f.debug_struct("Event")
            .field("layer", &e.layer)
            .field("kind", &e.kind)
            .field("payload", &self.payload.compact())
            .field("replayed", &self.replayed)
            .finish()
    }
}

impl<'a> Event<'a> {
    /// An event that happened now.
    pub(crate) fn live(payload: &'a dyn Payload) -> Self {
        Event {
            payload,
            replayed: false,
        }
    }

    /// An event re-emitted from what an earlier stage recorded.
    pub(crate) fn replayed(payload: &'a dyn Payload) -> Self {
        Event {
            payload,
            replayed: true,
        }
    }

    pub fn entry(&self) -> &'static Entry {
        self.payload.entry()
    }

    pub fn layer(&self) -> Layer {
        self.entry().layer
    }

    /// The kind within its layer (`confirmed` for `app.confirmed`).
    pub fn kind(&self) -> &'static str {
        self.entry().kind
    }

    pub fn is_replayed(&self) -> bool {
        self.replayed
    }

    /// The payload as its own type, when it is that type.
    pub fn get<P: Payload>(&self) -> Option<&'a P> {
        let any: &'a dyn Any = self.payload;
        any.downcast_ref()
    }

    /// The owned form, numbered `seq` by whoever keeps it (see
    /// [`EventRecord::seq`]).
    pub fn record(&self, seq: u32) -> EventRecord {
        let e = self.entry();
        EventRecord {
            layer: e.layer.code(),
            kind: e.code,
            flags: if self.replayed {
                EventRecord::REPLAYED
            } else {
                0
            },
            dropped: 0,
            seq,
            payload: self.payload.compact(),
        }
    }
}

/// The owned, bounded form of an event: what the [`crate::Recent`] ring
/// and the published state hold. No reference outlives the producer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventRecord {
    /// [`Layer::code`].
    pub layer: u8,
    /// [`Entry::code`] within the layer.
    pub kind: u8,
    /// [`EventRecord::REPLAYED`].
    pub flags: u8,
    /// Records dropped (oldest first) since the one before this,
    /// saturating.
    pub dropped: u8,
    /// One more than the previous record's, per publisher; wraps from
    /// `u32::MAX` to 0. Compare two with `b.seq.wrapping_sub(a.seq)`
    /// (serial number arithmetic): below 2^31 means `b` is newer.
    pub seq: u32,
    /// The payload fields in catalog order ([`crate::Field::bytes`]),
    /// zero-padded.
    pub payload: [u8; PAYLOAD_BYTES],
}

impl EventRecord {
    pub const REPLAYED: u8 = 1 << 0;
}

/// A transition called from a state it does not leave from. The state is
/// unchanged and nothing is emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub machine: &'static str,
    pub trigger: &'static str,
    pub state: &'static str,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} is not a transition from {}",
            self.machine, self.trigger, self.state
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_24_bytes() {
        assert_eq!(core::mem::size_of::<EventRecord>(), 24);
    }
}
