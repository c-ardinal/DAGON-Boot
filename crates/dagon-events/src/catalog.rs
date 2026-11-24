//! The catalog types every machine's generated `CATALOG` is made of.

use crate::event::Layer;

/// How an event is observed. Every kind has at least one path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observe {
    /// By extensions in the same image, as it happens.
    pub live: bool,
    /// Re-emitted by the app's agent at start, from the boot record or the
    /// state log, with `replayed: true`.
    pub replay: bool,
    /// By the host: replies, VERSION, the serve stream, `--events`, actions.
    pub host: bool,
}

impl Observe {
    pub const NONE: Observe = Observe {
        live: false,
        replay: false,
        host: false,
    };

    pub const fn any(self) -> bool {
        self.live || self.replay || self.host
    }
}

/// Further attributes of a catalog entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// A boot action program may run on this event.
    pub action: bool,
    /// No longer emitted; kept until the next `event` number.
    pub retired: bool,
}

impl Flags {
    pub const NONE: Flags = Flags {
        action: false,
        retired: false,
    };
}

/// One payload field and its bytes in the owned form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub ty: &'static str,
    /// Bytes in [`crate::EventRecord::payload`], in field order.
    pub bytes: usize,
}

/// One transition and the event it emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub layer: Layer,
    pub machine: &'static str,
    /// The event's kind within its layer, snake case (`update_offered`).
    pub kind: &'static str,
    /// The kind's number within its layer ([`crate::EventRecord::kind`]).
    pub code: u8,
    /// The transition method.
    pub trigger: &'static str,
    /// The states it leaves from; `*` is any state.
    pub from: &'static [&'static str],
    pub to: &'static str,
    pub payload: &'static [Field],
    pub observe: Observe,
    pub flags: Flags,
    /// The transition's doc; empty without [`crate::CATALOG_DOCS`].
    pub doc: &'static str,
}

impl Entry {
    /// Live observation by extensions in the same image.
    pub const fn live(&self) -> bool {
        self.observe.live
    }
}

/// One machine: its states and its transitions.
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    pub machine: &'static str,
    pub layer: Layer,
    /// The machine's doc; empty without [`crate::CATALOG_DOCS`].
    pub doc: &'static str,
    pub initial: &'static str,
    pub states: &'static [&'static str],
    pub entries: &'static [Entry],
    /// The machine's `REPLAY` ([`Replay`]).
    #[doc(hidden)]
    pub __replay: Replay,
    /// The machine's generated transition table, by state index and
    /// event code ([`Catalog::leaves`]).
    #[doc(hidden)]
    pub __transits: fn(u8, u8) -> Option<bool>,
}

/// The machines one crate declares: the `MACHINES` static
/// [`crate::state_machine!`] defines at the crate's root, and only there.
#[derive(Debug)]
pub struct Machines {
    krate: &'static str,
    catalogs: &'static [Catalog],
}

impl Machines {
    /// Used by [`crate::state_machine!`] only.
    #[doc(hidden)]
    pub const fn __new(krate: &'static str, catalogs: &'static [Catalog]) -> Self {
        Machines { krate, catalogs }
    }

    /// The Cargo package that declares them.
    pub const fn krate(&self) -> &'static str {
        self.krate
    }

    pub const fn catalogs(&self) -> &'static [Catalog] {
        self.catalogs
    }

    /// The entry of kind `code` in `layer`, for a display or a tool that
    /// names an event (codes are unique within a layer).
    pub fn entry(&self, layer: Layer, code: u8) -> Option<&'static Entry> {
        self.catalogs
            .iter()
            .filter(|c| c.layer == layer)
            .find_map(|c| c.entry(code))
    }
}

/// A machine's record decoder (its generated `REPLAY`): decodes a record
/// of the machine into its payload and hands it to the callback; `false`
/// when the record is not the machine's, is not replayable, or does not
/// decode. [`crate::Composed::replay`] takes the decoders of the machines
/// it reads, so an image links no catalog to replay.
pub type Replay = fn(&crate::EventRecord, &mut dyn FnMut(&dyn crate::Payload)) -> bool;

impl Catalog {
    pub fn entry(&self, code: u8) -> Option<&'static Entry> {
        self.entries.iter().find(|e| e.code == code)
    }

    /// Whether the transition emitting kind `code` leaves the state at
    /// index `state` of `states`, read from the machine's generated
    /// transition table (`State::leaves`); `None` for a state or code the
    /// machine does not have.
    pub fn leaves(&self, state: u8, code: u8) -> Option<bool> {
        (self.__transits)(state, code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_codes_round_trip_and_are_distinct() {
        for l in Layer::ALL {
            assert_eq!(Layer::from_code(l.code()), Some(l));
        }
        assert_eq!(Layer::from_code(4), None);
    }
}
