//! Resolves the verifying key from `DAGON_PUBKEY`: the path to the public key
//! file from `dagon key gen` (as for the other boards) or the 64 hex digits
//! themselves. The crate reads the result as `DAGON_PUBKEY_HEX`.

use std::{env, fs};

fn is_key_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    let Ok(value) = env::var("DAGON_PUBKEY") else {
        println!(
            "cargo:warning=DAGON_PUBKEY is unset: built with the public RFC 8032 test key \
             (TEST KEY), which accepts images signed with a published secret"
        );
        return;
    };
    let hex = if is_key_hex(value.trim()) {
        value.trim().to_string()
    } else {
        println!("cargo:rerun-if-changed={value}");
        let text = fs::read_to_string(&value)
            .unwrap_or_else(|e| panic!("DAGON_PUBKEY: cannot read key file {value}: {e}"));
        text.trim().to_string()
    };
    assert!(
        is_key_hex(&hex),
        "DAGON_PUBKEY: expected 64 hex digits (or a file holding them)"
    );
    println!("cargo:rustc-env=DAGON_PUBKEY_HEX={hex}");
}
