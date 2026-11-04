//! RP2040 flash layout as XIP addresses (flash offset + `XIP_BASE`), the
//! address space every `Flash` here uses and image headers' `load_addr`
//! names. Fixed: never move these.

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;

pub const XIP_BASE: u32 = 0x1000_0000;
/// Smallest flash this layout fits (the board package names the real size).
pub const MIN_FLASH_SIZE: usize = 4 * 1024 * 1024;
pub const SECTOR: u32 = 4096;

const fn xip(offset: u32) -> u32 {
    XIP_BASE + offset
}

/// boot2 (256 B) + stage0.
pub const STAGE0: u32 = xip(0x00_0000);
pub const STAGE0_SIZE: u32 = 16 * 1024;
pub const BOOT2_LEN: u32 = 0x100;
pub const BL_LOG: [u32; 2] = [xip(0x00_4000), xip(0x00_5000)];
pub const APP_LOG: [u32; 2] = [xip(0x00_6000), xip(0x00_7000)];
/// Boot core slots A and B (the MCU definition's `bootcore` slots): stage0
/// starts one, the agent writes a new core into the other.
pub const BL_SLOT: [u32; 2] = [xip(0x00_8000), xip(0x02_8000)];
pub const BL_SLOT_SIZE: u32 = 128 * 1024;
pub const APP_SLOT: [u32; 2] = [xip(0x04_8000), xip(0x14_8000)];
pub const APP_SLOT_SIZE: u32 = 1024 * 1024;
/// Display copy of the topology (first 4 KiB of the topology store, unsigned).
pub const TOPOLOGY: u32 = xip(0x24_8000);
pub const TOPOLOGY_SIZE: u32 = 4096;

/// Inline image header at the slot start; the body (vector table) follows.
pub const HEADER_LEN: u32 = 512;

pub const LOG_FACE_SIZE: u32 = SECTOR;

/// Watchdog period for every boot (below the RP2040 limit of 8.388 s).
pub const WDT_MS: u32 = 8_000;

/// RAM every image links against.
pub const RAM: u32 = 0x2000_0000;
pub const RAM_LEN: u32 = 256 * 1024;

/// The boot core log for dagon-core (boot core records only).
pub const BL_LOG_LAYOUT: LogLayout = LogLayout {
    faces: BL_LOG,
    face_len: LOG_FACE_SIZE,
};

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
