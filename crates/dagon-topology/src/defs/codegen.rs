//! The Rust a build script writes from a board definition, so a board
//! package or a boot stage holds no board fact of its own: the board id and
//! flash size, the link address, the boot LED and halt pattern, pins and
//! buses by name, and the devices on the buses with their registers.
//!
//! Every name used here is checked on load (`board_boot::wiring` and
//! `tables`): identifiers are generated only from names the loader accepted.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::board_boot::{code_ident, code_names, Active, HaltPattern};
use super::load::{self, BOARD_FILE, MCU_FILE};
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

/// A board's definition as a build script reads it: the board, its MCU,
/// and every file the validation read.
#[derive(Debug)]
pub struct Loaded {
    pub board: BoardDef,
    pub mcu: McuDef,
    pub files: Vec<PathBuf>,
}

fn def_err(at: impl std::fmt::Display, msg: impl Into<String>) -> DefError {
    DefError {
        at: at.to_string(),
        msg: msg.into(),
    }
}

/// The root of the DAGON source tree a package folder sits in
/// (`<root>/targets/…/<package>`).
fn tree_root(dir: &Path) -> Result<&Path, DefError> {
    dir.ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "targets"))
        .and_then(Path::parent)
        .ok_or_else(|| def_err(dir.display(), "not inside a DAGON source tree's targets/"))
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

fn loaded(sources: &[Source<'_>], reg: &Registry, board: &BoardDef) -> Result<Loaded, DefError> {
    Ok(Loaded {
        board: board.clone(),
        mcu: reg.mcu(&board.mcu).expect("checked on load").clone(),
        files: load::files(sources)?,
    })
}

/// A board package's own definition (`dir/dagon-board.toml`) and its MCU,
/// validated with the source tree it sits in.
pub fn package_board(dir: &Path) -> Result<Loaded, DefError> {
    let sources = [Source::Tree(tree_root(dir)?)];
    let reg = Registry::load(&sources)?;
    let id = id_in(dir, BOARD_FILE)?;
    loaded(
        &sources,
        &reg,
        reg.board(&id).expect("loaded with the tree"),
    )
}

/// The board a boot stage of the MCU package at `mcu_dir` is built for,
/// from the source tree plus, when given, a project's dagon.toml (its
/// inline `[board.<id>]` boards). Never a guess: `board` names it, and may
/// be left out only when the MCU has exactly one board and no project is
/// given.
pub fn boot_board(
    mcu_dir: &Path,
    board: Option<&str>,
    project: Option<&Path>,
) -> Result<Loaded, DefError> {
    let mut sources = vec![Source::Tree(tree_root(mcu_dir)?)];
    sources.extend(project.map(Source::Project));
    let reg = Registry::load(&sources)?;
    let mcu = id_in(mcu_dir, MCU_FILE)?;
    let at = format!("MCU '{mcu}'");
    let of_mcu: Vec<&BoardDef> = reg.boards().iter().filter(|b| b.mcu == mcu).collect();
    let b = match (board, &of_mcu[..]) {
        (Some(id), _) => *of_mcu
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| def_err(&at, format!("no board '{id}' of this MCU")))?,
        (None, [only]) if project.is_none() => *only,
        (None, []) => return Err(def_err(&at, "no board definition of this MCU")),
        (None, _) => {
            let ids: Vec<&str> = of_mcu.iter().map(|b| b.id.as_str()).collect();
            return Err(def_err(
                &at,
                format!(
                    "name the node's board with DAGON_BOARD=<board id> ({})",
                    ids.join(", ")
                ),
            ));
        }
    };
    loaded(&sources, &reg, b)
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

/// The constants of board `b`, for `include!` from a build script's
/// output.
pub fn board_rs(b: &BoardDef) -> String {
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s definition.\n\n",
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
        let bus = code_ident(&x.id).to_uppercase();
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
        let dev = code_ident(&d.id).to_uppercase();
        let _ = writeln!(
            o,
            "/// Device `{}` on bus `{}`: its 7-bit address and the registers anyone outside DAGON may write.\n\
             pub const DEVICE_{dev}_ADDR: u8 = {:#04x};\npub const DEVICE_{dev}_WRITABLE: &[u8] = &{:?};",
            d.id,
            d.bus,
            d.addr,
            d.writable_regs()
        );
        for r in &d.regs {
            let _ = writeln!(
                o,
                "/// Register `{}` of device `{}`.\npub const DEVICE_{dev}_REG_{}: u8 = {:#04x};",
                r.name,
                d.id,
                r.name.to_uppercase(),
                r.reg
            );
        }
    }
    o
}

