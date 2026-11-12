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
    for (key, name, what) in [
        (
            "trial_ms",
            "TRIAL_WDT_MS",
            "a trial boot (stage0 arms it before a core on trial)",
        ),
        ("confirmed_ms", "CONFIRMED_WDT_MS", "a confirmed boot"),
    ] {
        let ms = int(required(&t, "watchdog", key), key);
        out += &format!("/// Watchdog period of {what}, ms (`[watchdog] {key}`).\n");
        out += &format!("pub const {name}: u32 = {ms};\n");
    }

    let verifies = t
        .get("layout")
        .and_then(|l| l.get("stage0_verifies"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    out += "/// Stage0 checks a boot core header's signature too (`[layout]\n";
    out += "/// stage0_verifies`).\n";
    out += &format!("pub const STAGE0_VERIFIES: bool = {verifies};\n");

    let (masked, masked_known) = fact(required(&t, "execution", "masked_max_us"), "masked_max_us");
    out += "/// Longest DAGON's own code masks interrupts, us (`[execution]\n";
    out += "/// masked_max_us`); `None` while it is not measured.\n";
    out += &format!("pub const MASKED_MAX_US: Option<u32> = {masked:?};\n");
    out += "/// `MASKED_MAX_US` is checked against its source.\n";
    out += &format!("pub const MASKED_MAX_US_KNOWN: bool = {masked_known};\n");

    out += &erase_layout_rs(&t);

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

/// `[execution] erase_regions` (the MCU's default regions) and
/// `[[slot_layout]]` as constants: a slot store composes its erase layout
/// from the slot layouts and the regions its board returns. Also the
/// longest single erase of the defaults, with whether every erase time is
/// checked.
fn erase_layout_rs(t: &toml::Table) -> String {
    let regions = required(t, "execution", "erase_regions")
        .as_array()
        .expect("[execution] erase_regions is a list");
    let field = |r: &toml::Value, key: &str| -> toml::Value {
        r.get(key)
            .cloned()
            .unwrap_or_else(|| panic!("[execution] erase_regions: {key} is missing"))
    };
    let mut out = String::new();
    let (mut longest, mut known) = (0, true);
    out += "/// The MCU's erase regions (`[execution] erase_regions`): what a board\n";
    out += "/// without `[flash]` erases in (its `BoardDef` returns them).\n";
    out += &format!(
        "pub const ERASE_REGIONS: [dagon_port::timeout::EraseRegion; {}] = [\n",
        regions.len()
    );
    for r in regions {
        let (ms, ms_known) = fact(&field(r, "unit_max_ms"), "unit_max_ms");
        let ms = ms.expect("[execution] erase_regions: unit_max_ms needs a value");
        longest = longest.max(ms);
        known &= ms_known;
        out += &format!(
            "    dagon_port::timeout::EraseRegion {{ start: {:#x}, len: {:#x}, unit: {:#x}, \
             unit_max_ms: {ms} }},\n",
            int(&field(r, "start"), "start"),
            int(&field(r, "len"), "len"),
            int(&field(r, "unit"), "unit"),
        );
    }
    out += "];\n";
    let layouts = t
        .get("slot_layout")
        .and_then(|v| v.as_array())
        .map_or(&[][..], |v| v.as_slice());
    out += "/// Every target and slot the node receives into (`[[slot_layout]]`).\n";
    out += &format!(
        "pub const SLOT_LAYOUTS: [dagon_port::timeout::SlotLayout; {}] = [\n",
        layouts.len()
    );
    for l in layouts {
        out += &slot_layout_rs(l);
    }
    out += "];\n";
    out += "/// The longest single erase, ms (the largest `unit_max_ms`).\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS: u32 = {longest};\n");
    out += "/// Every `unit_max_ms` is checked against the flash's datasheet.\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = {known};\n");
    out
}

/// One `[[slot_layout]]` as a `SlotLayout` element.
fn slot_layout_rs(l: &toml::Value) -> String {
    let word = |key: &str| {
        l.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("[[slot_layout]] {key} is missing"))
    };
    let num = |key: &str| {
        let v = l
            .get(key)
            .unwrap_or_else(|| panic!("[[slot_layout]] {key} is missing"));
        int(v, key)
    };
    let target = match word("target") {
        "app" => "APP",
        "bootcore" => "BOOTCORE",
        "agent" => "AGENT",
        "topology" => "TOPOLOGY",
        other => panic!("[[slot_layout]] target {other:?} is not a target"),
    };
    let slot = match word("slot") {
        "a" => 0,
        "b" => 1,
        "factory" => 2,
        other => panic!("[[slot_layout]] slot {other:?} is not a slot"),
    };
    let header = match l.get("header") {
        Some(h) if h.as_str() == Some("inline") => "Inline".to_string(),
        Some(h) => {
            let at = h
                .get("detached")
                .unwrap_or_else(|| panic!("[[slot_layout]] header is inline or {{ detached }}"));
            format!("Detached {{ at: {:#x} }}", int(at, "detached"))
        }
        None => panic!("[[slot_layout]] header is missing"),
    };
    format!(
        "    dagon_port::timeout::SlotLayout {{ target: dagon_proto::payload::target::{target}, \
         slot: {slot}, start: {:#x}, capacity: {:#x}, \
         header: dagon_port::timeout::HeaderPlace::{header} }},\n",
        num("start"),
        num("capacity"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "[watchdog]\ntrial_attempts = 3\ntrial_ms = 4000\nconfirmed_ms = 8000\n\
                       [execution]\nmasked_max_us = 50\n\
                       erase_regions = [{ start = 0x0, len = 0x4000, unit = 0x1000, unit_max_ms = 400 },\n\
                       { start = 0x4000, len = 0x8000, unit = 0x4000, unit_max_ms = 900 }]\n\
                       [conformance]\nscratch = { at = 0x100, size = 0x10 }\n\
                       [[slot_layout]]\ntarget = \"app\"\nslot = \"b\"\nstart = 0x4000\n\
                       capacity = 0x3E00\nheader = \"inline\"\n\
                       [[slot_layout]]\ntarget = \"agent\"\nslot = \"factory\"\nstart = 0x8000\n\
                       capacity = 0x4000\nheader = { detached = 0x1000 }\n";

    #[test]
    fn constants_come_from_the_definition() {
        let rs = definition_rs(DEF);
        assert!(rs.contains("pub const TRIAL_ATTEMPTS: u8 = 3;\n"), "{rs}");
        assert!(rs.contains("pub const TRIAL_WDT_MS: u32 = 4000;\n"), "{rs}");
        assert!(
            rs.contains("pub const CONFIRMED_WDT_MS: u32 = 8000;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const STAGE0_VERIFIES: bool = false;\n"),
            "{rs}"
        );
        assert!(
            rs.contains(
                "EraseRegion { start: 0x4000, len: 0x8000, unit: 0x4000, unit_max_ms: 900 }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains(
                "SlotLayout { target: dagon_proto::payload::target::APP, slot: 1, \
                 start: 0x4000, capacity: 0x3e00, header: dagon_port::timeout::HeaderPlace::Inline }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains(
                "SlotLayout { target: dagon_proto::payload::target::AGENT, slot: 2, \
                 start: 0x8000, capacity: 0x4000, \
                 header: dagon_port::timeout::HeaderPlace::Detached { at: 0x1000 } }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 900;\n"),
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
            .replace("unit_max_ms = 400", "unit_max_ms = { unverified = 400 }");
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
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 900;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = false;\n"),
            "{rs}"
        );
    }
}
