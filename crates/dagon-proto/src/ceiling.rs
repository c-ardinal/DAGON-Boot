//! How much payload an answer may carry.

/// The most payload bytes an answer may have: what the link carries, and
/// what the requester said it takes in its HELLO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceiling {
    /// Max payload of the link the request came over.
    pub link: u16,
    /// HELLO `max_payload` of the requester; `None` before its HELLO.
    pub peer: Option<u16>,
}

impl Ceiling {
    /// A link of `max` payload bytes whose requester has said nothing.
    pub const fn link(max: usize) -> Self {
        Ceiling {
            link: if max > u16::MAX as usize {
                u16::MAX
            } else {
                max as u16
            },
            peer: None,
        }
    }

    /// The same ceiling over a link that carries `max` payload bytes at
    /// most.
    pub fn within(self, max: usize) -> Self {
        Ceiling {
            link: self.link.min(Ceiling::link(max).link),
            ..self
        }
    }

    /// The same requester over a link that carries `max` payload bytes:
    /// for a node whose requests come over another link than the one this
    /// ceiling was made for (a relay's uplink).
    pub fn over(self, max: usize) -> Self {
        Ceiling {
            link: Ceiling::link(max).link,
            ..self
        }
    }

    /// The payload bytes an answer may have.
    pub fn bytes(self) -> usize {
        usize::from(self.peer.map_or(self.link, |p| p.min(self.link)))
    }
}

impl From<usize> for Ceiling {
    /// [`Ceiling::link`].
    fn from(max: usize) -> Self {
        Ceiling::link(max)
    }
}
