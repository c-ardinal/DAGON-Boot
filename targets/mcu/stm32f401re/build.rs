//! `hal`: embeds the verifying key (`DAGON_PUBKEY`). For a target build,
//! writes the final binary's memory.x from the flash layout (src/layout.rs,
//! shared):
//! - `image-stage0`: all of sector 0 (stage0 sits at the reset vector);
//! - `image-bootcore`: body of boot core slot A (B with `slot-b`), after
//!   the 512-byte header;
//! - otherwise an app: body of APP slot A (B with `slot-b`), after the
//!   512-byte header (512-byte aligned, as VTOR needs).

use std::env;

#[allow(dead_code)]
#[path = "src/layout.rs"]
mod layout;
use layout::*;

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_TARGET_TAG");
    dagon_mcu_build::definition();
    if feature("HAL") {
        dagon_mcu_build::embed_pubkey();
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    assert!(
        !(feature("IMAGE_STAGE0") && feature("IMAGE_BOOTCORE")),
        "enable at most one of image-stage0, image-bootcore"
    );
    if feature("IMAGE_STAGE0") {
        dagon_mcu_build::memory_x(STAGE0, STAGE0_SIZE, RAM, RAM_LEN);
    } else if feature("IMAGE_BOOTCORE") {
        let (start, len) = CORE_SLOTS[usize::from(feature("SLOT_B"))];
        dagon_mcu_build::memory_x(start + HEADER_LEN, len - HEADER_LEN, RAM, RAM_LEN);
    } else {
        let slot = usize::from(feature("SLOT_B"));
        let body = SLOTS.slots[slot] + HEADER_LEN;
        dagon_mcu_build::memory_x(body, SLOTS.slot_len - HEADER_LEN, RAM, RAM_LEN);
    }
}
