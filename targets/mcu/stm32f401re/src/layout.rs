//! STM32F401RE flash layout (512 KiB: sectors 16K x4, 64K, 128K x3; the
//! topology's `f401-default` flash map) and the boot settings. Plain data:
//! this package's build.rs and the host test-image tool include this file.

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;
use dagon_port::FlashRegion;

/// Boot core: sectors 0-1. Its own signed header is the trailer at the end.
pub const BOOTCORE: u32 = 0x0800_0000;
pub const BOOTCORE_TRAILER: u32 = 0x0800_7E00;

/// State log faces A and B: sectors 2 and 3. Sector 4 stays unused.
pub const LOG: LogLayout = LogLayout {
    faces: [0x0800_8000, 0x0800_C000],
    face_len: 0x4000,
};

/// APP-A (sector 5) and APP-B (sector 6), header inline at the slot start.
/// Sector 7 stays reserved.
pub const SLOTS: SlotLayout = SlotLayout {
    slots: [0x0802_0000, 0x0804_0000],
    slot_len: 0x2_0000,
};

pub const MCU_ID: u16 = dagon_format::header::mcu_id::STM32F401XE;
/// Smallest flash the layout fits (APP-B ends in sector 6).
pub const MIN_FLASH_SIZE: usize = 512 * 1024;
/// RAM every image links against.
pub const RAM: u32 = 0x2000_0000;
pub const RAM_LEN: u32 = 96 * 1024;
pub const HEADER_LEN: u32 = 512;

pub const TRIAL_WDT_MS: u32 = 4_000;
pub const CONFIRMED_WDT_MS: u32 = 8_000;

/// Sector 4: the conformance build's scratch (`[conformance] scratch` of
/// dagon-mcu.toml); no image or log is there.
pub const SCRATCH_SECTOR: u32 = 0x0801_0000;
pub const SCRATCH_SECTOR_LEN: u32 = 0x1_0000;

/// Regions DAGON may erase and write (the flash port's geometry, and the
/// layout a conformance build declares): the two log faces, the scratch
/// sector and the two APP slots. Reads are allowed anywhere in flash; the
/// boot core sectors are never erased or written through the port.
pub const FLASH_REGIONS: [FlashRegion; 3] = [
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
        start: SLOTS.slots[0],
        len: 2 * SLOTS.slot_len,
        erase_size: SLOTS.slot_len,
        write_size: 4,
        erased_value: 0xFF,
    },
];
