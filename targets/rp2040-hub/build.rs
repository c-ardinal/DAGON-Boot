//! Picks the slot memory map (feature `slot-b`) and embeds the image
//! verification key from `DAGON_PUBKEY` (path to the hex public key file
//! from `dagon key gen`; unset: the public test key, flagged as TEST_KEY).
use std::{env, fs, path::PathBuf};

/// RFC 8032 section 7.1 TEST 1 public key (public test data).
const TEST_PUBKEY_HEX: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

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
    println!("cargo:rerun-if-env-changed=DAGON_ALLOW_TEST_KEY");
    let hex = match env::var("DAGON_PUBKEY") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            fs::read_to_string(&path)
                .expect("read DAGON_PUBKEY file")
                .trim()
                .to_string()
        }
        Err(_) => {
            if env::var("DAGON_ALLOW_TEST_KEY").as_deref() != Ok("1") {
                panic!(
                    "DAGON_PUBKEY is unset: set it to the public key file from `dagon key gen` \
                     (or its 64 hex digits), or set DAGON_ALLOW_TEST_KEY=1 to build with the \
                     public RFC 8032 test key (TEST KEY)"
                );
            }
            println!(
                "cargo:warning=DAGON_PUBKEY is unset: built with the public RFC 8032 test key \
                 (TEST KEY), which accepts images signed with a published secret"
            );
            TEST_PUBKEY_HEX.to_string()
        }
    };
    let hex = hex.as_str();
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
        format!(
            "pub const PUBKEY: [u8; 32] = [{}];\n\
             /// Built with the public test key (TEST KEY).\n\
             #[allow(dead_code)]\n\
             pub const TEST_KEY: bool = {};\n",
            bytes.join(", "),
            hex.eq_ignore_ascii_case(TEST_PUBKEY_HEX)
        ),
    )
    .unwrap();
}