/// The board's devices as `dagon_events::Device` capabilities (the only
/// way one is made), for a package that depends on `dagon-events`.
pub fn devices_rs(b: &BoardDef) -> String {
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s definition.\n\n",
        b.id
    );
    for d in &b.devices {
        let dev = code_ident(&d.id).to_uppercase();
        let _ = writeln!(
            o,
            "/// `[[device]] {}` on bus `{}`.\n\
             // SAFETY: the address and writable registers of a device the\n\
             // definition loader accepted.\n\
             #[allow(unsafe_code)]\n\
             pub const {dev}: ::dagon_events::Device = unsafe {{\n    \
             ::dagon_events::Device::from_definition({:#04x}, &{:?}, {})\n}};",
            d.id,
            d.bus,
            d.addr,
            d.writable_regs(),
            d.max_transfer
        );
    }
    o
}

/// The pin and block types of board `b` and the `take!` macro that moves
/// them out of the HAL's peripherals, for `include!` beside [`board_rs`].
pub fn pins_rs(b: &BoardDef, hal: &Hal<'_>) -> String {
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s definition.\n\n",
        b.id
    );
    let names = code_names(b);
    let lt = if hal.lifetime { "<'d>" } else { "" };
    for (n, def) in &names {
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
    o
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn out_dir() -> PathBuf {
    std::env::var_os("OUT_DIR")
        .expect("run from a build script")
        .into()
}

fn manifest_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("run from a build script")
        .into()
}

fn watch(files: &[PathBuf]) {
    for f in files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
}

/// A board package's build script: writes `def.rs` ([`board_rs`]),
/// `pins.rs` ([`pins_rs`]) and `devices.rs` ([`devices_rs`]) into
/// `OUT_DIR` from the package's own definition, and reruns when any
/// definition the validation read changes; a definition that does not
/// load fails the build with the loader's message.
pub fn package_build(hal: &Hal<'_>) {
    let l = package_board(&manifest_dir()).unwrap_or_else(|e| panic!("{e}"));
    watch(&l.files);
    write(&out_dir().join("def.rs"), &board_rs(&l.board));
    write(&out_dir().join("pins.rs"), &pins_rs(&l.board, hal));
    write(&out_dir().join("devices.rs"), &devices_rs(&l.board));
}

