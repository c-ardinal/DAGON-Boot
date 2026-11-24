//! The Rust a build script writes from a board definition, so a board
//! package or a boot stage holds no board fact of its own: the board id and
//! flash size, the link address, the boot LED and halt pattern, pins and
//! buses by name, and the devices on the buses with their registers.
//!
//! Every name used here is checked on load (`board_boot::wiring` and
//! `tables`): identifiers are generated only from names the loader accepted.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::board_boot::{code_ident, code_names, type_name as camel, Active, HaltPattern};
use super::boot_folder::{self, BootFolder, BOOT_FOLDER_ENV};
use super::execution::EraseRegionDef;
use super::load::{self, BOARD_FILE, MCU_FILE};
use super::{BoardDef, ClockSetup, ClockStage, DefError, McuDef, Registry, Source};

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
    package_board_in(dir, None)
}

/// [`package_board`], built for the node whose boot folder is `boot`
/// ([`BOOT_FOLDER_ENV`]) when given: the node's board and MCU as `dagon
/// gen` wrote them there ([`node_registry`]). A node without a board, or
/// on another board than this package's, is refused.
fn package_board_in(dir: &Path, boot: Option<&Path>) -> Result<Loaded, DefError> {
    let id = id_in(dir, BOARD_FILE)?;
    let sources = [Source::Tree(tree_root(dir)?)];
    let tree = Registry::load(&sources)?;
    let Some(boot) = boot else {
        return loaded(
            &sources,
            &tree,
            tree.board(&id).expect("loaded with the tree"),
        );
    };
    let on = tree.board(&id).expect("loaded with the tree").mcu.clone();
    let board = boot.join(boot_folder::BOARD_FILE);
    if !board.is_file() {
        return Err(def_err(
            board.display(),
            format!("the node names no board; this package is board '{id}'"),
        ));
    }
    let (reg, files) = node_registry(dir, boot, &on, Some((&id, Some(dir))))?;
    Ok(Loaded {
        board: reg.board(&id).expect("checked on load").clone(),
        mcu: reg.mcu(&on).expect("checked on load").clone(),
        files,
    })
}

/// The source tree's definitions with the node's own in place of theirs:
/// its MCU (`mcu`) and, when `board` names it, its board, as `dagon gen`
/// wrote them into the node's boot folder `boot` (with the project's
/// overrides), checked against its [`boot_folder::DEFS_FILE`] first; the
/// board's parts are in `board`'s package when given. Never the project's
/// dagon.toml, which may have changed since gen. Returns the registry and
/// the files it read. `dir` is a folder of the source tree.
fn node_registry(
    dir: &Path,
    boot: &Path,
    mcu: &str,
    board: Option<(&str, Option<&Path>)>,
) -> Result<(Registry, Vec<PathBuf>), DefError> {
    let sources = [Source::Tree(tree_root(dir)?)];
    let mut reg = Registry::load(&sources)?;
    let mut files = load::files(&sources)?;
    let mcu_file = boot.join(boot_folder::MCU_FILE);
    let board_file = boot.join(boot_folder::BOARD_FILE);
    files.extend([
        boot.join(boot_folder::DEFS_FILE),
        mcu_file.clone(),
        board_file.clone(),
    ]);
    boot_folder::check_defs(boot).map_err(|e| def_err(BOOT_FOLDER_ENV, e))?;
    let gen = "run `dagon gen`";
    if !mcu_file.is_file() {
        return Err(def_err(mcu_file.display(), format!("missing; {gen}")));
    }
    let node_mcu = id_in(boot, boot_folder::MCU_FILE)?;
    if node_mcu != mcu {
        return Err(def_err(
            mcu_file.display(),
            format!("the definition of MCU '{node_mcu}'; this build is for MCU '{mcu}'"),
        ));
    }
    let node_board = match board {
        None => None,
        Some((id, package)) => {
            if !board_file.is_file() {
                return Err(def_err(
                    board_file.display(),
                    format!("missing for board '{id}'; {gen}"),
                ));
            }
            let found = id_in(boot, boot_folder::BOARD_FILE)?;
            if found != id {
                return Err(def_err(
                    board_file.display(),
                    format!("the node is on board '{found}'; this build is for board '{id}'"),
                ));
            }
            Some((board_file.as_path(), package))
        }
    };
    load::node_files(&mut reg, &mcu_file, node_board)?;
    Ok((reg, files))
}

/// The boot folder a board package's build is given ([`BOOT_FOLDER_ENV`]).
fn boot_folder_env() -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed={BOOT_FOLDER_ENV}");
    std::env::var_os(BOOT_FOLDER_ENV).map(PathBuf::from)
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
                format!("name the node's board ({})", ids.join(", ")),
            ));
        }
    };
    loaded(&sources, &reg, b)
}

