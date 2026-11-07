//! Embeds the image verification key from `DAGON_PUBKEY`: the path to the
//! public key file from `dagon key gen` (as for the RP2040 and F401 builds),
//! or the 64 hex digits themselves. Unset, the build fails unless
//! `DAGON_ALLOW_TEST_KEY=1` opts in to the public RFC 8032 test key (then it
//! warns).
use std::{env, fs};

const TEST_KEY_HEX: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

fn is_key_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn main() {
    dagon_mcu_build::definition();
    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    println!("cargo:rerun-if-env-changed=DAGON_ALLOW_TEST_KEY");
    let hex = match env::var("DAGON_PUBKEY") {
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
            TEST_KEY_HEX.to_string()
        }
        Ok(v) if is_key_hex(v.trim()) => v.trim().to_string(),
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("DAGON_PUBKEY: cannot read {path}: {e}"));
            let hex = text.trim().to_string();
            assert!(
                is_key_hex(&hex),
                "DAGON_PUBKEY: {path} must hold 64 hex digits"
            );
            hex
        }
    };
    println!("cargo:rustc-env=DAGON_PUBKEY_HEX={hex}");
}
