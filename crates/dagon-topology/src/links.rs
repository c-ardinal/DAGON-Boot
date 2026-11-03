//! Which link kinds each MCU's firmware implements, up (towards the PC or
//! the 親機) and down (to its 子機), from the MCU definitions' `[links]`
//! (kept equal to the MCU packages' `LINK_KINDS` by a test): a kind missing
//! there is accepted in a 構成 but reported as not available yet (V-24),
//! and `dagon gen` refuses to generate it.

use crate::defs::Registry;
use crate::schema::LinkKind;

/// Direction of a hop as seen from one MCU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Towards the PC (a 親機's uplink) or the 親機 (a 子機's link).
    Up,
    /// From a 親機 to its 子機.
    Down,
}

/// Link kinds `mcu`'s firmware implements in direction `dir` (builtin
/// definitions); empty for an MCU it does not know.
pub fn available(mcu: &str, dir: Dir) -> &'static [LinkKind] {
    available_in(Registry::builtin(), mcu, dir)
}

/// [`available`] from the definitions of `reg`.
pub fn available_in<'r>(reg: &'r Registry, mcu: &str, dir: Dir) -> &'r [LinkKind] {
    reg.mcu(mcu).map_or(&[], |m| match dir {
        Dir::Up => &m.links.up,
        Dir::Down => &m.links.down,
    })
}

/// Whether `mcu` knows of any link at all (else other rules report it).
pub fn known(mcu: &str) -> bool {
    known_in(Registry::builtin(), mcu)
}

/// [`known`] from the definitions of `reg`.
pub fn known_in(reg: &Registry, mcu: &str) -> bool {
    !available_in(reg, mcu, Dir::Up).is_empty() || !available_in(reg, mcu, Dir::Down).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desk_demo_hops_are_available_and_radios_are_not() {
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

    /// `[links]` of each MCU definition is the firmware's `LINK_KINDS`
    /// (the rows marked `Support::Yes`), as sets.
    #[test]
    fn definitions_match_the_firmware_link_kinds() {
        const FW: [(&str, &str); 3] = [
            (
                "rp2040",
                include_str!("../../../targets/mcu/rp2040/src/kinds.rs"),
            ),
            (
                "stm32f401re",
                include_str!("../../../targets/mcu/stm32f401re/src/links.rs"),
            ),
            (
                "esp32s3",
                include_str!("../../../targets/mcu/esp32s3/src/links.rs"),
            ),
        ];
        for (mcu, src) in FW {
            let (mut up, mut down) = (Vec::new(), Vec::new());
            for row in src.split("KindInfo {").skip(1) {
                let field = |name: &str| {
                    let at = row.find(&format!("{name}: ")).unwrap() + name.len() + 2;
                    row[at..].split(',').next().unwrap().trim().to_string()
                };
                let variant = field("kind");
                let kind = LinkKind::ALL
                    .into_iter()
                    .find(|k| format!("Kind::{k:?}") == variant)
                    .unwrap_or_else(|| panic!("{mcu}: {variant}"));
                if field("up") == "Support::Yes" {
                    up.push(kind);
                }
                if field("down") == "Support::Yes" {
                    down.push(kind);
                }
            }
            let sorted = |v: &[LinkKind]| {
                let mut v = v.to_vec();
                v.sort();
                v
            };
            assert_eq!(sorted(available(mcu, Dir::Up)), sorted(&up), "{mcu} up");
            assert_eq!(
                sorted(available(mcu, Dir::Down)),
                sorted(&down),
                "{mcu} down"
            );
        }
    }
}
