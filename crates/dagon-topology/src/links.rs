//! Which link kinds each MCU's firmware implements, up (towards the PC or
//! the 親機) and down (to its 子機). The same table as the MCU packages'
//! `LINK_KINDS` (targets/mcu/<mcu>, "Links"): a kind missing here is
//! accepted in a 構成 but reported as not available yet (V-24), and
//! `dagon gen` refuses to generate it.

use crate::schema::LinkKind;

/// Direction of a hop as seen from one MCU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Towards the PC (a 親機's uplink) or the 親機 (a 子機's link).
    Up,
    /// From a 親機 to its 子機.
    Down,
}

/// Link kinds `mcu`'s firmware implements in direction `dir`; empty for an
/// MCU it does not know.
pub fn available(mcu: &str, dir: Dir) -> &'static [LinkKind] {
    use LinkKind::*;
    match (mcu, dir) {
        ("rp2040", Dir::Up) => &[UsbCdc],
        ("rp2040", Dir::Down) => &[I2c, Uart],
        ("stm32f401re", Dir::Up) => &[I2c],
        ("esp32s3", Dir::Up) => &[Uart],
        _ => &[],
    }
}

/// Whether `mcu` knows of any link at all (else other rules report it).
pub fn known(mcu: &str) -> bool {
    !available(mcu, Dir::Up).is_empty() || !available(mcu, Dir::Down).is_empty()
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
}
