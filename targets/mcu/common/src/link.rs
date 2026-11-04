//! One interface for every DAGON hop: [`Link`] carries whole frames up (to
//! the PC or a parent node) or down (to a child node), whatever the medium.
//! Each MCU package lists the link kinds it offers in a [`KindInfo`] table
//! (`LINK_KINDS`) and constructs them as `links::<Kind>::new(..)`; a board
//! package names the pins (`uplink_<kind>()` / `downlink_<kind>()`).
//!
//! The kind names are the 構成's: `usb-cdc`, `uart`, `i2c`, `ble`,
//! `wifi-tcp`, `ethernet`, `can`. Kinds without an implementation exist in
//! the tables as names only ([`Support::NotYet`]); a project that asks for
//! one fails to build with a message naming the hop.

/// Why a frame could not be read or written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// Nothing complete arrived within the link's own wait (a tick, not a
    /// failure: serve loops use it to drop abandoned transfers).
    Timeout,
    /// A frame longer than the link carries.
    Overflow,
    /// What arrived is not a valid frame (checked by the link).
    Corrupt,
    /// The peer went away or the medium failed.
    Closed,
}

/// What the peer sees when this end restarts (a reboot after an update).
/// The same split as dagon-host's `Reconnect` (`Reopen` / `Stays`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconnect {
    /// The link goes away with this end and the peer opens it again: a USB
    /// device re-enumerates; a TCP connection (wifi-tcp, ethernet) or a BLE
    /// connection is dropped by the restart and connected anew.
    Reenumerates,
    /// The link stays (UART, I2C, CAN): the peer keeps it open and simply
    /// sees silence while the node restarts.
    Persistent,
}

/// One end of a DAGON hop carrying whole frames (frozen wire format,
/// unchanged by the medium).
#[allow(async_fn_in_trait)]
pub trait Link {
    /// Largest payload one frame may carry on this link.
    fn max_payload(&self) -> usize;

    /// What the peer sees when this end restarts.
    fn reconnect(&self) -> Reconnect;

    /// The link's kind (its code goes into `app.link_up`, `app.request`).
    fn kind(&self) -> Kind;

    /// Waits for the next complete frame; the slice stays valid until the
    /// next call. [`LinkError::Timeout`] after the link's own tick with
    /// nothing complete.
    async fn read_frame(&mut self) -> Result<&[u8], LinkError>;

    /// Sends one frame.
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError>;

    /// Done with the request just read without answering it (not a valid
    /// request, or not for this node): the link must be ready for the next
    /// request (an I2C target stops NACKing; the request stays unanswered
    /// and the peer resends). Default: nothing to do.
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        Ok(())
    }

    /// Returns once everything written has left this end (the peer has
    /// read it, or it is on the wire), so a reset right after loses
    /// nothing. Default: at once.
    async fn flush(&mut self) -> Result<(), LinkError> {
        Ok(())
    }
}

/// A hop's medium, as the 構成 names it. The discriminant is the kind's
/// code in events; a new kind takes the next number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    UsbCdc = 1,
    Uart = 2,
    I2c = 3,
    Ble = 4,
    WifiTcp = 5,
    Ethernet = 6,
    Can = 7,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::UsbCdc,
        Kind::Uart,
        Kind::I2c,
        Kind::Ble,
        Kind::WifiTcp,
        Kind::Ethernet,
        Kind::Can,
    ];

    /// The kind's code in events (`app.link_up` `link`, `app.request`
    /// `via`).
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The 構成's name of the kind.
    pub const fn name(self) -> &'static str {
        match self {
            Kind::UsbCdc => "usb-cdc",
            Kind::Uart => "uart",
            Kind::I2c => "i2c",
            Kind::Ble => "ble",
            Kind::WifiTcp => "wifi-tcp",
            Kind::Ethernet => "ethernet",
            Kind::Can => "can",
        }
    }

    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// Whether an MCU package offers a kind in one direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Implemented: the project builds.
    Yes,
    /// A name only, not implemented yet: the project does not build.
    NotYet,
    /// Not possible on this MCU (no such peripheral, or not this way).
    No,
}

/// Direction of a hop seen from this node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Towards the PC (the 親機's PC link, a 子機's link to its parent).
    Up,
    /// Towards a child node (only a 親機 has these).
    Down,
}

/// One row of an MCU package's `LINK_KINDS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindInfo {
    pub kind: Kind,
    pub up: Support,
    pub down: Support,
}

