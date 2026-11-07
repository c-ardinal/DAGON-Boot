//! STM32F401RE flash layout (512 KiB: sectors 16K x4, 64K, 128K x3; the
//! topology's `f401-default` flash map) and the boot settings. Plain data:
//! this package's build.rs and the host test-image tool include this file.
//!
//! | Sector | Size | Holds |
//! |---|---|---|
//! | 0 | 16 KiB | stage0 (never updated: the reset vector is here) |
//! | 1, 2 | 16 KiB each | state log faces A and B (APP and boot core records) |
//! | 3 | 16 KiB | the conformance build's scratch |
//! | 4 | 64 KiB | boot core slot A |
//! | 5, 6 | 128 KiB each | APP slots A and B |
//! | 7 | 128 KiB | boot core slot B |

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;
use dagon_port::FlashRegion;

/// Stage0: all of sector 0, where the core fetches its reset vector.
pub const STAGE0: u32 = 0x0800_0000;
pub const STAGE0_SIZE: u32 = 0x4000;

/// State log faces A and B: sectors 1 and 2. One log holds the APP and the
/// boot core records (the small sectors leave no room for a second pair).
pub const LOG: LogLayout = LogLayout {
    faces: [0x0800_4000, 0x0800_8000],
    face_len: 0x4000,
};

/// Boot core slots A (sector 4) and B (sector 7): (start, length), header
/// inline at the slot start, body (vector table) at +512.
pub const CORE_SLOTS: [(u32, u32); 2] = [(0x0801_0000, 0x1_0000), (0x0806_0000, 0x2_0000)];

/// APP-A (sector 5) and APP-B (sector 6), header inline at the slot start.
pub const SLOTS: SlotLayout = SlotLayout {
    slots: [0x0802_0000, 0x0804_0000],
    slot_len: 0x2_0000,
};

pub const MCU_ID: u16 = dagon_format::header::mcu_id::STM32F401XE;
/// Smallest flash the layout fits (boot core B ends in sector 7).
pub const MIN_FLASH_SIZE: usize = 512 * 1024;
/// RAM every image links against.
pub const RAM: u32 = 0x2000_0000;
pub const RAM_LEN: u32 = 96 * 1024;
pub const HEADER_LEN: u32 = 512;

pub const TRIAL_WDT_MS: u32 = 8_000;
pub const CONFIRMED_WDT_MS: u32 = 8_000;

/// Sector 3: the conformance build's scratch (`[conformance] scratch` of
/// dagon-mcu.toml); no image or log is there.
pub const SCRATCH_SECTOR: u32 = 0x0800_C000;
pub const SCRATCH_SECTOR_LEN: u32 = 0x4000;

/// Regions DAGON may erase and write (the flash port's geometry, and the
/// layout a conformance build declares): the two log faces, the scratch
/// sector, boot core slot A, the two APP slots and boot core slot B.
/// Reads are allowed anywhere in flash; stage0's sector is never erased or
/// written through the port.
pub const FLASH_REGIONS: [FlashRegion; 5] = [
    FlashRegion {
        start: LOG.faces[0],
        len: 2 * LOG.face_len,
        erase_size: LOG.face_len,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: SCRATCH_SECTOR,
        len: SCRATCH_SECTOR_LEN,
        erase_size: SCRATCH_SECTOR_LEN,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: CORE_SLOTS[0].0,
        len: CORE_SLOTS[0].1,
        erase_size: CORE_SLOTS[0].1,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: SLOTS.slots[0],
        len: 2 * SLOTS.slot_len,
        erase_size: SLOTS.slot_len,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: CORE_SLOTS[1].0,
        len: CORE_SLOTS[1].1,
        erase_size: CORE_SLOTS[1].1,
        write_size: 4,
        erased_value: 0xFF,
    },
];
