//! `hal` and `image-stage0`: embeds the verifying key (`DAGON_PUBKEY`).
//! Always: generates the layout constants from the MCU definition the node
//! build is given (`dagon_mcu_build::definition`). For a target build,
//! writes the final binary's memory.x from that definition:
//! - `image-stage0`: boot2 (`[stage0] head_bytes`) + stage0 in the region
//!   the definition gives (`[layout] stage0`, `[stage0] region_bytes`), with the link-time
//!   check that the image leaves 1/16 of it free;
//! - `image-bootcore`: body of boot core slot A (B with `slot-b`), after the
//!   512-byte header;
//! - otherwise an app: body of APP slot A (B with `slot-b`), after the
//!   header.

use std::env;

use dagon_format::header::HEADER_SIZE;

/// Places boot2, the second stage the RP2040 boot ROM loads from the start
/// of flash, in the stage0 head (`[stage0] head_bytes`).
const BOOT2_SECTION: &str =
    "SECTIONS {\n  .boot2 ORIGIN(HEAD) :\n  {\n    KEEP(*(.boot2));\n  } > HEAD\n}\n";

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_TARGET_TAG");
    let def = dagon_mcu_build::definition();
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
    // Every image's RAM ends at the boot record (`[boot.record]`), which
    // survives the resets between them.
    let (ram, _) = def.ram().expect("the RP2040 definition gives its RAM");
    let (record, _) = def
        .boot_record()
        .expect("the RP2040 definition gives its boot record");
    let ram_len = record - ram;
    if feature("IMAGE_STAGE0") {
        dagon_mcu_build::stage0_memory_x(BOOT2_SECTION, ram, ram_len);
        return;
    }
    let image = if feature("IMAGE_BOOTCORE") {
        "bootcore"
    } else {
        "app"
    };
    let (start, size) = def.slot(image, feature("SLOT_B"));
    let header = HEADER_SIZE as u32;
    // The examples are apps linked here (a generated app's
    // .cargo/config.toml gives the same arguments).
    println!("cargo:rustc-link-arg-examples=--nmagic");
    println!("cargo:rustc-link-arg-examples=-Tlink.x");
    dagon_mcu_build::memory_x(start + header, size - header, ram, ram_len);
}