/// `GPIO25` → 25 for MCUs that number their pins.
fn gpio_number(pin: &str) -> Option<u8> {
    pin.strip_prefix("GPIO")?.parse().ok()
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

/// The erase regions of nodes on a board (the loader's effective regions,
/// `McuDef::erase_regions_on`), as `(start, len, unit, unit_max_ms)` rows a
/// `BoardDef` turns into its erase regions (`EraseRegion::table`). Written
/// into a board package's def.rs and a node's wiring.rs alike.
pub fn erase_regions_rs(regions: &[EraseRegionDef]) -> String {
    let mut o = format!(
        "/// The erase regions of nodes on this board, as (start, len, unit,\n\
         /// unit_max_ms): its `[flash] erase_regions` when it carries its own flash\n\
         /// part, else its MCU's. Its `BoardDef` returns them.\n\
         pub const ERASE_REGIONS: [(u32, u32, u32, u32); {}] = [\n",
        regions.len()
    );
    for r in regions {
        let _ = writeln!(
            o,
            "    ({:#x}, {:#x}, {:#x}, {}),",
            r.start,
            r.len,
            r.unit,
            r.unit_max_ms.value().unwrap_or(0)
        );
    }
    o += "];\n";
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
/// them out of the HAL's peripherals, for `include!` beside [`board_rs`];
/// with them the GPIOs of its MCU `m` the definition names for nothing
/// ([`spare_gpios`]): `spare_gpios!(m)` calls `m! { pin_0 PIN_0, ... }`
/// with each one's field name and singleton, and `take_spare_gpios!(p, S
/// { ... })` builds `S` with those fields moved out of `p` before the
/// fields given. A board's code takes what its definition does not name
/// from them, so a project that moves a pin to a free one changes both.
pub fn pins_rs(b: &BoardDef, m: &McuDef, hal: &Hal<'_>) -> String {
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
    let spare: Vec<(String, String)> = spare_gpios(b, m)
        .into_iter()
        .map(|g| (format!("pin_{g}"), (hal.name)(&format!("GPIO{g}"))))
        .collect();
    let list =
        |f: &dyn Fn(&(String, String)) -> String| spare.iter().map(f).collect::<Vec<_>>().join(" ");
    let _ = write!(
        o,
        "\n/// Calls `$m!` with the field name and singleton of each GPIO the definition\n\
         /// names for no pin or bus: `pin_0 PIN_0, ...`.\n\
         #[macro_export]\nmacro_rules! spare_gpios {{\n    ($m:ident) => {{ $m! {{ {} }} }};\n}}\n\
         \n/// Builds `$s` with each of those GPIOs moved out of `$p`, then the fields given.\n\
         #[macro_export]\nmacro_rules! take_spare_gpios {{\n    \
         ($p:ident, $s:ident {{ $($rest:tt)* }}) => {{ $s {{ {} $($rest)* }} }};\n}}\n",
        list(&|(f, t)| format!("{f} {t},")),
        list(&|(f, t)| format!("{f}: $p.{t},"))
    );
    o
}

/// The numbered GPIOs of MCU `m` (below its `gpio_count`) that board `b`'s
/// definition names for no pin, bus pin or boot indicator, in order; none
/// on an MCU whose pins are not numbered.
pub fn spare_gpios(b: &BoardDef, m: &McuDef) -> Vec<u8> {
    let named: std::collections::BTreeSet<u8> = code_names(b)
        .iter()
        .filter_map(|(_, pin)| gpio_number(pin))
        .collect();
    (0..m.gpio_count).filter(|g| !named.contains(g)).collect()
}

/// How a HAL names DMA streams, their interrupts and its binding macro
/// (embassy-stm32: `DMA1_CH4`, `DMA1_STREAM4`).
pub struct DmaHal<'a> {
    /// Path of the HAL's peripheral singletons.
    pub peripherals: &'a str,
    /// The singleton of stream `n` of controller `ctl` (a definition id).
    pub stream: &'a dyn Fn(&str, u8) -> String,
    /// The interrupt of that stream.
    pub interrupt: &'a dyn Fn(&str, u8) -> String,
    /// The interrupt handler type, generic over the stream singleton.
    pub handler: &'a str,
    /// The HAL's interrupt binding macro.
    pub bind: &'a str,
}

/// The DMA streams of board `b`'s `dma = true` buses
/// ([`super::dma::board_streams`]): per bus `<Bus>TxDma` and `<Bus>RxDma`
/// types, `take_dma!(p.<bus>_tx)` arms, and `bind_<bus>_dma!`, which is
/// the HAL's binding macro with that bus's stream interrupts added (the
/// package that opens the bus binds them; another bus is its user's).
pub fn dma_rs(b: &BoardDef, m: &McuDef, hal: &DmaHal<'_>) -> Result<String, DefError> {
    let buses = super::dma::board_streams(b, m).map_err(|e| def_err(&b.id, e))?;
    let mut o = format!(
        "// Written by the package's build script from board `{}`'s definition and\n\
         // MCU `{}`'s DMA request mapping.\n\n",
        b.id, m.id
    );
    let mut take = String::new();
    for x in &buses {
        let bus = code_ident(&x.bus.id);
        let mut arms = String::new();
        for (dir, s) in [("tx", &x.tx), ("rx", &x.rx)] {
            let one = (hal.stream)(&s.controller, s.stream);
            let _ = writeln!(
                o,
                "/// Bus `{}` {dir}: {} stream {}, request channel {}.\npub type {}{}Dma = {}::{one};",
                x.bus.id,
                s.controller,
                s.stream,
                s.channel,
                camel(&bus),
                camel(dir),
                hal.peripherals
            );
            let _ = writeln!(take, "    ($p:ident . {bus}_{dir}) => {{ $p.{one} }};");
            let _ = writeln!(
                arms,
                "            {} => {}<{}::{one}>;",
                (hal.interrupt)(&s.controller, s.stream),
                hal.handler,
                hal.peripherals
            );
        }
        let _ = writeln!(
            o,
            "\n/// `{bind}!` with bus `{id}`'s DMA stream interrupts added:\n\
             /// `bind_{bus}_dma!(pub struct Irqs {{ .. }});`.\n#[macro_export]\n\
             macro_rules! bind_{bus}_dma {{\n    ($vis:vis struct $name:ident {{ $($body:tt)* }}) => {{\n        \
             {bind}!($vis struct $name {{\n            $($body)*\n{arms}        }});\n    }};\n}}",
            bind = hal.bind,
            id = x.bus.id,
        );
    }
    o.push_str(
        "\n/// Takes a DMA stream of the definition out of the HAL's peripherals:\n\
         /// `take_dma!(p.link_tx)`.\n#[macro_export]\nmacro_rules! take_dma {\n",
    );
    o.push_str(&take);
    o.push_str("}\n");
    Ok(o)
}

/// A board package's build script, for a board with `dma = true` buses:
/// writes `dma.rs` ([`dma_rs`]) into `OUT_DIR` from the board definition
/// [`package_build`] reads. Call it beside [`package_build`].
pub fn dma_build(hal: &DmaHal<'_>) {
    let l = package_board_in(&manifest_dir(), boot_folder_env().as_deref())
        .unwrap_or_else(|e| panic!("{e}"));
    watch(&l.files);
    let rs = dma_rs(&l.board, &l.mcu, hal).unwrap_or_else(|e| panic!("{e}"));
    write(&out_dir().join("dma.rs"), &rs);
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

/// The bit rate of the node's UART link a firmware build is given: `dagon`
/// sets it from the topology ([`crate::Topology::uart_baud`]) for every
/// build of a node, so both ends of a link are built with one value.
pub const LINK_BAUD_ENV: &str = "DAGON_LINK_BAUD";

/// `LINK_BAUD`, the bit rate a board package runs DAGON's UART link at:
/// `env` ([`LINK_BAUD_ENV`]) when set, else the board's own value
/// (`[default_link] baud`, or the baud of its `dagon = true` UART bus).
/// A value that is not a bit rate fails the build, naming it.
pub fn link_baud_rs(b: &BoardDef, env: Option<&str>) -> Result<String, String> {
    let own = b.default_link.and_then(|l| l.baud).or_else(|| {
        b.buses
            .iter()
            .find(|x| x.dagon && x.baud.is_some())
            .and_then(|x| x.baud)
    });
    let baud = match env.filter(|v| !v.is_empty()) {
        Some(v) => match v.parse::<u32>() {
            Ok(n) if n > 0 => Some(n),
            _ => {
                return Err(format!(
                    "{LINK_BAUD_ENV}={v:?} is not a bit rate (1 or more)"
                ))
            }
        },
        None => own,
    };
    Ok(format!(
        "/// The bit rate of DAGON's UART link: the topology's for this node \
         when built by dagon ({LINK_BAUD_ENV}), else the board's own.\n\
         pub const LINK_BAUD: Option<u32> = {};\n",
        opt(baud)
    ))
}

/// A board package's build script: writes `def.rs` ([`board_rs`], the
/// link speed, the effective erase regions, [`erase_regions_rs`], and for
/// an MCU with a DAGON boot loader `app_clock`, the setup an app on the
/// board runs on, [`clock_rs`]),
/// `pins.rs` ([`pins_rs`]) and `devices.rs` ([`devices_rs`]) into
/// `OUT_DIR`, and reruns when any definition the validation read changes;
/// a definition that does not load fails the build with the loader's
/// message. The definition is the node's board and MCU in its boot folder
/// when [`BOOT_FOLDER_ENV`] is set (an app built for a node: `dagon gen`
/// wrote them there with the project's overrides, and a file that no
/// longer matches its [`boot_folder::DEFS_FILE`] line fails the build),
/// else the package's own and its MCU's in the source tree.
pub fn package_build(hal: &Hal<'_>) {
    let l = package_board_in(&manifest_dir(), boot_folder_env().as_deref())
        .unwrap_or_else(|e| panic!("{e}"));
    watch(&l.files);
    println!("cargo:rerun-if-env-changed={LINK_BAUD_ENV}");
    let env = std::env::var(LINK_BAUD_ENV).ok();
    let baud = link_baud_rs(&l.board, env.as_deref()).unwrap_or_else(|e| panic!("{e}"));
    let regions = l.mcu.erase_regions_on(Some(&l.board)).unwrap_or(&[]);
    let mut def = board_rs(&l.board) + &baud + &erase_regions_rs(regions);
    if let Some(c) = l.mcu.clock_for(ClockStage::App, Some(&l.board)) {
        def += &format!(
            "\n/// The clock an app on this board runs on.\npub mod app_clock {{\n{}}}\n",
            clock_rs(ClockStage::App, c)
        );
    }
    write(&out_dir().join("def.rs"), &def);
    write(&out_dir().join("pins.rs"), &pins_rs(&l.board, &l.mcu, hal));
    write(&out_dir().join("devices.rs"), &devices_rs(&l.board));
}

/// What a boot stage's build reads: its MCU package folder and the node's
/// boot folder ([`BOOT_FOLDER_ENV`], required).
fn boot_inputs() -> (PathBuf, BootFolder) {
    let dir = manifest_dir();
    let mcu = dir
        .ancestors()
        .find(|a| a.join(MCU_FILE).is_file())
        .unwrap_or_else(|| panic!("{}: not inside an MCU package", dir.display()))
        .to_path_buf();
    println!("cargo:rerun-if-env-changed={BOOT_FOLDER_ENV}");
    let folder = std::env::var_os(BOOT_FOLDER_ENV).map(PathBuf::from).unwrap_or_else(|| {
        panic!(
            "{BOOT_FOLDER_ENV} is unset: a boot stage is built for one node. Run `dagon gen`, then build \
             the node's boot crate in boot/<id> (it sets {BOOT_FOLDER_ENV}), or set \
             {BOOT_FOLDER_ENV}=<project>/boot/<id> for stage0"
        )
    });
    let f = BootFolder::read(&folder).unwrap_or_else(|e| panic!("{BOOT_FOLDER_ENV}: {e}"));
    println!(
        "cargo:rerun-if-changed={}",
        folder.join("Cargo.toml").display()
    );
    let own = id_in(&mcu, MCU_FILE).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        f.meta.mcu == own,
        "{BOOT_FOLDER_ENV}: node {} is on MCU {}; this boot stage is for {own}",
        f.meta.node,
        f.meta.mcu
    );
    (mcu, f)
}

/// A boot stage's build script (in a folder of the MCU package): writes
/// `board.rs` ([`board_rs`]) and, with a HAL, `pins.rs` into `OUT_DIR` for
/// the board of the node whose boot folder [`BOOT_FOLDER_ENV`] names (its
/// `board.toml` as `dagon gen` wrote it, [`boot_node`]), and returns that
/// board; a node without a board gets no LED and no halt pattern
/// ([`no_board_rs`]). A build without the boot folder, or with one of a
/// node on another MCU, fails.
pub fn boot_build(hal: Option<&Hal<'_>>) -> Option<BoardDef> {
    let (mcu, f) = boot_inputs();
    // Checked for every node, with or without a board.
    let (m, b, files) = boot_node(&mcu, &f).unwrap_or_else(|e| panic!("{e}"));
    watch(&files);
    let Some(b) = b else {
        write(&out_dir().join("board.rs"), &no_board_rs(&f.meta.node));
        if hal.is_some() {
            write(&out_dir().join("pins.rs"), NO_PINS_RS);
        }
        return None;
    };
    write(&out_dir().join("board.rs"), &board_rs(&b));
    if let Some(hal) = hal {
        write(&out_dir().join("pins.rs"), &pins_rs(&b, &m, hal));
    }
    Some(b)
}

/// The MCU and board (none for a node without one) a boot stage of the
/// MCU package at `mcu_dir` is built with, for the node of boot folder
/// `f`: its `mcu.toml` and `board.toml` as `dagon gen` wrote them
/// ([`node_registry`]), the same files its app builds from. Returns them
/// and the files read.
fn boot_node(
    mcu_dir: &Path,
    f: &BootFolder,
) -> Result<(McuDef, Option<BoardDef>, Vec<PathBuf>), DefError> {
    let mcu = id_in(mcu_dir, MCU_FILE)?;
    let board = f.meta.board.as_deref();
    let (reg, files) = node_registry(mcu_dir, &f.dir, &mcu, board.map(|b| (b, None)))?;
    Ok((
        reg.mcu(&mcu).expect("checked on load").clone(),
        board.map(|b| reg.board(b).expect("checked on load").clone()),
        files,
    ))
}

/// How an MCU package's stage0 drives a board's `[boot]` indicator.
pub struct DiagHal<'a> {
    /// Path of its `fn(pin, active_high: bool, period_ms: u32, code: u8)`
    /// that shows `code` on the indicator for a board whose halt pattern
    /// blinks every `period_ms` (it need not return).
    pub report: &'a str,
    /// That function's `pin` argument for a definition pin (`GPIO25` → `25`);
    /// `None`: a pin it cannot drive.
    pub pin: &'a dyn Fn(&str) -> Option<String>,
}

