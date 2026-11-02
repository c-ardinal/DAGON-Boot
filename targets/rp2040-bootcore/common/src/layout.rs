//! Hub flash layout (offsets from the start of flash; XIP maps it at
//! `XIP_BASE`). Fixed: never move these.

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;

pub const XIP_BASE: u32 = 0x1000_0000;
pub const FLASH_SIZE: usize = 4 * 1024 * 1024;
pub const SECTOR: u32 = 4096;

/// boot2 (256 B) + stage0.
pub const STAGE0: u32 = 0x00_0000;
pub const BL_LOG: [u32; 2] = [0x00_4000, 0x00_5000];
pub const APP_LOG: [u32; 2] = [0x00_6000, 0x00_7000];
/// Boot core slots. B is reserved (no boot core self-update).
pub const BL_SLOT: [u32; 2] = [0x00_8000, 0x02_8000];
pub const BL_SLOT_SIZE: u32 = 128 * 1024;
pub const APP_SLOT: [u32; 2] = [0x04_8000, 0x14_8000];
pub const APP_SLOT_SIZE: u32 = 1024 * 1024;
/// Display copy of the topology (first 4 KiB of the topology store, unsigned).
pub const TOPOLOGY: u32 = 0x24_8000;
pub const TOPOLOGY_SIZE: u32 = 4096;

/// Inline image header at the slot start; the body (vector table) follows.
pub const HEADER_LEN: u32 = 512;

pub const LOG_FACE_SIZE: u32 = SECTOR;

/// Trial boots before the boot core rolls back.
pub const MAX_ATTEMPTS: u8 = 3;
/// Watchdog period for every boot (below the RP2040 limit of 8.388 s).
pub const WDT_MS: u32 = 8_000;

/// Image `target_tag` of hub images (boot core and app).
pub const HUB_TAG: &str = "hub-rp2040-lcd";

/// The app log and slots for dagon-core.
pub const APP_LOG_LAYOUT: LogLayout = LogLayout {
    faces: APP_LOG,
    face_len: LOG_FACE_SIZE,
};
pub const APP_SLOTS: SlotLayout = SlotLayout {
    slots: APP_SLOT,
    slot_len: APP_SLOT_SIZE,
};

/// Body capacity of a slot of `slot_size` bytes.
pub const fn body_capacity(slot_size: u32) -> u32 {
    slot_size - HEADER_LEN
}

/// Rounds `len` up to whole sectors.
pub const fn sectors(len: u32) -> u32 {
    len.div_ceil(SECTOR) * SECTOR
}