/// A boot stage's build script (in a folder of the MCU package): writes
/// `board.rs` ([`board_rs`]) and, with a HAL, `pins.rs` into `OUT_DIR` for
/// the board `DAGON_BOARD` names (with `DAGON_PROJECT`, the path of a
/// project's dagon.toml, its inline boards too; see [`boot_board`]), and
/// returns that board.
pub fn boot_build(hal: Option<&Hal<'_>>) -> BoardDef {
    let dir = manifest_dir();
    let mcu = dir
        .ancestors()
        .find(|a| a.join(MCU_FILE).is_file())
        .unwrap_or_else(|| panic!("{}: not inside an MCU package", dir.display()));
    println!("cargo:rerun-if-env-changed=DAGON_BOARD");
    println!("cargo:rerun-if-env-changed=DAGON_PROJECT");
    let board = std::env::var("DAGON_BOARD").ok();
    let project = std::env::var_os("DAGON_PROJECT").map(PathBuf::from);
    let l = boot_board(mcu, board.as_deref(), project.as_deref()).unwrap_or_else(|e| panic!("{e}"));
    watch(&l.files);
    write(&out_dir().join("board.rs"), &board_rs(&l.board));
    if let Some(hal) = hal {
        write(&out_dir().join("pins.rs"), &pins_rs(&l.board, hal));
    }
    l.board
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
        let rs = board_rs(b) + &pins_rs(b, &hal);
        for want in [
            "pub const BOOT_LED_GPIO: Option<u8> = Some(25);",
            "pub type BootLed<'d> = hal::peripherals::GPIO25<'d>;",
            "($p:ident . child_i2c_sda) => { $p.GPIO16 };",
            "pub const BUS_CHILD_I2C_HZ: u32 = 400000;",
        ] {
            assert!(rs.contains(want), "{want} in\n{rs}");
        }
        assert!(!board_rs(b).contains("macro_rules"));
        let m5 = reg.board("m5stopwatch").unwrap();
        let rs = board_rs(m5) + &devices_rs(m5);
        for want in [
            "pub const DEVICE_IOE_REG_OUT_L: u8 = 0x05;",
            "pub const DEVICE_IOE_WRITABLE: &[u8] = &[3, 5, 19];",
            "::dagon_events::Device::from_definition(0x4f, &[3, 5, 19], 2)",
        ] {
            assert!(rs.contains(want), "{want} in\n{rs}");
        }
    }

    fn tree(name: &str, boards: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dagon-codegen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/mcu/rp2040");
        let mcu = dir.join("targets/mcu/rp2040");
        for part in ["stage0", "bootcore"] {
            std::fs::create_dir_all(mcu.join(part)).unwrap();
        }
        std::fs::copy(src.join(MCU_FILE), mcu.join(MCU_FILE)).unwrap();
        for (id, extra) in boards {
            let board = dir.join("targets/boards").join(id);
            std::fs::create_dir_all(&board).unwrap();
            std::fs::write(
                board.join(BOARD_FILE),
                format!(
                    "id = {id:?}\nproduct = \"P\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
                     drawing = \"x\"\n{extra}[boot]\nled = \"GPIO25\"\nhalt = {{ pattern = \"blink\", period_ms = 200 }}\n\
                     [[pin]]\npin = \"GPIO25\"\nname = \"led\"\n"
                ),
            )
            .unwrap();
        }
        (dir, mcu)
    }

    #[test]
    fn a_synthetic_board_package_reads_its_definition() {
        let (dir, mcu) = tree("one", &[("pico-like", "")]);
        let l = package_board(&dir.join("targets/boards/pico-like")).unwrap();
        assert_eq!(
            (l.board.id.as_str(), l.mcu.id.as_str()),
            ("pico-like", "rp2040")
        );
        // Watches every definition the validation read, the MCU's included.
        assert!(l.files.iter().any(|f| f.ends_with("rp2040/dagon-mcu.toml")));
        assert!(l
            .files
            .iter()
            .any(|f| f.ends_with("pico-like/dagon-board.toml")));
        // The MCU's only board needs no name.
        assert_eq!(boot_board(&mcu, None, None).unwrap().board.id, "pico-like");
        let rs = board_rs(&l.board);
        assert!(
            rs.contains("HALT_BLINK_PERIOD_MS: Option<u32> = Some(200)"),
            "{rs}"
        );
        assert!(boot_board(&mcu, Some("nope"), None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_boot_build_never_guesses_the_board() {
        // Two boards, one marked default: still named explicitly.
        let (dir, mcu) = tree("two", &[("a-board", "default = true\n"), ("b-board", "")]);
        let e = boot_board(&mcu, None, None).unwrap_err();
        assert!(
            e.msg.contains("DAGON_BOARD") && e.msg.contains("a-board, b-board"),
            "{e}"
        );
        assert_eq!(
            boot_board(&mcu, Some("b-board"), None).unwrap().board.id,
            "b-board"
        );
        // A project's inline board, by name only.
        let project = dir.join("dagon.toml");
        std::fs::write(
            &project,
            "[board.mine]\nproduct = \"Mine\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
             drawing = \"x\"\n[board.mine.boot]\nled = \"GPIO2\"\n[[board.mine.pin]]\npin = \"GPIO2\"\n",
        )
        .unwrap();
        let e = boot_board(&mcu, None, Some(&project)).unwrap_err();
        assert!(e.msg.contains("DAGON_BOARD"), "{e}");
        let l = boot_board(&mcu, Some("mine"), Some(&project)).unwrap();
        assert_eq!(l.board.boot.unwrap().led.as_deref(), Some("GPIO2"));
        assert!(l.files.contains(&project));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
