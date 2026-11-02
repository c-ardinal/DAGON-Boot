//! Build-script helpers shared by the DAGON MCU packages: the verifying key
//! embedded at build time, and the linker memory map of an APP slot.

use std::{env, fs, path::PathBuf};

/// RFC 8032 section 7.1 TEST 1 public key (public test data).
pub const TEST_PUBKEY_HEX: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"))
}

/// Writes `$OUT_DIR/pubkey.rs` with `PUBKEY: [u8; 32]` and `TEST_KEY: bool`.
///
/// `DAGON_PUBKEY` is the public key file from `dagon key gen` or its 64 hex
/// digits. Unset, the build fails unless `DAGON_ALLOW_TEST_KEY=1`, which
/// selects the public RFC 8032 test key (with a build warning).
pub fn embed_pubkey() {
    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    println!("cargo:rerun-if-env-changed=DAGON_ALLOW_TEST_KEY");
    let hex = match env::var("DAGON_PUBKEY") {
        Ok(v) if is_key_hex(v.trim()) => v.trim().to_string(),
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("DAGON_PUBKEY: cannot read key file {path}: {e}"))
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
    assert!(
        is_key_hex(&hex),
        "DAGON_PUBKEY: expected 64 hex digits (or a file holding them)"
    );
    let bytes: Vec<String> = (0..32)
        .map(|i| format!("{:#04x}", u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()))
        .collect();
    fs::write(
        out_dir().join("pubkey.rs"),
        format!(
            "/// Verifying key embedded at build time (`DAGON_PUBKEY`).\n\
             pub const PUBKEY: [u8; 32] = [{}];\n\
             /// Built with the public test key: anyone can sign images for it.\n\
             pub const TEST_KEY: bool = {};\n",
            bytes.join(", "),
            hex.eq_ignore_ascii_case(TEST_PUBKEY_HEX)
        ),
    )
    .unwrap();
}

fn is_key_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Writes `$OUT_DIR/memory.x` for an image whose vector table starts at
/// `body` (flash) with `len` bytes, RAM at `ram`/`ram_len`.
pub fn memory_x(body: u32, len: u32, ram: u32, ram_len: u32) {
    write_memory_x(&format!(
        "MEMORY
{{
  FLASH : ORIGIN = {body:#010x}, LENGTH = {len:#x}
           RAM   : ORIGIN = {ram:#010x}, LENGTH = {ram_len:#x}
}}
"
    ));
}

/// Writes `text` as `$OUT_DIR/memory.x` and puts it on the linker search
/// path of the final binary (cortex-m-rt's link.x includes it).
pub fn write_memory_x(text: &str) {
    let out = out_dir();
    fs::write(out.join("memory.x"), text).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
}
