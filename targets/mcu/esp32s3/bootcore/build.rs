//! The boot core's linker script (`bootcore.x`) on the linker's search
//! path, and its listen window from the ESP32-S3 definition
//! (`../dagon-mcu.toml` `[boot_receive] listen_window_ms`, the UART link
//! taking part).

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rerun-if-changed=bootcore.x");
    let def = format!("{dir}/../dagon-mcu.toml");
    println!("cargo:rerun-if-changed={def}");
    let text = std::fs::read_to_string(&def).expect("the ESP32-S3 definition");
    let t: toml::Table = text.parse().expect("dagon-mcu.toml parses");
    let r = t
        .get("boot_receive")
        .and_then(toml::Value::as_table)
        .expect("[boot_receive]");
    let uart = r
        .get("kinds")
        .and_then(|k| k.get("uart"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let ms = r
        .get("listen_window_ms")
        .and_then(toml::Value::as_integer)
        .expect("[boot_receive] listen_window_ms");
    let ms = if uart {
        u16::try_from(ms).expect("a u16 window")
    } else {
        0
    };
    let out = std::env::var("OUT_DIR").expect("set by cargo");
    std::fs::write(
        format!("{out}/window.rs"),
        format!("/// The UART link's listen window (ms; 0: none).\npub const LISTEN_WINDOW_MS: u16 = {ms};\n"),
    )
    .expect("OUT_DIR is writable");
}
