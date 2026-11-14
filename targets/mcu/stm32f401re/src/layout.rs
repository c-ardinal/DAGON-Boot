//! STM32F401RE flash layout (512 KiB: sectors 16K x4, 64K, 128K x3; the
//! topology's `f401-default` flash map) and the boot settings. Every
//! address, length and period comes from the MCU definition the node build
//! is given (the generated constants: `crate::slots`, `crate::APP_LOG_REGION`
//! and the others); nothing here is a second copy. The table is the
//! built-in definition's:
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
use dagon_format::header::HEADER_SIZE;
use dagon_port::FlashRegion;

/// Stage0: where the core fetches its reset vector.
pub const STAGE0: u32 = crate::STAGE0_REGION.0;
pub const STAGE0_SIZE: u32 = crate::STAGE0_REGION.1;

/// State log faces A and B. One log holds the APP and the boot core
/// records, so the definition gives both logs the same faces.
pub const LOG: LogLayout = {
    let (app, core) = (crate::APP_LOG_REGION, crate::BOOTCORE_LOG_REGION);
    assert!(
        app.0[0] == core.0[0] && app.0[1] == core.0[1] && app.1 == core.1,
        "the STM32F401RE package keeps app and boot core records in one log:          [layout] app_log and bootcore_log must agree"
    );
    LogLayout {
        faces: app.0,
        face_len: app.1,
    }
};

/// Boot core slots (start, length), A first, header inline at the slot
/// start, body (vector table) at +512.
pub const CORE_SLOTS: [(u32, u32); crate::slots::BOOTCORE.len()] = crate::slots::BOOTCORE;

/// The APP slots `(start, length)`, A first, each with its own length,
/// header inline at the slot start.
pub const SLOTS: SlotLayout = SlotLayout {
    slots: &crate::slots::APP,
};

/// The MCU id in image headers (`wire_id`).
pub const MCU_ID: u16 = crate::MCU_WIRE_ID;
/// Smallest flash the layout fits: the default flash map.
pub const MIN_FLASH_SIZE: usize = crate::DEFAULT_FLASH.1 as usize;
/// RAM every image links against.
pub const RAM: u32 = crate::RAM_REGION.0;
pub const RAM_LEN: u32 = crate::RAM_REGION.1;
pub const HEADER_LEN: u32 = HEADER_SIZE as u32;

pub use crate::{CONFIRMED_WDT_MS, TRIAL_WDT_MS};

/// The conformance build's scratch (`[conformance] scratch`); no image or
/// log is there.
pub const SCRATCH_SECTOR: u32 = scratch().0;
pub const SCRATCH_SECTOR_LEN: u32 = scratch().1;

/// Regions DAGON may erase and write (the flash port's geometry, and the
/// layout a conformance build declares): the log faces, the scratch, each
/// boot core slot and each APP slot, each erased in the unit of the
/// definition's erase region it starts in. Reads are allowed anywhere in
/// flash; stage0's region is never erased or written through the port.
pub const FLASH_REGIONS: [FlashRegion; 2 + CORE_SLOTS.len() + crate::slots::APP.len()] = {
    let mut out = [region(0, 0); 2 + CORE_SLOTS.len() + crate::slots::APP.len()];
    assert!(
        LOG.faces[1] == LOG.faces[0] + LOG.face_len,
        "the state log faces are adjacent"
    );
    out[0] = region(LOG.faces[0], 2 * LOG.face_len);
    out[1] = region(SCRATCH_SECTOR, SCRATCH_SECTOR_LEN);
    let mut i = 0;
    while i < CORE_SLOTS.len() {
        out[2 + i] = region(CORE_SLOTS[i].0, CORE_SLOTS[i].1);
        i += 1;
    }
    let mut j = 0;
    while j < crate::slots::APP.len() {
        out[2 + i + j] = region(crate::slots::APP[j].0, crate::slots::APP[j].1);
        j += 1;
    }
    out
};

/// A writable region erased in the unit of the erase region it starts in.
const fn region(start: u32, len: u32) -> FlashRegion {
    FlashRegion {
        start,
        len,
        erase_size: erase_unit(start),
        write_size: 4,
        erased_value: 0xFF,
    }
}

/// The erase unit at `addr` (`[execution] erase_regions`); the first
/// region's for an address outside them all.
const fn erase_unit(addr: u32) -> u32 {
    let r = &crate::ERASE_REGIONS;
    let mut i = 0;
    while i < r.len() {
        if addr >= r[i].start && addr - r[i].start < r[i].len {
            return r[i].unit;
        }
        i += 1;
    }
    r[0].unit
}

const fn scratch() -> (u32, u32) {
    match crate::CONFORM_SCRATCH {
        Some(s) => s,
        None => panic!("the STM32F401RE definition gives a conformance scratch"),
    }
}
