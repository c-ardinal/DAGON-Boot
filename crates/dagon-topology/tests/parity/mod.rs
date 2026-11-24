//! One project whose dagon.toml overrides, by difference, a built-in MCU and
//! board and a synthetic MCU and board no code names, with the same values:
//! the fixture every consumer's parity test reads (the CLI, `dagon serve`,
//! dagon-wasm, dagon-host, dagon-sign and the firmware constants), each
//! asserting it sees these values and not the definitions' own.
//!
//! The project's DAGON source is a scratch tree holding the built-in
//! definitions the topology uses, copied from this source tree, and the
//! synthetic ones, made from the built-in STM32F401RE and NUCLEO-F401RE
//! under new ids (so they stay valid as the format changes).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The built-in MCU and board the project overrides, and the synthetic ones.
pub const BUILTIN: (&str, &str) = ("stm32f401re", "nucleo-f401re");
pub const SYNTHETIC: (&str, &str) = ("parity-m4", "parity-board");
/// Both, with each one's node in [`TOPOLOGY`].
pub const OVERRIDDEN: [(&str, &str, &str); 2] = [
    (BUILTIN.0, BUILTIN.1, "seg"),
    (SYNTHETIC.0, SYNTHETIC.1, "seg2"),
];
/// The synthetic MCU's identity in image headers.
pub fn synthetic_mcu_id() -> dagon_format::header::McuId {
    dagon_format::header::McuId::of_name(SYNTHETIC.0)
}

/// App slot B: start (unchanged), bytes; and what a node receives into it.
pub const APP_B: (u32, u32) = (0x0804_0000, 0x1_C000);
pub const APP_B_CAPACITY: u32 = 0x1_BE00;
/// `[watchdog]` trial and confirmed periods, ms.
pub const TRIAL_MS: u32 = 7_000;
pub const CONFIRMED_MS: u32 = 12_000;
/// The erase region at this start: its unit's longest erase, ms.
pub const ERASE_START: u32 = 0x0802_0000;
pub const ERASE_MAX_MS: u32 = 3_000;
/// `[rom_i2c]`: the ROM loader's address and the sectors its first install
/// erases.
pub const ROM_ADDR: u8 = 0x3B;
pub const ROM_ERASE: [u16; 6] = [0, 1, 2, 3, 4, 5];
/// A pin the boards gain, and their product name.
pub const PIN: (&str, &str) = ("PC2", "a3");
pub const PRODUCT: &str = "Overridden board";

/// The overrides of one MCU and its board.
fn overrides(mcu: &str, board: &str) -> String {
    format!(
        "[[mcu_override.{mcu}.slot]]\nimage = \"app\"\nbinding = \"b\"\nsize = {:#x}\n\n\
         [[mcu_override.{mcu}.slot_layout]]\ntarget = \"app\"\nslot = \"b\"\ncapacity = {APP_B_CAPACITY:#x}\n\n\
         [mcu_override.{mcu}.watchdog]\ntrial_ms = {TRIAL_MS}\nconfirmed_ms = {CONFIRMED_MS}\n\n\
         [[mcu_override.{mcu}.execution.erase_regions]]\nstart = {ERASE_START:#x}\nunit_max_ms = {ERASE_MAX_MS}\n\n\
         [mcu_override.{mcu}.rom_i2c]\naddr = {ROM_ADDR:#x}\nerase_sectors = {ROM_ERASE:?}\n\n\
         [board_override.{board}]\nproduct = {PRODUCT:?}\n\n\
         [[board_override.{board}.pin]]\npin = {:?}\nname = {:?}\n\n",
        APP_B.1, PIN.0, PIN.1
    )
}

/// The project's dagon.toml without its `[dagon]` table.
pub fn dagon_toml() -> String {
    let mut text = "format = 1\ntopology = \"t.yaml\"\n\n".to_string();
    for (mcu, board, _) in OVERRIDDEN {
        text += &overrides(mcu, board);
    }
    text
}

/// The hub, the built-in F401 node and the synthetic one, on one I2C bus.
pub const TOPOLOGY: &str = "dagon: 2
name: parity
nodes:
  - { id: hub, mcu: rp2040, board: waveshare-rp2040-touch-lcd-1.28, role: coordinator, port: self-boot, flash_map: rp2040-4m, max_attempts: 3, listen_window_ms: 0, tag: hub }
  - { id: seg, mcu: stm32f401re, board: nucleo-f401re, role: node, port: self-boot, flash_map: f401-default, max_attempts: 3, listen_window_ms: 50, tag: seg }
  - { id: seg2, mcu: parity-m4, board: parity-board, role: node, port: self-boot, flash_map: parity-default, max_attempts: 3, listen_window_ms: 50, tag: seg2 }
