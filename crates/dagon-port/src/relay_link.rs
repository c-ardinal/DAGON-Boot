//! A link a node relays over: one entry of its MCU definition's
//! `[[relay_link]]`. The definitions loader is the one place that reads it
//! from TOML, and an MCU package's build generates the same values as
//! constants, so the PC and the firmware use one table.

use crate::LinkKind;

/// How a relay link names the node it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Addressing {
    /// A 7-bit bus address: the address the topology gives on the reached
    /// end of the link.
    SevenBit,
    /// One fixed address: the medium has one peer and no addresses.
    Fixed(u8),
}

impl Addressing {
    /// Whether `addr` names a node on such a link.
    pub const fn accepts(self, addr: u8) -> bool {
        match self {
            Addressing::SevenBit => addr < 0x80,
            Addressing::Fixed(a) => addr == a,
        }
    }
}

/// One link a node relays over (RELAY `link`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayLinkDef {
    /// Its id in RELAY, 1 to 254, unique within the MCU.
    pub id: u8,
    /// The index of the `[[link_port]]` it runs on: a topology link end at
    /// this node with this port index runs over it.
    pub port: u8,
    /// The port's link kind.
    pub kind: LinkKind,
    pub addressing: Addressing,
    /// Largest payload it carries.
    pub max_payload: u16,
}

impl RelayLinkDef {
    /// Whether a RELAY over this link may go to `addr` with an inner frame
    /// of `inner_len` bytes.
    pub const fn fits(&self, addr: u8, inner_len: usize) -> bool {
        self.addressing.accepts(addr)
            && inner_len <= dagon_proto::frame_len(self.max_payload as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUS: RelayLinkDef = RelayLinkDef {
        id: 7,
        port: 0,
        kind: LinkKind::I2c,
        addressing: Addressing::SevenBit,
        max_payload: 32,
    };

    #[test]
    fn a_link_takes_the_addresses_and_frames_it_carries() {
        let frame = dagon_proto::frame_len(32);
        assert!(BUS.fits(0x7F, frame));
        assert!(!BUS.fits(0x80, frame));
        assert!(!BUS.fits(0x42, frame + 1));
        let stream = RelayLinkDef {
            addressing: Addressing::Fixed(0xFF),
            ..BUS
        };
        assert!(stream.fits(0xFF, frame));
        assert!(!stream.fits(0x42, frame));
    }
}
