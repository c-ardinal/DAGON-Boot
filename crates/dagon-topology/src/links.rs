//! Which link kinds each MCU's firmware implements, up (towards the PC or
//! the node nearer the PC) and down (to the node at the other end of the link), from the MCU definitions' `[links]`
//! (kept equal to the MCU packages' `LINK_KINDS` by a test): a kind missing
//! there is accepted in a 構成 but reported as not available (V-24),
//! and `dagon gen` refuses to generate it.

use crate::defs::Registry;
use crate::schema::LinkKind;

/// Direction of a hop as seen from one MCU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Towards the PC (a coordinator's uplink) or the node nearer the PC.
    Up,
    /// From the node nearer the PC to the node at the other end of the link.
    Down,
}

/// Link kinds `mcu`'s firmware implements in direction `dir`, from the
/// definitions of `reg`; empty for an MCU it does not know.
pub fn available_in<'r>(reg: &'r Registry, mcu: &str, dir: Dir) -> &'r [LinkKind] {
    reg.mcu(mcu).map_or(&[], |m| match dir {
        Dir::Up => &m.links.up,
        Dir::Down => &m.links.down,
    })
}

/// Whether `mcu`'s firmware runs `kind` in either direction: an end of a
/// link, which has no direction, is judged by this.
pub fn runs_in(reg: &Registry, mcu: &str, kind: LinkKind) -> bool {
    [Dir::Up, Dir::Down]
        .into_iter()
        .any(|d| available_in(reg, mcu, d).contains(&kind))
}

/// Whether `mcu` knows of any link at all in `reg` (else other rules
/// report it).
pub fn known_in(reg: &Registry, mcu: &str) -> bool {
    !available_in(reg, mcu, Dir::Up).is_empty() || !available_in(reg, mcu, Dir::Down).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available(mcu: &str, dir: Dir) -> &'static [LinkKind] {
        available_in(Registry::builtin(), mcu, dir)
    }

    fn known(mcu: &str) -> bool {
        known_in(Registry::builtin(), mcu)
    }

    #[test]
    fn the_sample_hops_are_available_and_radios_are_not() {
        assert!(available("rp2040", Dir::Up).contains(&LinkKind::UsbCdc));
        assert!(available("rp2040", Dir::Down).contains(&LinkKind::I2c));
        assert!(available("stm32f401re", Dir::Up).contains(&LinkKind::I2c));
        assert!(available("esp32s3", Dir::Up).contains(&LinkKind::Uart));
        for m in ["rp2040", "stm32f401re", "esp32s3"] {
            for d in [Dir::Up, Dir::Down] {
                for k in [
                    LinkKind::Ble,
                    LinkKind::WifiTcp,
                    LinkKind::Ethernet,
                    LinkKind::Can,
                ] {
                    assert!(!available(m, d).contains(&k), "{m} {d:?} {k:?}");
                }
            }
        }
        assert!(!known("unknown-mcu"));
    }

    /// The firmware's `LINK_KINDS` is its MCU definition's `[links]`: each
    /// package builds the table from what `dagon-mcu-build` writes of it, and
    /// holds no second list.
    #[test]
    fn the_firmware_link_kinds_come_from_the_definitions() {
        const FW: [&str; 3] = [
            include_str!("../../../targets/mcu/rp2040/src/kinds.rs"),
            include_str!("../../../targets/mcu/stm32f401re/src/links.rs"),
            include_str!("../../../targets/mcu/esp32s3/src/links.rs"),
        ];
        for src in FW {
            assert!(
                src.contains("kinds_of_definition(crate::LINKS_UP, crate::LINKS_DOWN)"),
                "{src}"
            );
            assert!(!src.contains("KindInfo {"), "{src}");
        }
    }
}
