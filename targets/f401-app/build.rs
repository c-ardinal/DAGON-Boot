//! Links the app into the body of APP-A or APP-B: the slot's first 512 bytes
//! are the inline image header, so the vector table starts at slot + 0x200
//! (512-byte aligned, as VTOR needs).

use std::{env, fs, path::PathBuf};

fn main() {
    let slot = if env::var_os("CARGO_FEATURE_SLOT_B").is_some() {
        0x0804_0000u32
    } else {
        0x0802_0000u32
    };
    let memory = format!(
        "MEMORY\n{{\n  FLASH : ORIGIN = {:#010x}, LENGTH = 128K - 0x200\n  RAM   : ORIGIN = 0x20000000, LENGTH = 96K\n}}\n",
        slot + 0x200
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), memory).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
}
