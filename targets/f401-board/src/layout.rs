//! NUCLEO-F401RE flash layout (sectors 16K x4, 64K, 128K x3) and node
//! identity. Plain data: the host test-image tool includes this file too.

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;

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

pub const TARGET_TAG: [u8; 16] = dagon_format::header::target_tag("seg-f401-7seg");
pub const MCU_ID: u16 = dagon_format::header::mcu_id::STM32F401XE;
pub const MAX_ATTEMPTS: u8 = 3;
pub const TRIAL_WDT_MS: u32 = 4_000;
pub const CONFIRMED_WDT_MS: u32 = 8_000;