/// Stage0's diagnostic output for a node on `board` (`None`: no board), for
/// `include!` beside [`board_rs`]: `BoardDiag`, implementing
/// `dagon_core::bootcore::Diag` on the board's `[boot] led` when its halt
/// pattern blinks; `type BoardDiag = NoDiag` when the board has no LED or
/// its halt pattern is off, and for a node without a board. Refused: a
/// halt pattern `code` (stage0 shows its own code in the blink pattern) and
/// an LED pin the package cannot drive.
pub fn diag_rs(board: Option<&BoardDef>, hal: &DiagHal<'_>) -> Result<String, String> {
    const NONE: &str = "/// Stage0's diagnostic output: none, so stage0 reports nothing.\n\
                        pub type BoardDiag = ::dagon_core::bootcore::NoDiag;\n";
    let Some(b) = board else {
        return Ok(NONE.into());
    };
    let boot = b.boot.clone().unwrap_or_default();
    let (Some(led), Some(halt)) = (boot.led.as_deref(), boot.halt) else {
        return Ok(NONE.into());
    };
    let period = match halt.pattern {
        HaltPattern::Off => return Ok(NONE.into()),
        HaltPattern::Code => {
            return Err(format!(
                "board {}: stage0 shows a halt as blink or off, not code",
                b.id
            ))
        }
        HaltPattern::Blink => halt.period_ms,
    };
    let pin = (hal.pin)(led)
        .ok_or_else(|| format!("board {}: stage0 cannot drive [boot] led {led}", b.id))?;
    Ok(format!(
        "/// Stage0's diagnostic output (`dagon_core::bootcore::Diag`): board `{}`'s\n\
         /// `[boot] led` {led}, showing the code with the halt pattern's blink.\n\
         #[derive(Clone, Copy, Debug, Default)]\n\
         pub struct BoardDiag;\n\n\
         impl ::dagon_core::bootcore::Diag for BoardDiag {{\n    \
         fn report(&mut self, code: u8) {{\n        \
         {}({pin}, {}, {period}, code)\n    }}\n}}\n",
        b.id,
        hal.report,
        boot.led_active == Active::High,
    ))
}

