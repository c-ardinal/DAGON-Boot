//! The catalog types every machine's generated `CATALOG` is made of.

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
    pub doc: &'static str,
}

impl Entry {
    /// Live observation by extensions in the same image.
    pub const fn live(&self) -> bool {
        self.observe.live
    }
}

/// One machine: its states and its transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub machine: &'static str,
    pub layer: Layer,
    pub doc: &'static str,
    pub initial: &'static str,
    pub states: &'static [&'static str],
    pub entries: &'static [Entry],
}

impl Catalog {
    pub fn entry(&self, code: u8) -> Option<&'static Entry> {
        self.entries.iter().find(|e| e.code == code)
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