buses:
  - { id: i2c-main, kind: i2c, controller: i2c0, sda: 16, scl: 17, speed_hz: 400000, pullups: external }
links:
  - kind: i2c
    bus: i2c-main
    ends:
      - { node: hub, port: i2c0, nrst: 18, boot: 28 }
      - { node: seg, port: i2c3, addr: 0x42, node_pins: { sda: PB4, scl: PA8 } }
    control: { kind: full, rom_entry: stm32-i2c-rom }
  - kind: i2c
    bus: i2c-main
    ends:
      - { node: hub, port: i2c0 }
      - { node: seg2, port: i2c3, addr: 0x43, node_pins: { sda: PB4, scl: PA8 } }
    control: { kind: cooperative, rom_entry: stm32-i2c-rom }
recovery:
  hub: [local]
  seg: [local, redistribute, rom-forced]
  seg2: [local, redistribute]
update:
  order: [seg, seg2, hub]
";

/// A project folder with its DAGON source tree, removed on drop.
pub struct Project {
    pub dir: PathBuf,
    /// The DAGON source tree (`[dagon] path`).
    pub tree: PathBuf,
    /// The project's dagon.toml.
    pub toml: PathBuf,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn source(rel: &str) -> String {
    let at = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display()))
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// `text` with each `(from, to)` replaced, every `from` found exactly once.
fn renamed(text: &str, pairs: &[(&str, &str)]) -> String {
    pairs.iter().fold(text.to_string(), |t, (from, to)| {
        assert_eq!(t.matches(from).count(), 1, "{from}");
        t.replace(from, to)
    })
}

/// An MCU package of the scratch tree: its definition and its parts' folders.
fn mcu_package(tree: &Path, id: &str, text: &str) {
    let dir = tree.join("targets/mcu").join(id);
    write(&dir.join("dagon-mcu.toml"), text);
    for sub in text.lines().filter_map(|l| l.strip_prefix("sub = ")) {
        std::fs::create_dir_all(dir.join(sub.trim_matches('"'))).unwrap();
    }
}

/// The project `name` in a fresh scratch folder: dagon.toml (with
/// `[dagon] path` naming its tree) and t.yaml.
pub fn project(name: &str) -> Project {
    let dir = std::env::temp_dir().join(format!("dagon-parity-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tree = dir.join("tree");
    for id in ["rp2040", "stm32f401re"] {
        mcu_package(
            &tree,
            id,
            &source(&format!("targets/mcu/{id}/dagon-mcu.toml")),
        );
    }
    for id in ["waveshare-rp2040-touch-lcd-1.28", BUILTIN.1] {
        let rel = format!("targets/boards/{id}/dagon-board.toml");
        write(&tree.join(&rel), &source(&rel));
    }
    let mcu = renamed(
        &source("targets/mcu/stm32f401re/dagon-mcu.toml"),
        &[
            ("id = \"stm32f401re\"", "id = \"parity-m4\""),
            ("name = \"STM32F401RE\"", "name = \"PARITY-M4\""),
            ("name = \"f401-default\"", "name = \"parity-default\""),
        ],
    );
    mcu_package(&tree, SYNTHETIC.0, &mcu);
    let board = renamed(
        &source("targets/boards/nucleo-f401re/dagon-board.toml"),
        &[
            ("id = \"nucleo-f401re\"", "id = \"parity-board\""),
            ("product = \"NUCLEO-F401RE\"", "product = \"PARITY\""),
            ("mcu = \"stm32f401re\"", "mcu = \"parity-m4\""),
        ],
    );
    write(
        &tree.join("targets/boards/parity-board/dagon-board.toml"),
        &board,
    );
    let toml = dir.join("project/dagon.toml");
    let tree_path = tree.display().to_string().replace('\\', "/");
    write(
        &toml,
        &format!("{}[dagon]\npath = {tree_path:?}\n", dagon_toml()),
    );
    write(&dir.join("project/t.yaml"), TOPOLOGY);
    Project { dir, tree, toml }
}
