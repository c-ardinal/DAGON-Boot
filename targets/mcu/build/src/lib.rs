//! Build-script helpers shared by the DAGON MCU packages: the verifying key
//! embedded at build time, the constants the package's MCU definition
//! states, and the linker memory map of an APP slot.

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
        .map(|i| {
            format!(
                "{:#04x}",
                u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()
            )
        })
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

/// Writes `$OUT_DIR/definition.rs`: the constants the package's own MCU
/// definition (`dagon-mcu.toml` next to its Cargo.toml) states, so the boot
/// stage and the app layer read one value. Include it with
/// `include!(concat!(env!("OUT_DIR"), "/definition.rs"))`.
pub fn definition() {
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let path = dir.join("dagon-mcu.toml");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    fs::write(out_dir().join("definition.rs"), definition_rs(&text)).unwrap();
}

fn required<'a>(t: &'a toml::Table, table: &str, key: &str) -> &'a toml::Value {
    t.get(table)
        .and_then(|v| v.as_table())
        .and_then(|v| v.get(key))
        .unwrap_or_else(|| panic!("dagon-mcu.toml: [{table}] {key} is missing"))
}

fn int(v: &toml::Value, what: &str) -> u32 {
    v.as_integer()
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or_else(|| panic!("dagon-mcu.toml: {what} is not a u32"))
}

/// A hardware fact (`N`, `{ unverified = N }` or `"unmeasured"`): its value
/// and whether it is checked against its source.
fn fact(v: &toml::Value, what: &str) -> (Option<u32>, bool) {
    if v.as_str() == Some("unmeasured") {
        return (None, false);
    }
    match v.as_table().and_then(|t| t.get("unverified")) {
        Some(u) => (Some(int(u, what)), false),
        None => (Some(int(v, what)), true),
    }
}

/// The generated source for a definition's text.
pub fn definition_rs(text: &str) -> String {
    let t: toml::Table = text.parse().expect("dagon-mcu.toml");
    let mut out = String::new();
    let attempts = int(required(&t, "watchdog", "trial_attempts"), "trial_attempts");
    let attempts = u8::try_from(attempts).expect("[watchdog] trial_attempts fits a u8");
    out += "/// Boots the boot stage gives a pending image before it rolls back\n";
    out += "/// (`[watchdog] trial_attempts`).\n";
    out += &format!("pub const TRIAL_ATTEMPTS: u8 = {attempts};\n");

    let unit = int(required(&t, "execution", "erase_unit"), "erase_unit");
    out += "/// Largest unit one flash erase covers, bytes (`[execution] erase_unit`).\n";
    out += &format!("pub const ERASE_UNIT: u32 = {unit:#x};\n");

    let (masked, masked_known) = fact(required(&t, "execution", "masked_max_us"), "masked_max_us");
    out += "/// Longest DAGON's own code masks interrupts, us (`[execution]\n";
    out += "/// masked_max_us`); `None` while it is not measured.\n";
    out += &format!("pub const MASKED_MAX_US: Option<u32> = {masked:?};\n");
    out += "/// `MASKED_MAX_US` is checked against its source.\n";
    out += &format!("pub const MASKED_MAX_US_KNOWN: bool = {masked_known};\n");

    let (erase, erase_known) = fact(
        required(&t, "execution", "erase_unit_max_ms"),
        "erase_unit_max_ms",
    );
    let erase = erase.expect("[execution] erase_unit_max_ms needs a value");
    out += "/// Worst-case time of one such erase, ms (`[execution]\n";
    out += "/// erase_unit_max_ms`).\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS: u32 = {erase};\n");
    out += "/// `ERASE_UNIT_MAX_MS` is checked against the flash's datasheet.\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = {erase_known};\n");

    let scratch = t
        .get("conformance")
        .and_then(|c| c.get("scratch"))
        .and_then(|s| Some((s.get("at")?.as_integer()?, s.get("size")?.as_integer()?)));
    out += "/// Flash a conformance build may destroy (`[conformance] scratch`).\n";
    out += &match scratch {
        Some((at, size)) => {
            format!("pub const CONFORM_SCRATCH: Option<(u32, u32)> = Some(({at:#x}, {size:#x}));\n")
        }
        None => "pub const CONFORM_SCRATCH: Option<(u32, u32)> = None;\n".into(),
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "[watchdog]\ntrial_attempts = 3\n\
                       [execution]\nmasked_max_us = 50\nerase_unit = 0x1000\n\
                       erase_unit_max_ms = 400\n\
                       [conformance]\nscratch = { at = 0x100, size = 0x10 }\n";

    #[test]
    fn constants_come_from_the_definition() {
        let rs = definition_rs(DEF);
        assert!(rs.contains("pub const TRIAL_ATTEMPTS: u8 = 3;\n"), "{rs}");
        assert!(rs.contains("pub const ERASE_UNIT: u32 = 0x1000;\n"), "{rs}");
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 400;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = true;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const MASKED_MAX_US: Option<u32> = Some(50);\n"),
            "{rs}"
        );
        assert!(rs.contains("Some((0x100, 0x10))"), "{rs}");
    }

    /// Facts not yet established stay marked so in the constants.
    #[test]
    fn unverified_and_unmeasured_facts_are_marked() {
        let def = DEF
            .replace("masked_max_us = 50", "masked_max_us = \"unmeasured\"")
            .replace(
                "erase_unit_max_ms = 400",
                "erase_unit_max_ms = { unverified = 400 }",
            );
        let rs = definition_rs(&def);
        assert!(
            rs.contains("pub const MASKED_MAX_US: Option<u32> = None;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const MASKED_MAX_US_KNOWN: bool = false;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 400;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = false;\n"),
            "{rs}"
        );
    }
}
