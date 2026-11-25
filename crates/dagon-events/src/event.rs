//! Events as observers see them, and their owned record form.

use core::any::Any;
use core::fmt;

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
/// It has no `Debug` supertrait and names no catalog entry: an `&dyn
/// Payload` would carry the formatter, or the catalog's names, in its
/// vtable into every image that delivers events, the boot cores included.
/// An event is its layer, its kind's code and its payload bytes; names are
/// looked up from the code only where a person reads them
/// ([`crate::Machines::entry`]). Each generated payload type is `Debug` on
/// its own.
///
/// ```compile_fail,E0277
/// #[derive(Debug)]
/// struct Forged;
/// impl dagon_events::Payload for Forged {
///     fn layer(&self) -> dagon_events::Layer {
///         dagon_events::Layer::App
///     }
///     fn kind(&self) -> u8 {
///         dagon_events::decisions::ExtensionOverran::ENTRY.code
///     }
///     fn compact(&self) -> [u8; 16] {
///         [0; 16]
///     }
/// }
/// ```
pub trait Payload: crate::sealed::SealedPayload + Any {
    /// The layer of the machine that emits it.
    fn layer(&self) -> Layer;
    /// Its kind's code within the layer ([`crate::Entry::code`]).
    fn kind(&self) -> u8;
    /// The owned form ([`EventRecord::payload`]): the fields in order.
    fn compact(&self) -> [u8; PAYLOAD_BYTES];
}

/// What is known of a payload's kind where its type is: a constant, so
/// asking costs a firmware image no table and no call.
pub trait Kind: Payload + Sized {
    /// Whether the catalog marks the kind `replay` and not `retired`.
    const REPLAYS: bool;
}

/// An event as extensions observe it: a borrowed payload and whether it
/// happened now or is replayed from the state log.
/// Only the layer's composed chain makes one, from a delivered payload.
#[derive(Clone, Copy)]
pub struct Event<'a> {
    payload: &'a dyn Payload,
    replayed: bool,
}

impl fmt::Debug for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("layer", &self.layer())
            .field("kind", &self.kind())
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

    pub fn layer(&self) -> Layer {
        self.payload.layer()
    }

    /// The kind's code within its layer ([`crate::Entry::code`]): compare
    /// it with a payload type's `ENTRY.code`, or read the payload with
    /// [`Event::get`].
    pub fn kind(&self) -> u8 {
        self.payload.kind()
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
        EventRecord {
            layer: self.layer().code(),
            kind: self.kind(),
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
/// unchanged and nothing is emitted. The transition is named by its
/// event's layer and code, the state by its index in the catalog's
/// `states` (the machine's `StateId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub layer: Layer,
    pub kind: u8,
    pub state: u8,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:#04x}: not a transition from state {}",
            self.layer.name(),
            self.kind,
            self.state
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

/// The layer an event, a port or a decision point belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Layer {
    /// The boot stages (stage0 and the boot core).
    Boot,
    /// The app's agent.
    App,
    /// The hub's PC link and sub-node links.
    Relay,
    /// `dagon-host`, driven by the CLI and `dagon serve`.
    Host,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Layer::Boot, Layer::App, Layer::Relay, Layer::Host];

    /// The prefix of the layer's event kinds (`app` in `app.confirmed`).
    pub const fn name(self) -> &'static str {
        match self {
            Layer::Boot => "boot",
            Layer::App => "app",
            Layer::Relay => "relay",
            Layer::Host => "host",
        }
    }

    /// The layer's number in [`crate::EventRecord::layer`].
    pub const fn code(self) -> u8 {
        match self {
            Layer::Boot => 0,
            Layer::App => 1,
            Layer::Relay => 2,
            Layer::Host => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.code() == code)
    }
}
