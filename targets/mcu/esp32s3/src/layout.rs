//! DAGON's flash layout on the ESP32-S3. `partitions.csv` (next to this
//! crate's `Cargo.toml`) is the table that gets flashed; a host test keeps
//! the two in step. It needs a chip with at least [`MIN_FLASH_SIZE`]; the
//! board crate states the actual size (espflash `--flash-size`).

use dagon_port::FlashRegion;

/// Smallest flash the table fits (it ends at 0x720000).
pub const MIN_FLASH_SIZE: u32 = 8 * 1024 * 1024;
pub const ERASE_UNIT: u32 = 4096;
pub const WRITE_UNIT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub name: &'static str,
    pub ptype: u8,
    pub subtype: u8,
    pub offset: u32,
    pub size: u32,
}

pub const OTADATA: Partition = Partition {
    name: "otadata",
    ptype: 0x01,
    subtype: 0x00,
    offset: 0x00D000,
    size: 0x2000,
};
pub const FACTORY: Partition = Partition {
    name: "factory",
    ptype: 0x00,
    subtype: 0x00,
    offset: 0x010000,
    size: 0x100000,
};
pub const OTA_0: Partition = Partition {
    name: "ota_0",
    ptype: 0x00,
    subtype: 0x10,
    offset: 0x110000,
    size: 0x300000,
};
pub const OTA_1: Partition = Partition {
    name: "ota_1",
    ptype: 0x00,
    subtype: 0x11,
    offset: 0x410000,
    size: 0x300000,
};
/// Custom data partition (type 0x40): detached image headers.
pub const DAGON: Partition = Partition {
    name: "dagon",
    ptype: 0x40,
    subtype: 0x00,
    offset: 0x710000,
    size: 0x10000,
};

/// Every partition in table order.
pub const PARTITIONS: [Partition; 5] = [OTADATA, FACTORY, OTA_0, OTA_1, DAGON];

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

/// Slot whose app partition starts at `offset`.
pub fn slot_at(offset: u32) -> Option<u8> {
    [SLOT_A, SLOT_B, SLOT_FACTORY]
        .into_iter()
        .find(|&s| app_partition(s).is_some_and(|p| p.offset == offset))
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

    const CSV: &str = include_str!("../partitions.csv");

    fn num(s: &str) -> u32 {
        let (digits, mul) = match s.as_bytes().last() {
            Some(b'K') => (&s[..s.len() - 1], 1024),
            Some(b'M') => (&s[..s.len() - 1], 1024 * 1024),
            _ => (s, 1),
        };
        let v = match digits.strip_prefix("0x") {
            Some(hex) => u32::from_str_radix(hex, 16).unwrap(),
            None => digits.parse().unwrap(),
        };
        v * mul
    }

    fn ptype(s: &str) -> u8 {
        match s {
            "app" => 0x00,
            "data" => 0x01,
            _ => num(s) as u8,
        }
    }

    fn subtype(s: &str) -> u8 {
        match s {
            "factory" | "ota" => 0x00,
            "ota_0" => 0x10,
            "ota_1" => 0x11,
            _ => num(s) as u8,
        }
    }

    #[test]
    fn matches_partitions_csv() {
        let rows: Vec<Partition> = CSV
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                Partition {
                    name: PARTITIONS.iter().find(|p| p.name == f[0]).unwrap().name,
                    ptype: ptype(f[1]),
                    subtype: subtype(f[2]),
                    offset: num(f[3]),
                    size: num(f[4]),
                }
            })
            .collect();
        assert_eq!(rows, PARTITIONS);
    }

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
