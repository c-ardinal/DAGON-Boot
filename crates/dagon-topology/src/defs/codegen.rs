//! The Rust a build script writes from a board definition, so a board
//! package or a boot stage holds no board fact of its own: the board id and
//! flash size, the link address, the boot LED and halt pattern, pins and
//! buses by name, and the devices on the buses.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use super::board_boot::{Active, HaltPattern};
use super::load::{BOARD_FILE, MCU_FILE};
use super::{BoardDef, DefError, McuDef, Registry, Source};

/// How a HAL names the singletons of pins and register blocks.
pub struct Hal<'a> {
    /// Path of the HAL's peripheral singletons (`embassy_rp::peripherals`).
    pub peripherals: &'a str,
    /// The singleton of a definition name (`GPIO16` → `PIN_16`, `i2c0` →
    /// `I2C0`).
    pub name: &'a dyn Fn(&str) -> String,
    /// The singletons take a lifetime (`GPIO47<'d>`).
    pub lifetime: bool,
}

fn def_err(at: impl std::fmt::Display, msg: impl Into<String>) -> DefError {
    DefError {
        at: at.to_string(),
        msg: msg.into(),
    }
}

/// The definitions of the DAGON source tree a package folder sits in
/// (`<root>/targets/…/<package>`), validated as a whole.
pub fn tree_of(dir: &Path) -> Result<Registry, DefError> {
    let root = dir
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "targets"))
        .and_then(Path::parent)
        .ok_or_else(|| def_err(dir.display(), "not inside a DAGON source tree's targets/"))?;
    Registry::load(&[Source::Tree(root)])
}

/// The `id` of the definition `dir/file`.
fn id_in(dir: &Path, file: &str) -> Result<String, DefError> {
    let path = dir.join(file);
    let text =
        std::fs::read_to_string(&path).map_err(|e| def_err(path.display(), e.to_string()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| def_err(path.display(), e.to_string()))?;
    table
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| def_err(path.display(), "no id"))
}

/// A board package's own definition (`dir/dagon-board.toml`) and its MCU,
/// validated with the source tree it sits in.
pub fn package_board(dir: &Path) -> Result<(BoardDef, McuDef), DefError> {
    let reg = tree_of(dir)?;
    let id = id_in(dir, BOARD_FILE)?;
    let b = reg.board(&id).expect("loaded with the tree").clone();
    let m = reg.mcu(&b.mcu).expect("checked on load").clone();
    Ok((b, m))
}

/// The board a boot stage of the MCU package at `mcu_dir` is built for:
/// `board` when given, else the MCU's default board.
pub fn boot_board(mcu_dir: &Path, board: Option<&str>) -> Result<(BoardDef, McuDef), DefError> {
    let reg = tree_of(mcu_dir)?;
    let mcu = id_in(mcu_dir, MCU_FILE)?;
    let at = format!("MCU '{mcu}'");
    let b = match board {
        Some(id) => reg
            .board(id)
            .filter(|b| b.mcu == mcu)
            .ok_or_else(|| def_err(&at, format!("no board '{id}' of this MCU")))?,
        None => reg
            .default_board(&mcu)
            .ok_or_else(|| def_err(&at, "no default board; name one (DAGON_BOARD=<board id>)"))?,
    };
    let m = reg.mcu(&mcu).expect("loaded with the tree").clone();
    Ok((b.clone(), m))
}

/// `GPIO25` → 25 for MCUs that number their pins.
fn gpio_number(pin: &str) -> Option<u8> {
    pin.strip_prefix("GPIO")?.parse().ok()
}

/// `child_uart_tx` → `ChildUartTx`.
fn camel(snake: &str) -> String {
    snake
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or("None".into(), |v| format!("Some({v})"))
}

/// Everything with a code name: (name, definition name of the singleton).
fn named(b: &BoardDef) -> Vec<(String, String)> {
    let led = b.boot.as_ref().and_then(|x| x.led.clone());
    let mut out: Vec<(String, String)> = led.map(|p| ("boot_led".into(), p)).into_iter().collect();
    for p in &b.pins {
        if let Some(n) = &p.name {
            out.push((n.clone(), p.pin.clone()));
        }
    }
    for x in &b.buses {
        let bus = x.id.replace('-', "_");
        out.push((bus.clone(), x.controller.clone()));
        for (s, pin) in x.pins() {
            out.push((format!("{bus}_{s}"), pin.to_string()));
        }
    }
    out
}