/// What a stage0 is built for: the node's board, and the clock setup its
/// stage0 runs on ([`McuDef::stage0_clock`]).
#[derive(Debug)]
pub struct Stage0Build {
    pub board: Option<BoardDef>,
    pub clock: ClockSetup,
}

/// A stage0's build script: [`boot_build`] without a HAL, and `diag.rs`
/// ([`diag_rs`]) into `OUT_DIR`; a board whose indicator stage0 cannot use
/// fails the build. Returns the node's board and the clock setup its
/// MCU's stage0 runs on for that board (the MCU definition as the node's
/// project sees it).
pub fn stage0_build(hal: &DiagHal<'_>) -> Stage0Build {
    let board = boot_build(None);
    let rs = diag_rs(board.as_ref(), hal).unwrap_or_else(|e| panic!("{e}"));
    write(&out_dir().join("diag.rs"), &rs);
    let clock = boot_clock(ClockStage::Stage0, board.as_ref());
    Stage0Build { board, clock }
}

/// The setup `stage` runs on for the node whose boot folder
/// [`BOOT_FOLDER_ENV`] names, on `board` (its MCU definition as `dagon gen`
/// wrote it there, [`boot_node`]); the build fails for a stage its MCU
/// does not have.
fn boot_clock(stage: ClockStage, board: Option<&BoardDef>) -> ClockSetup {
    let (dir, f) = boot_inputs();
    let (mcu, _, files) = boot_node(&dir, &f).unwrap_or_else(|e| panic!("{e}"));
    watch(&files);
    mcu.clock_for(stage, board)
        .unwrap_or_else(|| panic!("MCU '{}' has no [{}] clock", mcu.id, stage.name()))
        .clone()
}

/// A boot loader's build script: `clock.rs` ([`clock_rs`]) into `OUT_DIR`,
/// the `[[bootcore.clock]]` setup for the board (`board`, from
/// [`boot_build`]) of the node whose boot folder [`BOOT_FOLDER_ENV`] names;
/// returns it.
pub fn bootcore_clock_build(board: Option<&BoardDef>) -> ClockSetup {
    let clock = boot_clock(ClockStage::Bootcore, board);
    write(
        &out_dir().join("clock.rs"),
        &clock_rs(ClockStage::Bootcore, &clock),
    );
    clock
}

/// The setup `stage` runs on as constants a HAL start-up reads: `SOURCE`
/// (`"internal"`, `"crystal"`, `"external"`), `IN_HZ`, `OUT_HZ` and `PLL`,
/// the setup's `pll` fields by name (the MCU package's names, which it
/// checks when it reads them). A boot loader's build includes them, and
/// `dagon gen` writes an app's into its `wiring.rs`.
pub fn clock_rs(stage: ClockStage, c: &ClockSetup) -> String {
    let pll: Vec<String> = c
        .pll_fields()
        .iter()
        .map(|(k, v)| format!("({k:?}, {v})"))
        .collect();
    format!(
        "/// What drives the clock the {stage} runs on (the MCU definition's\n\
         /// `[[{stage}.clock]]` setup for the node's board).\n\
         pub const SOURCE: &str = {:?};\n\
         /// The frequency of that input, Hz (none for the internal oscillator\n\
         /// without a PLL).\n\
         pub const IN_HZ: Option<u32> = {:?};\n\
         /// The core clock the {stage} runs at, Hz.\n\
         pub const OUT_HZ: u32 = {};\n\
         /// The setup's PLL fields by name (none without a PLL).\n\
         pub const PLL: &[(&str, u32)] = &[{}];\n",
        c.source.name(),
        c.in_hz,
        c.out_hz,
        pll.join(", "),
        stage = stage.name(),
    )
}

/// The MCU definition of the package at `mcu_dir`, from the source tree
/// plus, when given, a project's dagon.toml (its overrides).
#[cfg(test)]
fn boot_mcu(mcu_dir: &Path, project: Option<&Path>) -> Result<McuDef, DefError> {
    let mut sources = vec![Source::Tree(tree_root(mcu_dir)?)];
    sources.extend(project.map(Source::Project));
    let reg = Registry::load(&sources)?;
    let id = id_in(mcu_dir, MCU_FILE)?;
    watch(&load::files(&sources)?);
    reg.mcu(&id)
        .cloned()
        .ok_or_else(|| def_err(mcu_dir.display(), format!("no MCU '{id}'")))
}

