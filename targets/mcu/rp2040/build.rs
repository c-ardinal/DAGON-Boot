//! `hal` and `image-stage0`: embeds the verifying key (`DAGON_PUBKEY`).
//! For a target build, writes the final binary's memory.x from the flash
//! layout (src/layout.rs, shared):
//! - `image-stage0`: boot2 (256 B) + stage0 in the region the MCU
//!   definition gives (`[layout] stage0`, `[stage0] region_bytes`), with the
//!   link-time check that the image leaves 1/16 of it free;
//! - `image-bootcore`: body of boot core slot A (B with `slot-b`), after the
//!   512-byte header;
//! - otherwise an app: body of APP slot A (B with `slot-b`), after the
//!   header.

use std::env;

#[allow(dead_code)]
#[path = "src/layout.rs"]
mod layout;
use layout::*;

const BOOT2_SECTION: &str =
    "SECTIONS {\n  .boot2 ORIGIN(HEAD) :\n  {\n    KEEP(*(.boot2));\n  } > HEAD\n}\n";

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_TARGET_TAG");
    dagon_mcu_build::definition();
    if feature("HAL") || feature("IMAGE_STAGE0") {
        dagon_mcu_build::embed_pubkey();
    }
    assert!(
        !(feature("IMAGE_STAGE0") && feature("IMAGE_BOOTCORE")),
        "enable at most one of image-stage0, image-bootcore"
    );
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    if feature("IMAGE_STAGE0") {
        dagon_mcu_build::stage0_memory_x(BOOT2_LEN, BOOT2_SECTION, RAM, RAM_LEN);
    } else if feature("IMAGE_BOOTCORE") {
        let body = BL_SLOT[usize::from(feature("SLOT_B"))] + HEADER_LEN;
        dagon_mcu_build::memory_x(body, BL_SLOT_SIZE - HEADER_LEN, RAM, RAM_LEN);
    } else {
        let slot = usize::from(feature("SLOT_B"));
        let body = APP_SLOT[slot] + HEADER_LEN;
        dagon_mcu_build::memory_x(body, APP_SLOT_SIZE - HEADER_LEN, RAM, RAM_LEN);
    }
}