/// The constants of board `b`, for `include!` from a build script's
/// output.
pub fn board_rs(b: &BoardDef) -> String {
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s dagon-board.toml.\n\n",
        b.id
    );
    let boot = b.boot.clone().unwrap_or_default();
    let led = boot.led.as_deref();
    let blink = boot
        .halt
        .filter(|h| h.pattern == HaltPattern::Blink)
        .map(|h| h.period_ms);
    let link = b.default_link;
    let _ = writeln!(
        o,
        "/// The board id.\npub const BOARD_ID: &str = {:?};\n\
         /// Flash size in bytes.\npub const FLASH_BYTES: u32 = {:#x};\n\
         /// `[default_link] addr`: the node's I2C target address.\npub const DEFAULT_LINK_ADDR: Option<u8> = {};\n\
         /// `[default_link] baud`.\npub const DEFAULT_LINK_BAUD: Option<u32> = {};\n\
         /// `[boot] led` as a GPIO number (MCUs that number their pins).\npub const BOOT_LED_GPIO: Option<u8> = {};\n\
         /// `[boot] led_active`.\npub const BOOT_LED_ACTIVE_HIGH: bool = {};\n\
         /// `[boot] halt` when it blinks: the period in ms.\npub const HALT_BLINK_PERIOD_MS: Option<u32> = {};\n",
        b.id,
        b.flash_bytes,
        opt(link.and_then(|l| l.addr).map(|a| format!("{a:#04x}"))),
        opt(link.and_then(|l| l.baud)),
        opt(led.and_then(gpio_number)),
        boot.led_active == Active::High,
        opt(blink),
    );
    for x in &b.buses {
        let bus = x.id.replace('-', "_").to_uppercase();
        if let Some(hz) = x.hz {
            let _ = writeln!(
                o,
                "/// Clock of bus `{}`.\npub const BUS_{bus}_HZ: u32 = {hz};",
                x.id
            );
        }
        if let Some(baud) = x.baud {
            let _ = writeln!(
                o,
                "/// Bit rate of bus `{}`.\npub const BUS_{bus}_BAUD: u32 = {baud};",
                x.id
            );
        }
    }
    for d in &b.devices {
        let dev = d.id.replace('-', "_").to_uppercase();
        let _ = writeln!(
            o,
            "/// Device `{}` on bus `{}`: its 7-bit address and the registers anyone outside DAGON may write.\n\
             pub const DEVICE_{dev}_ADDR: u8 = {:#04x};\npub const DEVICE_{dev}_WRITABLE: &[u8] = &{:?};",
            d.id, d.bus, d.addr, d.writable
        );
    }
    o
}

/// The pin and block types of board `b` and the `take!` macro that moves
/// them out of the HAL's peripherals, for `include!` beside [`board_rs`].
pub fn pins_rs(b: &BoardDef, hal: &Hal<'_>) -> Result<String, String> {
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s dagon-board.toml.\n",
        b.id
    );
    let names = named(b);
    let mut seen = BTreeSet::new();
    for (n, _) in &names {
        if !seen.insert(camel(n)) {
            return Err(format!("board {}: two pins or buses are named {n}", b.id));
        }
    }
    o.push('\n');
    for (n, def) in &names {
        let lt = if hal.lifetime { "<'d>" } else { "" };
        let _ = writeln!(
            o,
            "/// `{def}`.\npub type {}{lt} = {}::{}{lt};",
            camel(n),
            hal.peripherals,
            (hal.name)(def)
        );
    }
    o.push_str(
        "\n/// Takes a pin or block of the definition out of the HAL's peripherals:\n\
         /// `take!(p.lcd_cs)` (`$crate::take!` from an exported macro).\n\
         #[macro_export]\nmacro_rules! take {\n",
    );
    for (n, def) in &names {
        let _ = writeln!(o, "    ($p:ident . {n}) => {{ $p.{} }};", (hal.name)(def));
    }
    o.push_str("}\n");
    Ok(o)
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn out_dir() -> std::path::PathBuf {
    std::env::var_os("OUT_DIR")
        .expect("run from a build script")
        .into()
}

fn manifest_dir() -> std::path::PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("run from a build script")
        .into()
}