/// A boot core library's build script: [`BootFolder::verify_stamp`] for
/// the node's boot folder, so the build refuses a boot crate whose
/// dependency graph the host check (`dagon gen`) did not pass for exactly
/// its current `Cargo.toml`, `Cargo.lock` and trusted extension sources.
/// The graph itself is checked on the host, where cargo can resolve it
/// (all features, every target); a build script cannot reproduce the
/// outer build's features or resolve offline reliably.
pub fn boot_verify_stamp() {
    let (_, f) = boot_inputs();
    println!(
        "cargo:rerun-if-changed={}",
        f.dir.join(super::boot_folder::STAMP_FILE).display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        f.dir.join("Cargo.lock").display()
    );
    // A whole folder, so a file added to an extension reruns the check.
    watch(&f.trusted_folders());
    f.verify_stamp().unwrap_or_else(|e| panic!("{e}"));
}

/// `board.rs` of a node without a board: no boot LED, no halt pattern.
pub fn no_board_rs(node: &str) -> String {
    format!(
        "// Written by the package's build script: node `{node}` names no board.\n\n\
         /// `[boot] led` as a GPIO number: none.\npub const BOOT_LED_GPIO: Option<u8> = None;\n\
         /// `[boot] led_active`.\npub const BOOT_LED_ACTIVE_HIGH: bool = true;\n\
         /// `[boot] halt` when it blinks: none.\npub const HALT_BLINK_PERIOD_MS: Option<u32> = None;\n"
    )
}

