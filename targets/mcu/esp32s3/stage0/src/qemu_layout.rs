//! The provisional layout of the QEMU test (feature `qemu-layout`): what
//! the ESP32-S3 definition does not give yet. C-559 U2 adds stage0's
//! definition (the boot core slot, the two logs) and U4 the retained
//! cell's place, after the hardware run U0b shows which RTC words an app
//! leaves alone and the RTC slow clock's rate. Until then these values
//! serve the emulator only, and stage0 builds with no other layout.

use dagon_core::boot::{places, Place};
use dagon_core::log::LogLayout;
use dagon_mcu_esp32s3::slots;

/// The boot core: the factory partition, its header detached into the
/// dagon partition (the definition's `agent` slot).
pub static CORE_PLACES: [Place; 1] = places(slots::AGENT_PLACES);

/// APP slot A (ota_0, the definition's `app` slot) and B (ota_1, as the
/// definition's `[[slot_layout]]` app b gives it).
pub static APP_PLACES: [Place; 2] = [
    places(slots::APP_PLACES)[0],
    Place::detached(0x71_1000, 0x41_0000, 0x30_0000),
];

/// The boot core log and the app log: two face pairs of 8 KiB in the
/// dagon partition (0x71_0000, 64 KiB), past the three header sectors.
pub static CORE_LOG: LogLayout = LogLayout {
    faces: [0x71_4000, 0x71_6000],
    face_len: 0x2000,
};
pub static APP_LOG: LogLayout = LogLayout {
    faces: [0x71_8000, 0x71_A000],
    face_len: 0x2000,
};

/// The retained cell: the last two words of RTC slow memory (8 KiB from
/// 0x5000_0000), kept across a main system reset.
pub const CELL: [u32; 2] = [0x5000_1FF8, 0x5000_1FFC];

/// The RTC slow clock: RC_SLOW_CLK's nominal rate.
pub const SLOW_HZ: u32 = 136_000;