/// A board package's build script: writes `def.rs` ([`board_rs`]) and
/// `pins.rs` ([`pins_rs`]) into `OUT_DIR` from the package's own
/// definition; a definition that does not load fails the build with the
/// loader's message.
pub fn package_build(hal: &Hal<'_>) {
    let dir = manifest_dir();
    println!("cargo:rerun-if-changed={}", dir.join(BOARD_FILE).display());
    let (b, _) = package_board(&dir).unwrap_or_else(|e| panic!("{e}"));
    write(&out_dir().join("def.rs"), &board_rs(&b));
    let pins = pins_rs(&b, hal).unwrap_or_else(|e| panic!("{e}"));
    write(&out_dir().join("pins.rs"), &pins);
}

/// A boot stage's build script (in a folder of the MCU package): writes
/// `board.rs` ([`board_rs`]) and, with a HAL, `pins.rs` into `OUT_DIR` for
/// the board `DAGON_BOARD` names, else the MCU's default board, and
/// returns that board.
pub fn boot_build(hal: Option<&Hal<'_>>) -> BoardDef {
    let dir = manifest_dir();
    let mcu = dir
        .ancestors()
        .find(|a| a.join(MCU_FILE).is_file())
        .unwrap_or_else(|| panic!("{}: not inside an MCU package", dir.display()));
    println!("cargo:rerun-if-env-changed=DAGON_BOARD");
    let targets = mcu
        .parent()
        .and_then(Path::parent)
        .expect("targets/mcu/<id>");
    println!(
        "cargo:rerun-if-changed={}",
        targets.join("boards").display()
    );
    let board = std::env::var("DAGON_BOARD").ok();
    let (b, _) = boot_board(mcu, board.as_deref()).unwrap_or_else(|e| panic!("{e}"));
    write(&out_dir().join("board.rs"), &board_rs(&b));
    if let Some(hal) = hal {
        let pins = pins_rs(&b, hal).unwrap_or_else(|e| panic!("{e}"));
        write(&out_dir().join("pins.rs"), &pins);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upper(s: &str) -> String {
        s.to_uppercase()
    }

    #[test]
    fn writes_every_board_fact_by_name() {
        let reg = Registry::builtin();
        let b = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
        let hal = Hal {
            peripherals: "hal::peripherals",
            name: &upper,
            lifetime: true,
        };
        let rs = board_rs(b) + &pins_rs(b, &hal).unwrap();
        for want in [
            "pub const BOOT_LED_GPIO: Option<u8> = Some(25);",
            "pub type BootLed<'d> = hal::peripherals::GPIO25<'d>;",
            "($p:ident . child_i2c_sda) => { $p.GPIO16 };",
            "pub const BUS_CHILD_I2C_HZ: u32 = 400000;",
        ] {
            assert!(rs.contains(want), "{want} in\n{rs}");
        }
        assert!(!board_rs(b).contains("macro_rules"));
    }

    #[test]
    fn a_synthetic_board_package_reads_its_definition() {
        let dir = std::env::temp_dir().join(format!("dagon-codegen-{}", std::process::id()));
        let root = dir.join("tree");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/mcu/rp2040");
        let mcu = root.join("targets/mcu/rp2040");
        for part in ["stage0", "bootcore"] {
            std::fs::create_dir_all(mcu.join(part)).unwrap();
        }
        std::fs::copy(src.join(MCU_FILE), mcu.join(MCU_FILE)).unwrap();
        let board = root.join("targets/boards/pico-like");
        std::fs::create_dir_all(&board).unwrap();
        std::fs::write(
            board.join(BOARD_FILE),
            "id = \"pico-like\"\nproduct = \"P\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
             drawing = \"x\"\n[boot]\nled = \"GPIO25\"\nhalt = { pattern = \"blink\", period_ms = 200 }\n\
             [[pin]]\npin = \"GPIO25\"\nname = \"led\"\n",
        )
        .unwrap();
        let (b, m) = package_board(&board).unwrap();
        assert_eq!((b.id.as_str(), m.id.as_str()), ("pico-like", "rp2040"));
        let (d, _) = boot_board(&mcu, None).unwrap();
        assert_eq!(d.id, "pico-like");
        let rs = board_rs(&b);
        assert!(
            rs.contains("HALT_BLINK_PERIOD_MS: Option<u32> = Some(200)"),
            "{rs}"
        );
        assert!(boot_board(&mcu, Some("nope")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
