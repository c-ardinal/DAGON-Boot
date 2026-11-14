//! DAGON's flash layout on the ESP32-S3: the partition table the MCU
//! definition names (`partitions.csv` next to this crate's `Cargo.toml`,
//! the table that gets flashed), as the build generates it
//! (`crate::PARTITION_TABLE`); nothing here is a second copy. It needs a
//! chip with at least [`MIN_FLASH_SIZE`]; the board crate states the actual
//! size (espflash `--flash-size`).

use dagon_port::FlashRegion;

/// Smallest flash the table fits: its end, rounded up to a power of two.
pub const MIN_FLASH_SIZE: u32 = table_end().next_power_of_two();
/// The erase unit of the MCU's first erase region (`[execution]
/// erase_regions`).
pub const ERASE_UNIT: u32 = crate::ERASE_REGIONS[0].unit;
pub const WRITE_UNIT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub name: &'static str,
    pub ptype: u8,
    pub subtype: u8,
    pub offset: u32,
    pub size: u32,
}

pub const OTADATA: Partition = named("otadata");
pub const FACTORY: Partition = named("factory");
pub const OTA_0: Partition = named("ota_0");
pub const OTA_1: Partition = named("ota_1");
/// Custom data partition: detached image headers.
pub const DAGON: Partition = named("dagon");

/// Every partition in table order.
pub const PARTITIONS: [Partition; crate::PARTITION_TABLE.len()] = {
    let t = &crate::PARTITION_TABLE;
    let mut out = [named("factory"); crate::PARTITION_TABLE.len()];
    let mut i = 0;
    while i < t.len() {
        out[i] = row(i);
        i += 1;
    }
    out
};

const fn row(i: usize) -> Partition {
    let (name, ptype, subtype, offset, size) = crate::PARTITION_TABLE[i];
    Partition {
        name,
        ptype,
        subtype,
        offset,
        size,
    }
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The partition named `name` in the table.
const fn named(name: &str) -> Partition {
    let mut i = 0;
    while i < crate::PARTITION_TABLE.len() {
        if same(crate::PARTITION_TABLE[i].0, name) {
            return row(i);
        }
        i += 1;
    }
    panic!(
        "the partition table lacks a partition DAGON uses (otadata, factory, ota_0, ota_1, dagon)"
    )
}

const fn table_end() -> u32 {
    let mut end = 0;
    let mut i = 0;
    while i < crate::PARTITION_TABLE.len() {
        let (_, _, _, offset, size) = crate::PARTITION_TABLE[i];
        if offset + size > end {
            end = offset + size;
        }
        i += 1;
    }
    end
}

/// Boot-selection slot numbers (`dagon_port::BootSelectV`).
pub const SLOT_A: u8 = 0;
pub const SLOT_B: u8 = 1;
pub const SLOT_FACTORY: u8 = 2;

/// App partition of a boot-selection slot.
pub fn app_partition(slot: u8) -> Option<Partition> {
    match slot {
        SLOT_A => Some(OTA_0),
        SLOT_B => Some(OTA_1),
        SLOT_FACTORY => Some(FACTORY),
        _ => None,
    }
}

/// Slot whose body starts at `offset` (the app's slots and the agent's,
/// in the definition's slot layouts).
pub fn slot_at(offset: u32) -> Option<u8> {
    use dagon_proto::payload::target;
    crate::SLOT_LAYOUTS
        .iter()
        .find(|l| (l.target == target::APP || l.target == target::AGENT) && l.start == offset)
        .map(|l| l.slot)
}

const fn region(p: Partition) -> FlashRegion {
    FlashRegion {
        start: p.offset,
        len: p.size,
        erase_size: ERASE_UNIT,
        write_size: WRITE_UNIT,
        erased_value: 0xFF,
    }
}

/// What DAGON may erase and write: both OTA slots, the dagon partition and
/// factory (the recovery agent's partition, written only under a confirmed
/// OTA app). The 2nd stage, partition table and otadata stay out of reach.
pub const REGIONS: [FlashRegion; 4] =
    [region(OTA_0), region(OTA_1), region(DAGON), region(FACTORY)];

/// The region that wholly contains `addr..addr + len`.
pub fn region_for(addr: u32, len: u32) -> Option<&'static FlashRegion> {
    let end = addr.checked_add(len)?;
    REGIONS
        .iter()
        .find(|r| addr >= r.start && end <= r.start + r.len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_are_ordered_aligned_and_fit() {
        // The partition table itself sits at 0x8000..0x9000.
        let mut end = 0x9000;
        for p in PARTITIONS {
            assert!(p.offset >= end, "{} overlaps", p.name);
            assert_eq!(p.offset % ERASE_UNIT, 0, "{}", p.name);
            assert_eq!(p.size % ERASE_UNIT, 0, "{}", p.name);
            if p.ptype == 0x00 {
                assert_eq!(p.offset % 0x10000, 0, "{} not 64 KiB aligned", p.name);
            }
            end = p.offset + p.size;
        }
        assert!(end <= MIN_FLASH_SIZE);
    }

    #[test]
    fn slots_map_both_ways() {
        for s in [SLOT_A, SLOT_B, SLOT_FACTORY] {
            assert_eq!(slot_at(app_partition(s).unwrap().offset), Some(s));
        }
        assert_eq!(app_partition(3), None);
        assert_eq!(slot_at(DAGON.offset), None);
        assert_eq!(slot_at(0), None);
    }

    #[test]
    fn region_lookup_bounds() {
        assert_eq!(region_for(OTA_0.offset, OTA_0.size), Some(&REGIONS[0]));
        assert_eq!(
            region_for(OTA_1.offset + OTA_1.size - 4, 4),
            Some(&REGIONS[1])
        );
        assert_eq!(region_for(DAGON.offset + 0x1000, 512), Some(&REGIONS[2]));
        // ota_0 and ota_1 are adjacent: a range across both is still refused.
        assert_eq!(region_for(OTA_1.offset - 4, 8), None);
        assert_eq!(region_for(FACTORY.offset, 4), Some(&REGIONS[3]));

        assert_eq!(region_for(OTADATA.offset, 32), None);
        assert_eq!(region_for(DAGON.offset + DAGON.size, 4), None);
        assert_eq!(region_for(u32::MAX - 1, 4), None);
    }
}
