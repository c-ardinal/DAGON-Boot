//! `hal` and `image-stage0`: embeds the verifying key (`DAGON_PUBKEY`).
//! Always: generates the layout constants from the MCU definition the node
//! build is given (`dagon_mcu_build::definition`). For a target build,
//! writes the final binary's memory.x from that definition:
//! - `image-stage0`: the region the definition gives (`[layout] stage0`,
//!   `[stage0] region_bytes`: where stage0 sits at the reset vector), with
//!   the link-time check that the image leaves 1/16 of it free;
//! - `image-bootcore`: body of boot core slot A (B with `slot-b`), after
//!   the 512-byte header;
//! - otherwise an app: body of APP slot A (B with `slot-b`), after the
//!   512-byte header (512-byte aligned, as VTOR needs).

use std::env;

use dagon_format::header::HEADER_SIZE;

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_TARGET_TAG");
    let def = dagon_mcu_build::definition();
    if feature("HAL") || feature("IMAGE_STAGE0") {
        dagon_mcu_build::embed_pubkey();
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    assert!(
        !(feature("IMAGE_STAGE0") && feature("IMAGE_BOOTCORE")),
        "enable at most one of image-stage0, image-bootcore"
    );
    let (ram, ram_len) = def.ram().expect("the STM32F401RE definition gives its RAM");
    if feature("IMAGE_STAGE0") {
        dagon_mcu_build::stage0_memory_x(0, "", ram, ram_len);
        return;
    }
    let image = if feature("IMAGE_BOOTCORE") {
        "bootcore"
    } else {
        "app"
    };
    let (start, size) = def.slot(image, feature("SLOT_B"));
    let header = HEADER_SIZE as u32;
    dagon_mcu_build::memory_x(start + header, size - header, ram, ram_len);
}
