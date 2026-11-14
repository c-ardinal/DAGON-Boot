//! RP2040 flash layout as XIP addresses (flash offset + `XIP_BASE`), the
//! address space every `Flash` here uses and image headers' `load_addr`
//! names. Every address and length comes from the MCU definition the node
//! build is given (the generated constants: `crate::slots`,
//! `crate::STAGE0_REGION` and the others); nothing here is a second copy.

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogLayout;
use dagon_format::header::HEADER_SIZE;
use dagon_proto::payload::target;

/// Base of the default flash map (XIP).
pub const XIP_BASE: u32 = crate::DEFAULT_FLASH.0;
/// Smallest flash this layout fits: the default flash map (the board
/// package names the real size).
pub const MIN_FLASH_SIZE: usize = crate::DEFAULT_FLASH.1 as usize;
/// The erase unit of the MCU's first erase region (`[execution]
/// erase_regions`).
pub const SECTOR: u32 = crate::ERASE_REGIONS[0].unit;

/// boot2 (256 B) + stage0.
pub const STAGE0: u32 = crate::STAGE0_REGION.0;
pub const STAGE0_SIZE: u32 = crate::STAGE0_REGION.1;
/// The second stage the boot ROM loads (RP2040 datasheet section 2.8.1.3).
pub const BOOT2_LEN: u32 = 0x100;
pub const BL_LOG: [u32; 2] = crate::BOOTCORE_LOG_REGION.0;
pub const APP_LOG: [u32; 2] = crate::APP_LOG_REGION.0;
/// Boot core slot starts (the definition's `bootcore` slots, A first):
/// stage0 starts one, the agent writes a new core into another.
pub const BL_SLOT: [u32; crate::slots::BOOTCORE.len()] = starts(crate::slots::BOOTCORE);
/// Display copy of the topology (the definition's topology slot layout,
/// unsigned).
pub const TOPOLOGY: u32 = topology().0;
pub const TOPOLOGY_SIZE: u32 = topology().1;

/// Inline image header at the slot start; the body (vector table) follows.
pub const HEADER_LEN: u32 = HEADER_SIZE as u32;

pub const LOG_FACE_SIZE: u32 = crate::APP_LOG_REGION.1;

/// The watchdog period of an image on trial (`[watchdog] trial_ms`) or a
/// confirmed one (`confirmed_ms`).
pub const fn watchdog_ms(on_trial: bool) -> u32 {
    if on_trial {
        crate::TRIAL_WDT_MS
    } else {
        crate::CONFIRMED_WDT_MS
    }
}

/// RAM every image links against: up to the boot record.
pub const RAM: u32 = crate::RAM_REGION.0;
pub const RAM_LEN: u32 = crate::BOOT_RECORD_REGION.0 - RAM;

/// The boot core log for dagon-core (boot core records only).
pub const BL_LOG_LAYOUT: LogLayout = LogLayout {
    faces: BL_LOG,
    face_len: crate::BOOTCORE_LOG_REGION.1,
};

/// The app log and slots for dagon-core.
pub const APP_LOG_LAYOUT: LogLayout = LogLayout {
    faces: APP_LOG,
    face_len: LOG_FACE_SIZE,
};
/// Each with its own length: `(start, length)`, A first.
pub const APP_SLOTS: SlotLayout = SlotLayout {
    slots: &crate::slots::APP,
};

/// Body capacity of a slot of `slot_size` bytes.
pub const fn body_capacity(slot_size: u32) -> u32 {
    slot_size - HEADER_LEN
}

/// Rounds `len` up to whole sectors.
pub const fn sectors(len: u32) -> u32 {
    len.div_ceil(SECTOR) * SECTOR
}

/// The start of each (start, length) slot.
const fn starts<const N: usize>(slots: [(u32, u32); N]) -> [u32; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = slots[i].0;
        i += 1;
    }
    out
}

/// The topology slot layout: start, capacity.
const fn topology() -> (u32, u32) {
    let mut i = 0;
    while i < crate::SLOT_LAYOUTS.len() {
        let l = &crate::SLOT_LAYOUTS[i];
        if l.target == target::TOPOLOGY {
            return (l.start, l.capacity);
        }
        i += 1;
    }
    panic!("the RP2040 definition gives the topology a slot layout")
}
