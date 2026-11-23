//! One interface for every DAGON hop: [`Link`] carries whole frames up (to
//! the PC or a parent node) or down (to a child node), whatever the medium.
//! Each MCU package lists the link kinds it offers in a [`KindInfo`] table
//! (`LINK_KINDS`) and constructs them as `links::<Kind>::new(..)`; a board
//! package names the pins (`uplink_<kind>()` / `downlink_<kind>()`).
//!
//! The kind names are the topology's, from the one list of link kinds
//! (`dagon_format::with_link_kinds!`). Kinds without an implementation
//! exist in the tables as names only ([`Support::NotYet`]); a project that asks for
//! one fails to build with a message naming the hop.

/// The frame link interface and the kind type are dagon-port's (one
/// interface for every MCU and for relays); `Kind` is
/// [`dagon_port::LinkKind`] under the name the MCU packages use.
pub use dagon_port::{Link, LinkError, LinkKind as Kind, Reconnect};

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

impl Support {
    pub const fn as_str(self) -> &'static str {
        match self {
            Support::Yes => "Yes",
            Support::NotYet => "NotYet",
            Support::No => "No",
        }
    }
}

/// Direction of a hop seen from this node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Towards the PC (the relaying node's PC link, or a node's link to the node that relays to it).
    Up,
    /// Towards a child node (only a relaying node has these).
    Down,
}

/// One row of an MCU package's `LINK_KINDS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindInfo {
    pub kind: Kind,
    pub up: Support,
    pub down: Support,
}

/// The README matrix of an MCU package's `LINK_KINDS`: one row per kind
/// of the list, absent kinds `No` (each package's README holds it, kept
/// equal by a test there).
pub fn write_matrix(table: &[KindInfo], out: &mut impl core::fmt::Write) -> core::fmt::Result {
    out.write_str("| Kind | Up | Down |\n|---|---|---|\n")?;
    for k in Kind::ALL {
        let up = support(table, *k, Dir::Up).as_str();
        let down = support(table, *k, Dir::Down).as_str();
        writeln!(out, "| `{}` | {up} | {down} |", k.name())?;
    }
    Ok(())
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
        use $crate::link::{support, Dir, Support};
        let s = support($table, $kind, Dir::$dir);
        if !matches!(s, Support::Yes) {
            let yet = matches!(s, Support::NotYet);
            $crate::__format::with_link_kinds!(
                [$crate::__link_requirement_message] ($mcu, $hop, $kind, yet)
            );
        }
    };
}

/// The compile-time message of a kind a hop cannot use, one arm per kind
/// of the list (`link_requirements!`).
#[doc(hidden)]
#[macro_export]
macro_rules! __link_requirement_message {
    (($mcu:literal, $hop:literal, $kind:ident, $yet:ident) $( $v:ident = $c:literal, $n:literal; )*) => {
        #[allow(unreachable_patterns)]
        match ($kind, $yet) {
            $(
                ($crate::link::Kind::$v, true) => {
                    panic!(concat!($hop, " ", $n, ": not available yet on ", $mcu))
                }
                ($crate::link::Kind::$v, false) => {
                    panic!(concat!($hop, " ", $n, ": not possible on ", $mcu))
                }
            )*
            _ => panic!(concat!($hop, ": a link kind unknown to ", $mcu)),
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

    link_requirements!("test-mcu", &TABLE);

    #[test]
    fn the_matrix_lists_every_kind_absent_ones_as_no() {
        extern crate std;
        let mut m = std::string::String::new();
        write_matrix(&TABLE, &mut m).unwrap();
        assert!(
            m.starts_with("| Kind | Up | Down |\n|---|---|---|\n"),
            "{m}"
        );
        assert!(m.contains("| `usb-cdc` | Yes | No |\n"), "{m}");
        assert!(m.contains("| `ble` | NotYet | NotYet |\n"), "{m}");
        assert!(m.contains("| `can` | No | No |\n"), "{m}");
        assert_eq!(m.lines().count(), 2 + Kind::ALL.len());
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