/// `kind`'s support in `dir` according to `table` (`No` if absent).
pub const fn support(table: &[KindInfo], kind: Kind, dir: Dir) -> Support {
    let mut i = 0;
    while i < table.len() {
        if table[i].kind as u8 == kind as u8 {
            return match dir {
                Dir::Up => table[i].up,
                Dir::Down => table[i].down,
            };
        }
        i += 1;
    }
    Support::No
}

/// Generates an MCU package's `require_uplink` / `require_downlink` const
/// fns: evaluated in a project's `const _`, they stop the build with a
/// message naming the hop and kind when the kind is not available.
#[macro_export]
macro_rules! link_requirements {
    ($mcu:literal, $table:expr) => {
        /// Stops the build unless `kind` works as this node's uplink.
        pub const fn require_uplink(kind: $crate::link::Kind) {
            $crate::link_requirements!(@check $mcu, $table, kind, Up, "uplink");
        }

        /// Stops the build unless every kind works as a downlink.
        pub const fn require_downlinks(kinds: &[$crate::link::Kind]) {
            let mut i = 0;
            while i < kinds.len() {
                let kind = kinds[i];
                $crate::link_requirements!(@check $mcu, $table, kind, Down, "downlink");
                i += 1;
            }
        }
    };
    (@check $mcu:literal, $table:expr, $kind:ident, $dir:ident, $hop:literal) => {
        use $crate::link::{support, Dir, Kind, Support};
        let s = support($table, $kind, Dir::$dir);
        if !matches!(s, Support::Yes) {
            let yet = matches!(s, Support::NotYet);
            match ($kind, yet) {
                (Kind::UsbCdc, true) => panic!(concat!($hop, " usb-cdc: not available yet on ", $mcu)),
                (Kind::UsbCdc, false) => panic!(concat!($hop, " usb-cdc: not possible on ", $mcu)),
                (Kind::Uart, true) => panic!(concat!($hop, " uart: not available yet on ", $mcu)),
                (Kind::Uart, false) => panic!(concat!($hop, " uart: not possible on ", $mcu)),
                (Kind::I2c, true) => panic!(concat!($hop, " i2c: not available yet on ", $mcu)),
                (Kind::I2c, false) => panic!(concat!($hop, " i2c: not possible on ", $mcu)),
                (Kind::Ble, true) => panic!(concat!($hop, " ble: not available yet on ", $mcu)),
                (Kind::Ble, false) => panic!(concat!($hop, " ble: not possible on ", $mcu)),
                (Kind::WifiTcp, true) => panic!(concat!($hop, " wifi-tcp: not available yet on ", $mcu)),
                (Kind::WifiTcp, false) => panic!(concat!($hop, " wifi-tcp: not possible on ", $mcu)),
                (Kind::Ethernet, true) => panic!(concat!($hop, " ethernet: not available yet on ", $mcu)),
                (Kind::Ethernet, false) => panic!(concat!($hop, " ethernet: not possible on ", $mcu)),
                (Kind::Can, true) => panic!(concat!($hop, " can: not available yet on ", $mcu)),
                (Kind::Can, false) => panic!(concat!($hop, " can: not possible on ", $mcu)),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: [KindInfo; 2] = [
        KindInfo {
            kind: Kind::UsbCdc,
            up: Support::Yes,
            down: Support::No,
        },
        KindInfo {
            kind: Kind::Ble,
            up: Support::NotYet,
            down: Support::NotYet,
        },
    ];

    crate::link_requirements!("test-mcu", &TABLE);

    #[test]
    fn names_round_trip() {
        for k in Kind::ALL {
            assert_eq!(Kind::from_name(k.name()), Some(k));
        }
        assert_eq!(Kind::from_name("usb"), None);
    }

    #[test]
    fn codes_are_distinct_and_never_zero() {
        for (i, k) in Kind::ALL.iter().enumerate() {
            assert_ne!(k.code(), 0);
            assert!(Kind::ALL[i + 1..].iter().all(|o| o.code() != k.code()));
        }
    }

    #[test]
    fn support_by_direction() {
        assert_eq!(support(&TABLE, Kind::UsbCdc, Dir::Up), Support::Yes);
        assert_eq!(support(&TABLE, Kind::UsbCdc, Dir::Down), Support::No);
        assert_eq!(support(&TABLE, Kind::Can, Dir::Up), Support::No);
        require_uplink(Kind::UsbCdc);
        require_downlinks(&[]);
    }

    #[test]
    #[should_panic(expected = "uplink ble: not available yet on test-mcu")]
    fn missing_kind_names_the_hop() {
        require_uplink(Kind::Ble);
    }

    #[test]
    #[should_panic(expected = "downlink usb-cdc: not possible on test-mcu")]
    fn impossible_direction_names_the_hop() {
        require_downlinks(&[Kind::UsbCdc]);
    }
}
