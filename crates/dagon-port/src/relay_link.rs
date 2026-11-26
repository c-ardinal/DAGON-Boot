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

/// One node a relay recovers through its ROM loader: the relay link and
/// the address the loader answers on, and the relay's GPIOs on the node's
/// NRST and BOOT lines where the topology names them. An MCU package's
/// build generates them (`RECOVERY_TARGETS`) from the node's topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryTargetDef {
    /// Id of the relay link ([`RelayLinkDef::id`]).
    pub link: u8,
    /// The loader's 7-bit I2C address.
    pub rom_addr: u8,
    pub nrst: Option<u8>,
    pub boot: Option<u8>,
}

impl RecoveryTargetDef {
    /// Whether lines a board wires for a node on relay link `link`, on the
    /// GPIOs `nrst` and `boot`, are this target's: the link is its link,
    /// and each line the topology names is on that GPIO.
    pub fn wired_by(&self, link: u8, nrst: u8, boot: u8) -> bool {
        self.link == link
            && self.nrst.is_none_or(|p| p == nrst)
            && self.boot.is_none_or(|p| p == boot)
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
    /// The most bytes one transfer over this link carries: one frame of
    /// its largest payload. A relay's port and worker for the link hold
    /// this much, and on the recovery link it bounds I2C_RAW's data and
    /// read length.
    pub const fn buffer_len(&self) -> usize {
        dagon_proto::frame_len(self.max_payload as usize)
    }

    /// Whether a RELAY over this link may go to `addr` with an inner frame
    /// of `inner_len` bytes.
    pub const fn fits(&self, addr: u8, inner_len: usize) -> bool {
        self.addressing.accepts(addr) && inner_len <= self.buffer_len()
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

    /// Lines are a target's when they are wired for its link and on every
    /// GPIO the topology names; a target that names none takes the lines
    /// of its link.
    #[test]
    fn a_target_takes_the_lines_wired_for_its_link_and_gpios() {
        let named = RecoveryTargetDef {
            link: 1,
            rom_addr: 0x3b,
            nrst: Some(18),
            boot: Some(28),
        };
        assert!(named.wired_by(1, 18, 28));
        assert!(!named.wired_by(3, 18, 28));
        assert!(!named.wired_by(1, 26, 28));
        assert!(!named.wired_by(1, 18, 27));
        let unnamed = RecoveryTargetDef {
            nrst: None,
            boot: None,
            ..named
        };
        assert!(unnamed.wired_by(1, 26, 27));
        assert!(!unnamed.wired_by(3, 26, 27));
    }

    #[test]
    fn a_links_buffer_is_one_frame_of_its_largest_payload() {
        // Header (8) + payload + CRC (4).
        assert_eq!(BUS.buffer_len(), 8 + 32 + 4);
        let uart = RelayLinkDef {
            max_payload: 1024,
            ..BUS
        };
        assert_eq!(uart.buffer_len(), 1036);
        assert!(BUS.fits(0x42, BUS.buffer_len()));
        assert!(!BUS.fits(0x42, BUS.buffer_len() + 1));
    }

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