/// `pins.rs` of a node without a board: nothing to take.
const NO_PINS_RS: &str = "// Written by the package's build script: the node names no board.\n\n\
                          #[macro_export]\nmacro_rules! take {\n    ($p:ident . $name:ident) => {\n        \
                          compile_error!(\"the node names no board: no pin to take\")\n    };\n}\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn upper(s: &str) -> String {
        s.to_uppercase()
    }

    /// The effective regions become rows a `BoardDef` reads: a board's
    /// `[flash]` replaces its MCU's.
    #[test]
    fn a_boards_erase_regions_are_generated() {
        let reg = Registry::builtin();
        let plain = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
        let m = reg.mcu("rp2040").unwrap();
        let mcu_rows = erase_regions_rs(m.erase_regions_on(Some(plain)).unwrap());
        assert!(
            mcu_rows.contains(
                "[(u32, u32, u32, u32); 1] = [\n    (0x10000000, 0x400000, 0x1000, 500),\n];"
            ),
            "{mcu_rows}"
        );
        let mut b = plain.clone();
        b.flash = Some(crate::defs::BoardFlash {
            part: "W25Q16JV".into(),
            erase_regions: vec![crate::defs::EraseRegionDef {
                start: 0x1000_0000,
                len: 0x40_0000,
                unit: 0x1000,
                unit_max_ms: crate::defs::Fact::Unverified { unverified: 400 },
            }],
        });
        let rows = erase_regions_rs(m.erase_regions_on(Some(&b)).unwrap());
        assert!(
            rows.contains("    (0x10000000, 0x400000, 0x1000, 400),\n"),
            "{rows}"
        );
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
        let m = reg.mcu("rp2040").unwrap();
        let rs = board_rs(b) + &pins_rs(b, m, &hal);
        for want in [
            "pub const BOOT_LED_GPIO: Option<u8> = Some(25);",
            "pub type BootLed<'d> = hal::peripherals::GPIO25<'d>;",
            "($p:ident . child_i2c_sda) => { $p.GPIO16 };",
            "pub const BUS_CHILD_I2C_HZ: u32 = 400000;",
            // The GPIOs the definition names for nothing, for the board's
            // code to hand the app.
            "($m:ident) => { $m! { pin_0 GPIO0, pin_1 GPIO1, ",
            "pin_7 GPIO7, pin_12 GPIO12, pin_14 GPIO14, ",
            "pin_24 GPIO24, pin_29 GPIO29, } };",
            "$s { pin_0: $p.GPIO0, ",
        ] {
            assert!(rs.contains(want), "{want} in\n{rs}");
        }
        assert_eq!(
            spare_gpios(b, m),
            [0, 1, 2, 3, 4, 5, 6, 7, 12, 14, 15, 19, 20, 21, 23, 24, 29]
        );
        // A pin moved to a free GPIO frees the one it left.
        let mut moved = b.clone();
        let touch = moved
            .pins
            .iter_mut()
            .find(|p| p.name.as_deref() == Some("touch_rst"))
            .unwrap();
        touch.pin = "GPIO21".into();
        assert_eq!(
            spare_gpios(&moved, m),
            [0, 1, 2, 3, 4, 5, 6, 7, 12, 14, 15, 19, 20, 22, 23, 24, 29]
        );
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

    /// Stage0's diagnostic output comes from the board's `[boot]` LED and
    /// blinking halt pattern; without either (or without a board) it is
    /// `NoDiag`, and a pin the package cannot drive is refused.
    #[test]
    fn stage0_diag_is_the_boards_indicator_or_none() {
        let reg = Registry::builtin();
        let gpio = |p: &str| p.strip_prefix("GPIO").map(str::to_string);
        let hal = DiagHal {
            report: "mcu::led::report",
            pin: &gpio,
        };
        let b = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
        let rs = diag_rs(Some(b), &hal).unwrap();
        assert!(rs.contains("pub struct BoardDiag;"), "{rs}");
        assert!(
            rs.contains("impl ::dagon_core::bootcore::Diag for BoardDiag"),
            "{rs}"
        );
        assert!(
            rs.contains("mcu::led::report(25, true, 1000, code)"),
            "{rs}"
        );
        let none = "pub type BoardDiag = ::dagon_core::bootcore::NoDiag;";
        assert!(diag_rs(None, &hal).unwrap().contains(none));
        let mut dark = b.clone();
        dark.boot.as_mut().unwrap().halt = Some(super::super::board_boot::Halt {
            pattern: HaltPattern::Off,
            period_ms: 0,
        });
        assert!(diag_rs(Some(&dark), &hal).unwrap().contains(none));
        let mut no_led = b.clone();
        no_led.boot.as_mut().unwrap().led = None;
        assert!(diag_rs(Some(&no_led), &hal).unwrap().contains(none));
        let mut no_boot = b.clone();
        no_boot.boot = None;
        assert!(diag_rs(Some(&no_boot), &hal).unwrap().contains(none));
        let nucleo = reg.board("nucleo-f401re").unwrap();
        assert!(diag_rs(Some(nucleo), &hal)
            .unwrap_err()
            .contains("cannot drive [boot] led PA5"));
        let mut code = b.clone();
        code.boot.as_mut().unwrap().halt = Some(super::super::board_boot::Halt {
            pattern: HaltPattern::Code,
            period_ms: 0,
        });
        assert!(diag_rs(Some(&code), &hal).is_err());
    }

    #[test]
    fn link_baud_is_the_builds_and_else_the_boards_own() {
        let reg = Registry::builtin();
        let want = |v: &str| format!("pub const LINK_BAUD: Option<u32> = {v};");
        for id in ["waveshare-rp2040-touch-lcd-1.28", "m5stopwatch"] {
            let b = reg.board(id).unwrap();
            // Both ends of a link get the same value from the build.
            assert!(link_baud_rs(b, Some("230400"))
                .unwrap()
                .contains(&want("Some(230400)")));
            assert!(link_baud_rs(b, None)
                .unwrap()
                .contains(&want("Some(115200)")));
            assert!(link_baud_rs(b, Some(""))
                .unwrap()
                .contains(&want("Some(115200)")));
            for bad in ["0", "fast", "-1", "4294967296"] {
                let e = link_baud_rs(b, Some(bad)).unwrap_err();
                assert!(e.contains(LINK_BAUD_ENV) && e.contains(bad), "{e}");
            }
        }
        let f401 = reg.board("nucleo-f401re").unwrap();
        assert!(link_baud_rs(f401, None).unwrap().contains(&want("None")));
    }

    #[test]
    fn dma_streams_come_from_the_mcu_definition() {
        let reg = Registry::builtin();
        let b = reg.board("nucleo-f401re").unwrap();
        let m = reg.mcu(&b.mcu).unwrap();
        let stream = |c: &str, n: u8| format!("{}_CH{n}", c.to_uppercase());
        let irq = |c: &str, n: u8| format!("{}_STREAM{n}", c.to_uppercase());
        let hal = DmaHal {
            peripherals: "hal::peripherals",
            stream: &stream,
            interrupt: &irq,
            handler: "hal::dma::InterruptHandler",
            bind: "hal::bind_interrupts",
        };
        let rs = dma_rs(b, m, &hal).unwrap();
        for want in [
            // I2C3: tx stream 4 (channel 3), rx stream 1 (channel 1), the
            // first of each in RM0368's table.
            "pub type LinkTxDma = hal::peripherals::DMA1_CH4;",
            "pub type LinkRxDma = hal::peripherals::DMA1_CH1;",
            "pub type ArduinoI2cTxDma = hal::peripherals::DMA1_CH6;",
            "pub type ArduinoI2cRxDma = hal::peripherals::DMA1_CH0;",
            "($p:ident . link_rx) => { $p.DMA1_CH1 };",
            "macro_rules! bind_link_dma",
            "DMA1_STREAM4 => hal::dma::InterruptHandler<hal::peripherals::DMA1_CH4>;",
            "macro_rules! bind_arduino_i2c_dma",
        ] {
            assert!(rs.contains(want), "{want} in\n{rs}");
        }
        // The VCP is handed out without DMA.
        assert!(!rs.contains("Vcp"), "{rs}");
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
                    "id = {id:?}\nformat = 3\nproduct = \"P\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
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

    /// A board package built for a node (`DAGON_BOOT_CRATE` set) reads the
    /// node's board and MCU from its boot folder, as `dagon gen` wrote
    /// them with the project's overrides, and watches them; a file that no
    /// longer matches `defs.sha256`, a folder without a board, and the
    /// board of another package are refused.
    #[test]
    fn a_board_package_built_for_a_node_reads_the_node_s_board() {
        use super::super::boot_folder::{
            board_file_text, defs_text, mcu_file_text, BOARD_FILE as NODE_BOARD, DEFS_FILE,
            MCU_FILE as NODE_MCU,
        };
        let (dir, _) = tree("node", &[("pico-like", ""), ("other", "")]);
        let pkg = dir.join("targets/boards/pico-like");
        let project = dir.join("dagon.toml");
        std::fs::write(
            &project,
            "format = 1\n[[board_override.pico-like.pin]]\npin = \"GPIO3\"\nname = \"button\"\n\n\
             [mcu_override.rp2040.watchdog]\ntrial_ms = 6_000\n",
        )
        .unwrap();
        let reg = Registry::load(&[Source::Tree(&dir), Source::Project(&project)]).unwrap();
        let boot = dir.join("boot/n");
        std::fs::create_dir_all(&boot).unwrap();
        let gen = |board: &str| {
            let mcu = mcu_file_text(&reg, "rp2040").unwrap();
            let board = board_file_text(&reg, board).unwrap();
            std::fs::write(boot.join(NODE_MCU), &mcu).unwrap();
            std::fs::write(boot.join(NODE_BOARD), &board).unwrap();
            std::fs::write(
                boot.join(DEFS_FILE),
                defs_text(&[(NODE_MCU, &mcu), (NODE_BOARD, &board)]),
            )
            .unwrap();
        };
        gen("pico-like");
        let button = |l: &Loaded| l.board.pins.iter().any(|p| p.pin == "GPIO3");
        assert!(!button(&package_board(&pkg).unwrap()));
        let l = package_board_in(&pkg, Some(&boot)).unwrap();
        assert!(button(&l));
        assert_eq!(l.mcu.watchdog, reg.mcu("rp2040").unwrap().watchdog);
        for f in [NODE_MCU, NODE_BOARD, DEFS_FILE] {
            assert!(l.files.contains(&boot.join(f)), "{f}: {:?}", l.files);
        }
        // Edited by hand after gen.
        let text = std::fs::read_to_string(boot.join(NODE_BOARD)).unwrap();
        std::fs::write(boot.join(NODE_BOARD), text.replace("GPIO3", "GPIO4")).unwrap();
        let e = package_board_in(&pkg, Some(&boot)).unwrap_err().to_string();
        assert!(e.contains("board.toml") && e.contains(DEFS_FILE), "{e}");
        // Another package's board.
        gen("other");
        let e = package_board_in(&pkg, Some(&boot)).unwrap_err().to_string();
        assert!(e.contains("'other'") && e.contains("pico-like"), "{e}");
        // A node that names no board.
        std::fs::remove_file(boot.join(NODE_BOARD)).unwrap();
        let mcu = std::fs::read_to_string(boot.join(NODE_MCU)).unwrap();
        std::fs::write(boot.join(DEFS_FILE), defs_text(&[(NODE_MCU, &mcu)])).unwrap();
        let e = package_board_in(&pkg, Some(&boot)).unwrap_err().to_string();
        assert!(e.contains("names no board"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A boot stage builds from the node's `mcu.toml` and `board.toml` as
    /// `dagon gen` wrote them, the files its app builds from, never from
    /// dagon.toml: an override changed there after gen does not reach it,
    /// a file edited by hand is refused, and a project's inline board is
    /// read from its `board.toml` like any other.
    #[test]
    fn a_boot_stage_builds_from_the_files_gen_wrote_never_dagon_toml() {
        use super::super::boot_folder::{
            board_file_text, defs_text, mcu_file_text, BOARD_FILE as NODE_BOARD, DEFS_FILE,
            MCU_FILE as NODE_MCU,
        };
        let (dir, mcu_dir) = tree("boot-node", &[("pico-like", "")]);
        let project = dir.join("dagon.toml");
        let overrides = |pin: &str, trial: u32| {
            format!(
                "format = 1\n[[board_override.pico-like.pin]]\npin = \"{pin}\"\nname = \"button\"\n\n\
                 [mcu_override.rp2040.watchdog]\ntrial_ms = {trial}\n\n\
                 [board.mine]\nformat = 3\nproduct = \"Mine\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
                 drawing = \"x\"\n[board.mine.boot]\nled = \"{pin}\"\n[[board.mine.pin]]\npin = \"{pin}\"\n"
            )
        };
        std::fs::write(&project, overrides("GPIO3", 6_000)).unwrap();
        let reg = Registry::load(&[Source::Tree(&dir), Source::Project(&project)]).unwrap();
        let boot = dir.join("boot/n");
        std::fs::create_dir_all(&boot).unwrap();
        let gen = |board: &str| {
            std::fs::write(
                boot.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"n-boot\"\nversion = \"0.1.0\"\n\
                     [package.metadata.dagon.boot]\nnode = \"n\"\nmcu = \"rp2040\"\nboard = \"{board}\"\n\
                     project = \"../../dagon.toml\"\n"
                ),
            )
            .unwrap();
            let mcu = mcu_file_text(&reg, "rp2040").unwrap();
            let b = board_file_text(&reg, board).unwrap();
            std::fs::write(boot.join(NODE_MCU), &mcu).unwrap();
            std::fs::write(boot.join(NODE_BOARD), &b).unwrap();
            std::fs::write(
                boot.join(DEFS_FILE),
                defs_text(&[(NODE_MCU, &mcu), (NODE_BOARD, &b)]),
            )
            .unwrap();
            BootFolder::read(&boot).unwrap()
        };
        let trial = |m: &McuDef| m.watchdog.as_ref().map(|w| w.trial_ms);
        let pins = |b: &BoardDef| b.pins.iter().map(|p| p.pin.clone()).collect::<Vec<_>>();
        let f = gen("pico-like");
        // dagon.toml changes after gen: the boot stage still builds from
        // what gen wrote.
        std::fs::write(&project, overrides("GPIO4", 7_000)).unwrap();
        let (m, b, files) = boot_node(&mcu_dir, &f).unwrap();
        assert_eq!(trial(&m), trial(reg.mcu("rp2040").unwrap()));
        let b = b.unwrap();
        assert!(pins(&b).contains(&"GPIO3".to_string()), "{:?}", pins(&b));
        assert!(!pins(&b).contains(&"GPIO4".to_string()));
        assert!(!files.contains(&project), "{files:?}");
        assert!(files.contains(&boot.join(NODE_BOARD)));
        // Edited by hand after gen.
        let text = std::fs::read_to_string(boot.join(NODE_BOARD)).unwrap();
        std::fs::write(boot.join(NODE_BOARD), text.replace("GPIO3", "GPIO5")).unwrap();
        let e = boot_node(&mcu_dir, &f).unwrap_err().to_string();
        assert!(e.contains("board.toml") && e.contains(DEFS_FILE), "{e}");
        // A project's inline board.
        let f = gen("mine");
        let (_, b, _) = boot_node(&mcu_dir, &f).unwrap();
        assert_eq!(b.unwrap().boot.unwrap().led.as_deref(), Some("GPIO3"));
        // The node's board file is required.
        std::fs::remove_file(boot.join(NODE_BOARD)).unwrap();
        let mcu = std::fs::read_to_string(boot.join(NODE_MCU)).unwrap();
        std::fs::write(boot.join(DEFS_FILE), defs_text(&[(NODE_MCU, &mcu)])).unwrap();
        let e = boot_node(&mcu_dir, &f).unwrap_err().to_string();
        assert!(e.contains("board.toml") && e.contains("dagon gen"), "{e}");
        // A node without a board: its MCU definition is checked all the
        // same.
        std::fs::write(
            boot.join("Cargo.toml"),
            "[package]\nname = \"n-boot\"\nversion = \"0.1.0\"\n\
             [package.metadata.dagon.boot]\nnode = \"n\"\nmcu = \"rp2040\"\n",
        )
        .unwrap();
        let f = BootFolder::read(&boot).unwrap();
        let (_, b, _) = boot_node(&mcu_dir, &f).unwrap();
        assert!(b.is_none());
        std::fs::write(boot.join(NODE_MCU), format!("{mcu}# edited\n")).unwrap();
        let e = boot_node(&mcu_dir, &f).unwrap_err().to_string();
        assert!(e.contains("mcu.toml") && e.contains(DEFS_FILE), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A node folder without `defs.sha256` (an older one, or one whose
    /// hashes were deleted) is refused by a board package's build and by
    /// the boot stages', for a node with a board and for one without,
    /// saying to run `dagon gen`.
    #[test]
    fn a_node_folder_without_its_hashes_is_refused_by_every_build() {
        use super::super::boot_folder::{
            board_file_text, defs_text, mcu_file_text, BOARD_FILE as NODE_BOARD, DEFS_FILE,
            MCU_FILE as NODE_MCU,
        };
        let (dir, mcu_dir) = tree("no-hashes", &[("pico-like", "")]);
        let pkg = dir.join("targets/boards/pico-like");
        let reg = Registry::load(&[Source::Tree(&dir)]).unwrap();
        let boot = dir.join("boot/n");
        std::fs::create_dir_all(&boot).unwrap();
        let mcu = mcu_file_text(&reg, "rp2040").unwrap();
        let board = board_file_text(&reg, "pico-like").unwrap();
        let folder = |board_line: &str| {
            std::fs::write(
                boot.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"n-boot\"\nversion = \"0.1.0\"\n\
                     [package.metadata.dagon.boot]\nnode = \"n\"\nmcu = \"rp2040\"\n{board_line}"
                ),
            )
            .unwrap();
            BootFolder::read(&boot).unwrap()
        };
        let refused = |e: DefError| {
            let e = e.to_string();
            assert!(
                e.contains("no defs.sha256") && e.contains("dagon gen"),
                "{e}"
            );
        };
        // A node with a board: its board package and its boot stages.
        std::fs::write(boot.join(NODE_MCU), &mcu).unwrap();
        std::fs::write(boot.join(NODE_BOARD), &board).unwrap();
        std::fs::write(
            boot.join(DEFS_FILE),
            defs_text(&[(NODE_MCU, &mcu), (NODE_BOARD, &board)]),
        )
        .unwrap();
        let f = folder("board = \"pico-like\"\n");
        package_board_in(&pkg, Some(&boot)).unwrap();
        boot_node(&mcu_dir, &f).unwrap();
        std::fs::remove_file(boot.join(DEFS_FILE)).unwrap();
        refused(package_board_in(&pkg, Some(&boot)).unwrap_err());
        refused(boot_node(&mcu_dir, &f).unwrap_err());
        // A node without a board: its boot stages.
        std::fs::remove_file(boot.join(NODE_BOARD)).unwrap();
        std::fs::write(boot.join(DEFS_FILE), defs_text(&[(NODE_MCU, &mcu)])).unwrap();
        let f = folder("");
        boot_node(&mcu_dir, &f).unwrap();
        std::fs::remove_file(boot.join(DEFS_FILE)).unwrap();
        refused(boot_node(&mcu_dir, &f).unwrap_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_boot_build_never_guesses_the_board() {
        // Two boards, one marked default: still named explicitly.
        let (dir, mcu) = tree("two", &[("a-board", "default = true\n"), ("b-board", "")]);
        let e = boot_board(&mcu, None, None).unwrap_err();
        assert!(
            e.msg.contains("name the node's board") && e.msg.contains("a-board, b-board"),
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
            "format = 1\n[board.mine]\nformat = 3\nproduct = \"Mine\"\nmcu = \"rp2040\"\nflash_bytes = 0x40_0000\n\
             drawing = \"x\"\n[board.mine.boot]\nled = \"GPIO2\"\n[[board.mine.pin]]\npin = \"GPIO2\"\n",
        )
        .unwrap();
        let e = boot_board(&mcu, None, Some(&project)).unwrap_err();
        assert!(e.msg.contains("name the node's board"), "{e}");
        let l = boot_board(&mcu, Some("mine"), Some(&project)).unwrap();
        assert_eq!(l.board.boot.unwrap().led.as_deref(), Some("GPIO2"));
        assert!(l.files.contains(&project));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A stage0 build reads its MCU as the tree gives it and runs on the
    /// setup the node's board picks: the crystal's, else the internal one.
    #[test]
    fn a_stage0_build_takes_the_clock_its_board_picks() {
        let xtal = "[clock]
source = { crystal_hz = 12_000_000 }
";
        let (dir, mcu) = tree("clock", &[("with-xtal", xtal), ("plain", "")]);
        let m = boot_mcu(&mcu, None).unwrap();
        assert_eq!(m.stage0_clock(None).unwrap().out_hz, 6_500_000);
        for (board, out_hz) in [("with-xtal", 125_000_000), ("plain", 6_500_000)] {
            let l = boot_board(&mcu, Some(board), None).unwrap();
            assert_eq!(m.stage0_clock(Some(&l.board)).unwrap().out_hz, out_hz);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The boot loader and an app on a board package run on the setup the
    /// board picks: the crystal's, else (no `[clock]`, no crystal) the
    /// internal one, which on the RP2040 has no USB clock.
    #[test]
    fn the_boot_loader_and_a_board_packages_app_take_the_clock_the_board_picks() {
        let xtal = "[clock]
source = { crystal_hz = 12_000_000 }
";
        let (dir, mcu) = tree("hal-clock", &[("with-xtal", xtal), ("plain", "")]);
        let m = boot_mcu(&mcu, None).unwrap();
        let none = m.clock_for(ClockStage::Bootcore, None).unwrap();
        assert_eq!((none.source.name(), none.out_hz), ("internal", 6_500_000));
        for (board, source, out_hz) in [
            ("with-xtal", "crystal", 125_000_000),
            ("plain", "internal", 6_500_000),
        ] {
            let l = boot_board(&mcu, Some(board), None).unwrap();
            let boot = m.clock_for(ClockStage::Bootcore, Some(&l.board)).unwrap();
            assert_eq!((boot.source.name(), boot.out_hz), (source, out_hz));
            let rs = clock_rs(ClockStage::Bootcore, boot);
            assert!(
                rs.contains(&format!("pub const SOURCE: &str = {source:?};\n")),
                "{rs}"
            );
            assert!(
                rs.contains(&format!("pub const OUT_HZ: u32 = {out_hz};\n")),
                "{rs}"
            );
            let pkg = package_board(&dir.join("targets/boards").join(board)).unwrap();
            let app = pkg
                .mcu
                .clock_for(ClockStage::App, Some(&pkg.board))
                .unwrap();
            assert_eq!(app.out_hz, out_hz, "{board}");
            assert_eq!(app.pll.contains_key("usb_fbdiv"), source == "crystal");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The constants name the setup's PLL fields as the definition does.
    #[test]
    fn clock_rs_writes_the_setup_by_name() {
        let reg = Registry::builtin();
        let ws = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
        let c = reg
            .mcu("rp2040")
            .unwrap()
            .clock_for(ClockStage::App, Some(ws))
            .unwrap();
        let rs = clock_rs(ClockStage::App, c);
        for want in [
            "pub const SOURCE: &str = \"crystal\";\n",
            "pub const IN_HZ: Option<u32> = Some(12000000);\n",
            "pub const OUT_HZ: u32 = 125000000;\n",
            "(\"fbdiv\", 125), (\"postdiv1\", 6), (\"postdiv2\", 2), (\"refdiv\", 1), \
             (\"usb_fbdiv\", 120), (\"usb_postdiv1\", 6), (\"usb_postdiv2\", 5), (\"usb_refdiv\", 1)];\n",
            "`[[app.clock]]`",
        ] {
            assert!(rs.contains(want), "{want} in {rs}");
        }
        assert!(
            rs.lines()
                .all(|l| l.starts_with("///") || l.starts_with("pub const ")),
            "{rs}"
        );
        let st = reg
            .mcu("stm32f401re")
            .unwrap()
            .clock_for(ClockStage::Bootcore, None)
            .unwrap();
        let rs = clock_rs(ClockStage::Bootcore, st);
        assert!(
            rs.contains("pub const SOURCE: &str = \"internal\";\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const IN_HZ: Option<u32> = Some(16000000);\n"),
            "{rs}"
        );
    }
}
