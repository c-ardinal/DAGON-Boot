//! Picks the slot memory map (feature `slot-b`) and embeds the image
//! verification key from `DAGON_PUBKEY` (path to the hex public key file
//! from `dagon key gen`).
use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let map = if env::var_os("CARGO_FEATURE_SLOT_B").is_some() {
        "memory-b.x"
    } else {
        "memory-a.x"
    };
    fs::copy(map, out.join("memory.x")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory-a.x");
    println!("cargo:rerun-if-changed=memory-b.x");

    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    let path = env::var("DAGON_PUBKEY")
        .expect("set DAGON_PUBKEY to the public key file (dagon-signing.pub)");
    println!("cargo:rerun-if-changed={path}");
    let text = fs::read_to_string(&path).expect("read DAGON_PUBKEY file");
    let hex = text.trim();
    assert!(
        hex.len() == 64 && hex.is_ascii(),
        "DAGON_PUBKEY: expected 64 hex digits"
    );
    let bytes: Vec<String> = (0..32)
        .map(|i| {
            let b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex digit");
            format!("{b:#04x}")
        })
        .collect();
    fs::write(
        out.join("pubkey.rs"),
        format!("pub const PUBKEY: [u8; 32] = [{}];\n", bytes.join(", ")),
    )
    .unwrap();
}
