//! Parity with the topology tables, validation, and loading new MCUs and
//! boards without code changes. Parity with signing, provisioning, the host
//! and the CLI is in `tests/defs_parity.rs`.

use std::path::{Path, PathBuf};

use super::load::{parse_board_dir, parse_mcu_dir, parse_mcu_text};
use super::*;
use crate::{boards, mcu_id_of, port_kind_of};

fn reg() -> &'static Registry {
    Registry::builtin()
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn mcus_match_the_port_registry() {
    let ids: Vec<&str> = reg().mcus().iter().map(|m| m.id.as_str()).collect();
    let table: Vec<&str> = boards::MCUS.iter().map(|m| m.id).collect();
    assert_eq!(ids, table);
    for t in boards::MCUS {
        let m = reg().mcu(t.id).unwrap();
        assert_eq!(m.port.as_str(), t.port, "{}", t.id);
        assert_eq!(m.gpio_count, t.gpio_count, "{}", t.id);
        assert_eq!(Some(m.wire_id), mcu_id_of(t.id), "{}", t.id);
        assert!(port_kind_of(m.port.as_str()).is_some());
        assert_eq!(reg().mcu_by_wire(m.wire_id).unwrap().id, t.id);
    }
}

#[test]
fn flash_maps_match_the_table() {
    let all: Vec<(&str, &str, u32)> = reg()
        .mcus()
        .iter()
        .flat_map(|m| {
            m.flash_maps
                .iter()
                .map(move |f| (f.name.as_str(), m.id.as_str(), f.bytes))
        })
        .collect();
    let table: Vec<(&str, &str, u32)> = boards::FLASH_MAPS
        .iter()
        .map(|f| (f.name, f.mcu, f.bytes))
        .collect();
    assert_eq!(all, table);
    for t in boards::FLASH_MAPS {
        let (m, f) = reg().flash_map(t.name).unwrap();
        assert_eq!((m.id.as_str(), f.bytes), (t.mcu, t.bytes));
        let d = boards::default_flash_map(t.mcu).unwrap();
        assert_eq!(m.default_flash_map().name, d.name);
    }
}

fn pairs(p: &[I2cPair]) -> Vec<(&str, &str, &str)> {
    p.iter()
        .map(|p| (p.controller.as_str(), p.sda.as_str(), p.scl.as_str()))
        .collect()
}

fn table_pairs(p: &[boards::I2cPair]) -> Vec<(&str, &str, &str)> {
    p.iter().map(|p| (p.controller, p.sda, p.scl)).collect()
}

#[test]
fn boards_match_the_table_field_for_field() {
    let ids: Vec<&str> = reg().boards().iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, boards::BOARDS.iter().map(|b| b.id).collect::<Vec<_>>());
    for t in boards::BOARDS {
        let b = reg().board(t.id).unwrap();
        let m = reg().mcu(&b.mcu).unwrap();
        assert_eq!(b.product, t.product);
        assert_eq!(m.name, t.mcu_name);
        assert_eq!(b.mcu, t.mcu);
        let header: Vec<(&str, Option<&str>)> = b
            .header
            .iter()
            .map(|h| (h.pin.as_str(), h.label.as_deref()))
            .collect();
        let want: Vec<(&str, Option<&str>)> =
            t.header.iter().map(|h| (h.pin, h.position)).collect();
        assert_eq!(header, want, "{}", t.id);
        assert_eq!(pairs(&b.i2c_pairs), table_pairs(t.i2c_pairs), "{}", t.id);
        let rom = m.rom_i2c.as_ref().map_or(vec![], |r| pairs(&r.pairs));
        assert_eq!(rom, table_pairs(t.rom_i2c_pairs), "{}", t.id);
        assert_eq!(b.i2c_target, t.i2c_target);
        assert_eq!(b.flash_bytes, t.flash_bytes);
        assert_eq!(b.drawing, t.drawing);
        let link = b
            .default_link
            .map(|l| (l.kind, l.iface, l.addr, l.baud, l.control, l.rom_entry));
        let want = t
            .default_link
            .map(|l| (l.kind, l.iface, l.addr, l.baud, l.control, l.rom_entry));
        assert_eq!(link, want, "{}", t.id);
        assert_eq!(
            reg().default_board(t.mcu).map(|b| &b.id[..]),
            boards::default_board(t.mcu).map(|b| b.id)
        );
    }
}

#[test]
fn coordinator_and_rom_rules_match_the_former_check_constants() {
    // The values check.rs held before it read them from the definitions.
    let c = reg().mcu("rp2040").unwrap().coordinator.as_ref().unwrap();
    assert_eq!(c.pio_state_machines, 8);
    assert_eq!(c.pio_uart_state_machines, 2);
    assert_eq!(c.uart0_tx, [0, 12, 16, 28]);
    assert_eq!(c.uart0_rx, [1, 13, 17, 29]);
    let rom = reg().mcu("stm32f401re").unwrap().rom_i2c.as_ref().unwrap();
    assert_eq!(rom.addr, 0x39);
    assert_eq!(rom.max_hz, 400_000);
    for id in ["stm32f401re", "esp32s3"] {
        assert!(reg().mcu(id).unwrap().coordinator.is_none());
    }
    for id in ["rp2040", "esp32s3"] {
        assert!(reg().mcu(id).unwrap().rom_i2c.is_none());
    }
}

#[test]
fn source_tree_loads_and_equals_builtin() {
    let tree = Registry::load(&[Source::Tree(&source_root())]).unwrap();
    let sorted = |r: &Registry| {
        let mut m = r.mcus().to_vec();
        m.sort_by(|a, b| a.id.cmp(&b.id));
        let mut b = r.boards().to_vec();
        b.sort_by(|a, b| a.id.cmp(&b.id));
        (m, b)
    };
    assert_eq!(sorted(&tree), sorted(reg()));
    assert_eq!(Registry::load(&[]).unwrap(), *reg());
}

pub(super) const FAKE_MCU: &str = r#"
id = "fake-m0"
name = "FAKE-M0"
wire_id = 0x7F01
port = "self-boot"
gpio_count = 16

[[flash_map]]
name = "fake-1m"
base = 0x0010_0000
bytes = 0x10_0000

[layout]
app_log = { faces = [0x0010_8000, 0x0010_9000], face_size = 0x1000 }

[[slot]]
image = "bootcore"
binding = "a"
header = "trailer"
at = 0x0010_0000
size = 0x8000

[[slot]]
image = "app"
binding = "a"
header = "inline"
at = 0x0011_0000
size = 0x4_0000

[[slot]]
image = "app"
binding = "b"
header = "inline"
at = 0x0015_0000
size = 0x4_0000

[writer]
kind = "probe-rs"
chip = "FAKE0"

[[part]]
name = "bootcore"
sub = "bootcore"

[connection]
label = "SWD"
hint = "SWD probe"
"#;

pub(super) const FAKE_BOARD: &str = r#"
product = "Fake Board"
mcu = "fake-m0"
flash_bytes = 0x20_0000
i2c_target = true
drawing = "nucleo-64"
header = [{ pin = "GPIO2", label = "D2" }]
i2c = [{ controller = "i2c0", sda = "GPIO2", scl = "GPIO3" }]
default_link = { kind = "i2c", addr = 0x44, control = "full" }
"#;

/// A fresh scratch folder under the system temp dir.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dagon-defs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The desk demo with its F401 node replaced by the fake MCU and board.
fn fake_desk(addr: &str) -> crate::Topology {
    let yaml = include_str!("../../../../topology/desk-demo.yaml")
        .replace("mcu: stm32f401re", "mcu: fake-m0")
        .replace("board: nucleo-f401re", "board: fake-board")
        .replace("flash_map: f401-default", "flash_map: fake-1m")
        .replace("{ sda: PB4, scl: PA8 }", "{ sda: GPIO2, scl: GPIO3 }")
        .replace("addr: 0x42", &format!("addr: {addr}"));
    crate::parse_str(&yaml).unwrap()
}

/// A registry with the fake MCU (its ROM I2C loader capped at `max_hz`).
fn fake_registry(name: &str, max_hz: u32) -> Registry {
    let dir = scratch(name);
    let rom = format!(
        "\n[rom_i2c]\nname = \"Fake ROM loader\"\naddr = 0x3A\nmax_hz = {max_hz}\n\
         pairs = [{{ controller = \"i2c0\", sda = \"GPIO2\", scl = \"GPIO3\" }}]\n"
    );
    write(&dir.join("m/dagon-mcu.toml"), &format!("{FAKE_MCU}{rom}"));
    std::fs::create_dir_all(dir.join("m/bootcore")).unwrap();
    let board = FAKE_BOARD.replace("product", "[board.fake-board]\nproduct");
    write(
        &dir.join("dagon.toml"),
        &format!("mcu_files = [\"m/dagon-mcu.toml\"]\n{board}"),
    );
    let r = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    r
}

fn rules(r: &crate::Report) -> Vec<&'static str> {
    r.errors().map(|i| i.rule).collect()
}

#[test]
fn checks_and_compile_follow_a_project_registry() {
    let reg = fake_registry("check-with", 400_000);
    let t = fake_desk("0x42");
    assert_eq!(rules(&crate::check_with(&reg, &t)), Vec::<&str>::new());
    let bin = crate::compile_with(&reg, &t).unwrap();
    assert!(!bin.is_empty());
    // The builtin set does not know the fake MCU.
    assert!(rules(&crate::check(&t)).contains(&"V-06"));
    assert!(crate::compile(&t).is_err());
    // Its ROM loader's address is reserved on the bus (from the definition).
    assert_eq!(
        rules(&crate::check_with(&reg, &fake_desk("0x3A"))),
        ["V-18"]
    );
    assert_eq!(
        rules(&crate::check_with(&reg, &fake_desk("0x39"))),
        Vec::<&str>::new()
    );
    // Its ROM loader caps the bus speed.
    let slow = fake_registry("check-with-slow", 100_000);
    assert_eq!(rules(&crate::check_with(&slow, &t)), ["V-20"]);
    // Messages name the loader from its definition.
    let msgs = |reg: &Registry, t: &crate::Topology| -> Vec<String> {
        crate::check_with(reg, t)
            .errors()
            .map(|i| i.message.clone())
            .collect()
    };
    assert_eq!(
        msgs(&reg, &fake_desk("0x3A")),
        ["0x3a is reserved for the Fake ROM loader on this bus"]
    );
    assert_eq!(
        msgs(&slow, &t),
        ["speed_hz must be one of [100000, 400000] (Fake ROM loader limit 100000)"]
    );
    // The fake MCU has no [coordinator] section: it cannot be the hub.
    let mut hub = fake_desk("0x42");
    hub.nodes[0].mcu = "fake-m0".into();
    hub.nodes[0].board = None;
    hub.nodes[0].flash_map = None;
    let r = crate::check_with(&reg, &hub);
    let v25: Vec<_> = r.errors().filter(|i| i.rule == "V-25").collect();
    assert_eq!(v25.len(), 1, "{r:?}");
    assert_eq!(v25[0].path, "nodes[0].mcu");
    assert_eq!(
        v25[0].message,
        "fake-m0 cannot be the coordinator: its MCU definition has no [coordinator] section"
    );
}

#[test]
fn fake_mcu_and_board_load_from_a_project_without_code_changes() {
    let dir = scratch("fake");
    write(&dir.join("defs/fake-m0/dagon-mcu.toml"), FAKE_MCU);
    std::fs::create_dir_all(dir.join("defs/fake-m0/bootcore")).unwrap();
    let board = FAKE_BOARD.replace("product", "[board.fake-board]\nproduct");
    write(
        &dir.join("dagon.toml"),
        &format!("topology = \"t.yaml\"\nmcu_files = [\"defs/fake-m0/dagon-mcu.toml\"]\n{board}"),
    );
    let r = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap();
    let m = r.mcu("fake-m0").unwrap();
    assert_eq!(m.default_flash_map().name, "fake-1m");
    let boot = r
        .slot("fake-m0", image_kind::BOOTCORE, slot_binding::A)
        .unwrap();
    assert_eq!(
        (
            boot.header_addr,
            boot.load_addr,
            boot.max_payload,
            boot.placement.wire()
        ),
        (0x0010_7E00, 0x0010_0000, 0x7E00, placement::TRAILER)
    );
    let app_b = r.slot("fake-m0", image_kind::APP, slot_binding::B).unwrap();
    assert_eq!(
        (app_b.header_addr, app_b.load_addr),
        (0x0015_0000, 0x0015_0200)
    );
    let b = r.default_board("fake-m0").unwrap();
    assert_eq!(b.id, "fake-board");
    assert_eq!(b.default_link.unwrap().kind, LinkKind::I2c);
    // The builtin set is still there.
    assert!(r.board("nucleo-f401re").is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn project_may_not_redefine_an_id() {
    let dir = scratch("redefine");
    std::fs::create_dir_all(dir.join("m/bootcore")).unwrap();
    let board = FAKE_BOARD.replace("product", "[board.nucleo-f401re]\nproduct");
    write(
        &dir.join("dagon.toml"),
        &board.replace("fake-m0", "stm32f401re"),
    );
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("already defined"), "{e}");

    write(
        &dir.join("m/dagon-mcu.toml"),
        &FAKE_MCU.replace("\"fake-m0\"", "\"rp2040\""),
    );
    write(
        &dir.join("dagon.toml"),
        "mcu_files = [\"m/dagon-mcu.toml\"]\n",
    );
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("already defined"), "{e}");

    write(
        &dir.join("m/dagon-mcu.toml"),
        &FAKE_MCU.replace("0x7F01", "0x0001"),
    );
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("wire_id"), "{e}");

    write(
        &dir.join("m/dagon-mcu.toml"),
        &FAKE_MCU.replace("0x7F01", "0"),
    );
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("wire_id 0"), "{e}");

    write(&dir.join("dagon.toml"), "[board.x]\nid = \"x\"\n");
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("table name"), "{e}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn board_with_an_unknown_mcu_is_rejected() {
    let dir = scratch("nomcu");
    let board = FAKE_BOARD.replace("product", "[board.lone]\nproduct");
    write(&dir.join("dagon.toml"), &board);
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("MCU 'fake-m0' is not defined"), "{e}");
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) fn mcu_err(text: &str) -> String {
    parse_mcu_text(text, &[]).unwrap_err().to_string()
}

#[test]
fn unknown_fields_and_kinds_are_rejected_listing_the_known_values() {
    let e = mcu_err(&FAKE_MCU.replace("gpio_count", "gpio_cnt"));
    assert!(e.contains("unknown field `gpio_cnt`"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("\"self-boot\"", "\"rom-boot\""));
    assert!(
        e.contains("self-boot") && e.contains("vendor-stage2"),
        "{e}"
    );
    let e = mcu_err(&FAKE_MCU.replace("\"trailer\"", "\"footer\""));
    assert!(e.contains("inline") && e.contains("detached"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("\"probe-rs\"", "\"jlink\""));
    assert!(e.contains("uf2-drive") && e.contains("espflash"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("chip = \"FAKE0\"", "chip = \"FAKE0\"\nspeed = 4"));
    assert!(e.contains("unknown field `speed`"), "{e}");
    let bad = FAKE_BOARD.replace("kind = \"i2c\"", "kind = \"spi\"");
    let e = toml::from_str::<BoardDef>(&format!("id = \"x\"\n{bad}"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("i2c") && e.contains("ethernet"), "{e}");
}

#[test]
fn slots_must_fit_and_not_overlap() {
    let e = mcu_err(&FAKE_MCU.replace("at = 0x0015_0000", "at = 0x001E_0000"));
    assert!(e.contains("outside flash map 'fake-1m'"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("at = 0x0015_0000", "at = 0x0012_0000"));
    assert!(e.contains("overlaps"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("0x0010_9000]", "0x0010_7000]"));
    assert!(e.contains("overlaps"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("binding = \"b\"", "binding = \"any\""));
    assert!(e.contains("defined twice"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("size = 0x8000", "size = 0x200"));
    assert!(e.contains("cannot hold"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("[[flash_map]]", "[[flash_mapx]]"));
    assert!(e.contains("flash_mapx"), "{e}");
}

#[test]
fn detached_headers_resolve_from_partitions_and_stay_inside() {
    let esp = reg().mcu("esp32s3").unwrap();
    let names: Vec<&str> = esp
        .partition_table
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["otadata", "factory", "ota_0", "ota_1", "dagon"]);
    let text = include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml");
    let csv = include_str!("../../../../targets/mcu/esp32s3/partitions.csv");
    let e = parse_mcu_text(
        &text.replace("0x2000", "0xFF00"),
        &[("partitions.csv", csv)],
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("outside partition 'dagon'"), "{e}");
    let e = parse_mcu_text(text, &[]).unwrap_err().to_string();
    assert!(e.contains("not embedded"), "{e}");
    let e = parse_mcu_text(
        &text.replace("\"ota_0\"", "\"ota_9\""),
        &[("partitions.csv", csv)],
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("no partition 'ota_9'"), "{e}");
}

#[test]
fn referenced_files_exist_and_stay_inside_the_package() {
    let text = include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml");
    let csv = include_str!("../../../../targets/mcu/esp32s3/partitions.csv");
    // TOML text: `vendor\\bootloader.bin` is the string `vendor\bootloader.bin`.
    for bad in [
        "../rp2040/Cargo.toml",
        "/etc/passwd",
        "C:/x",
        "vendor\\\\bootloader.bin",
        "",
    ] {
        let t = text.replace("vendor/bootloader.bin", bad);
        let e = parse_mcu_text(&t, &[("partitions.csv", csv)])
            .unwrap_err()
            .to_string();
        assert!(e.contains("inside the package"), "{bad}: {e}");
    }
    let dir = scratch("files");
    let pkg = dir.join("pkg");
    write(&pkg.join("dagon-mcu.toml"), text);
    write(
        &pkg.join("partitions.csv"),
        include_str!("../../../../targets/mcu/esp32s3/partitions.csv"),
    );
    write(&pkg.join("vendor/SHA256SUMS"), "");
    let e = parse_mcu_dir(&pkg).unwrap_err().to_string();
    assert!(e.contains("'vendor/bootloader.bin' is not a file"), "{e}");
    write(&pkg.join("vendor/bootloader.bin"), "");
    assert_eq!(parse_mcu_dir(&pkg).unwrap().id, "esp32s3");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Path escapes as TOML string bodies (`\\` is one backslash).
const ESCAPES: [&str; 7] = [
    "../../outside",
    "bootcore/../../x",
    "/abs",
    "C:/x",
    "C:x",
    "boot\\\\core",
    "./bootcore",
];

#[test]
fn part_folders_and_names_stay_inside_the_package() {
    for bad in ESCAPES {
        let t = FAKE_MCU.replace("sub = \"bootcore\"", &format!("sub = \"{bad}\""));
        let e = mcu_err(&t);
        assert!(e.contains("inside the package"), "{bad}: {e}");
    }
    for bad in ["../x", "a/b", "a\\\\b", "", "C:x"] {
        let t = FAKE_MCU.replace("name = \"bootcore\"", &format!("name = \"{bad}\""));
        let e = mcu_err(&t);
        assert!(e.contains("part name/bin"), "{bad}: {e}");
        let t = FAKE_MCU.replace("sub = \"bootcore\"", &format!("bin = \"{bad}\""));
        let e = mcu_err(&t);
        assert!(e.contains("part name/bin"), "{bad}: {e}");
    }
}

#[test]
fn part_folders_exist_on_disk_and_symlinks_cannot_leave() {
    let dir = scratch("parts");
    let pkg = dir.join("pkg");
    write(&pkg.join("dagon-mcu.toml"), FAKE_MCU);
    let e = parse_mcu_dir(&pkg).unwrap_err().to_string();
    assert!(e.contains("'bootcore' is not a folder"), "{e}");
    std::fs::create_dir_all(pkg.join("bootcore")).unwrap();
    assert_eq!(parse_mcu_dir(&pkg).unwrap().id, "fake-m0");

    let board = dir.join("board");
    let text = format!("id = \"b\"\n{FAKE_BOARD}\n[[part]]\nname = \"factory\"\nsub = \"fw\"\n");
    write(&board.join("dagon-board.toml"), &text);
    let e = parse_board_dir(&board).unwrap_err().to_string();
    assert!(e.contains("'fw' is not a folder"), "{e}");
    for bad in ESCAPES {
        write(
            &board.join("dagon-board.toml"),
            &text.replace("\"fw\"", &format!("\"{bad}\"")),
        );
        let e = parse_board_dir(&board).unwrap_err().to_string();
        assert!(e.contains("inside the package"), "{bad}: {e}");
    }

    // A folder link pointing out of the package (skipped where links need
    // privileges the test run lacks, e.g. Windows without developer mode).
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    std::fs::remove_dir_all(pkg.join("bootcore")).unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(dir.join("outside"), pkg.join("bootcore"));
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(dir.join("outside"), pkg.join("bootcore"));
    match linked {
        Ok(()) => {
            let e = parse_mcu_dir(&pkg).unwrap_err().to_string();
            assert!(e.contains("'bootcore' is not a folder inside"), "{e}");
        }
        Err(e) => eprintln!("symlink case skipped: {e}"),
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inline_project_boards_have_no_parts() {
    let dir = scratch("inline-parts");
    let board = FAKE_BOARD
        .replace("mcu = \"fake-m0\"", "mcu = \"rp2040\"")
        .replace("0x20_0000", "0x40_0000")
        .replace("product", "[board.mine]\nproduct");
    write(
        &dir.join("dagon.toml"),
        &format!("{board}\n[[board.mine.part]]\nname = \"factory\"\n"),
    );
    let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
    assert!(e.msg.contains("no parts"), "{e}");
    write(&dir.join("dagon.toml"), &board);
    let r = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap();
    assert_eq!(r.board("mine").unwrap().mcu, "rp2040");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn flash_maps_and_detached_headers_past_u32_are_rejected() {
    for (base, bytes) in [("0xFFF0_0000", "0x10_0000"), ("0xFFFF_0000", "0x2_0000")] {
        let t = FAKE_MCU.replace(
            "base = 0x0010_0000\nbytes = 0x10_0000",
            &format!("base = {base}\nbytes = {bytes}"),
        );
        let e = mcu_err(&t);
        assert!(
            e.contains("flash map 'fake-1m'") && e.contains("32-bit"),
            "{e}"
        );
    }
    let e = mcu_err(&FAKE_MCU.replace("bytes = 0x10_0000", "bytes = 0"));
    assert!(e.contains("is empty"), "{e}");
    let detached = |at: &str| {
        FAKE_MCU.replace(
            "header = \"inline\"\nat = 0x0015_0000",
            &format!("header = \"detached\"\nheader_at = {at}\nat = 0x0015_0000"),
        )
    };
    let e = mcu_err(&detached("0xFFFF_FF00"));
    assert!(e.contains("header at 0xffffff00 runs past"), "{e}");
    // Inside u32 but outside the map: the map check still catches it.
    let e = mcu_err(&detached("0xFFFF_FD00"));
    assert!(
        e.contains("header") && e.contains("outside flash map"),
        "{e}"
    );
    let ok = detached("0x0019_0000");
    assert_eq!(
        parse_mcu_text(&ok, &[]).unwrap().slots[2].header_addr,
        0x0019_0000
    );
}

#[test]
fn addresses_near_u32_max_are_errors_not_overflows() {
    let t = FAKE_MCU.replace("at = 0x0015_0000", "at = 0xFFFF_FF00");
    let e = mcu_err(&t);
    assert!(e.contains("32-bit address space"), "{e}");
    let t = FAKE_MCU.replace(
        "at = 0x0010_0000\nsize = 0x8000",
        "at = 0xFFFF_F000\nsize = 0x1000",
    );
    // Ends exactly at 2^32: one past the address space.
    let e = mcu_err(&t);
    assert!(e.contains("32-bit address space"), "{e}");
    let t = FAKE_MCU.replace(
        "at = 0x0010_0000\nsize = 0x8000",
        "at = 0xFFFF_E000\nsize = 0x1000",
    );
    let e = mcu_err(&t);
    assert!(e.contains("outside flash map"), "{e}");
    let text = include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml");
    let csv = include_str!("../../../../targets/mcu/esp32s3/partitions.csv")
        .replace("0x710000, 64K", "0xFFFFF000, 0x4000");
    let e = parse_mcu_text(text, &[("partitions.csv", &csv)])
        .unwrap_err()
        .to_string();
    assert!(e.contains("32-bit address space"), "{e}");
}

#[test]
fn the_json_form_round_trips_and_is_validated_again() {
    // Doctor commands stay on this machine; everything else comes back.
    let without_doctor = |r: &Registry| -> Vec<McuDef> {
        r.mcus()
            .iter()
            .map(|m| McuDef {
                doctor: Vec::new(),
                ..m.clone()
            })
            .collect()
    };
    let back = Registry::from_json(&reg().to_json()).unwrap();
    assert_eq!(back.mcus(), without_doctor(reg()));
    assert_eq!(back.boards(), reg().boards());
    // A project registry (MCU file + inline board) survives too.
    let project = fake_registry("json", 400_000);
    let back = Registry::from_json(&project.to_json()).unwrap();
    assert_eq!(back.mcus(), without_doctor(&project));
    assert_eq!(back.boards(), project.boards());
    // Every definition is checked again: a bad sector in transit fails.
    let bad = reg().to_json().replace("sector = 4096", "sector = 128");
    let e = Registry::from_json(&bad).unwrap_err();
    assert!(e.msg.contains("UF2 builder"), "{e}");
    assert!(Registry::from_json("{\"mcus\": []}").is_err());
}

#[test]
fn each_mcu_records_where_its_definition_came_from() {
    assert_eq!(reg().mcu_origin("rp2040"), Some(Origin::Builtin));
    let project = fake_registry("origin", 400_000);
    assert_eq!(project.mcu_origin("rp2040"), Some(Origin::Builtin));
    assert_eq!(project.mcu_origin("fake-m0"), Some(Origin::Project));
    assert_eq!(project.mcu_origin("nope"), None);
    let tree = Registry::load(&[Source::Tree(&source_root())]).unwrap();
    assert_eq!(tree.mcu_origin("esp32s3"), Some(Origin::Tree));
    // Definitions received as JSON are never trusted to run commands.
    let back = Registry::from_json(&reg().to_json()).unwrap();
    assert_eq!(back.mcu_origin("rp2040"), Some(Origin::Project));
    assert_eq!(project.board_origin("fake-board"), Some(Origin::Project));
    assert_eq!(tree.board_origin("m5stopwatch"), Some(Origin::Tree));
    assert_eq!(reg().board_origin("nucleo-f401re"), Some(Origin::Builtin));
}

#[test]
fn the_json_form_carries_no_path_comment_or_command() {
    let dir = scratch("public");
    let secret = "sk-ant-FAKE-TOKEN-0123";
    let mcu = FAKE_MCU.replace(
        "id = \"fake-m0\"",
        &format!(
            "# kept at {} with {secret}\nid = \"fake-m0\"",
            dir.display()
        ),
    );
    let doctor = format!(
        "\n[[doctor]]\nwhat = \"tool\"\nprobe = [\"{}\", \"{secret}\"]\nexpect = \"runs\"\nhint = \"{secret}\"\n",
        dir.join("tool.exe").display().to_string().replace('\\', "/")
    );
    write(&dir.join("m/dagon-mcu.toml"), &format!("{mcu}{doctor}"));
    std::fs::create_dir_all(dir.join("m/bootcore")).unwrap();
    let board = FAKE_BOARD.replace("product", "[board.fake-board]\nproduct");
    write(
        &dir.join("dagon.toml"),
        &format!("mcu_files = [\"m/dagon-mcu.toml\"]\n{board}"),
    );
    let project = Registry::load(&[
        Source::Tree(&source_root()),
        Source::Project(&dir.join("dagon.toml")),
    ])
    .unwrap();
    assert!(!project.mcu("fake-m0").unwrap().doctor.is_empty());
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    for (what, json) in [("builtin", reg().to_json()), ("loaded", project.to_json())] {
        let plain = json.replace("\\\\", "\\");
        for local in [
            dir.display().to_string(),
            source_root().canonicalize().unwrap().display().to_string(),
            source_root().display().to_string(),
            home.clone(),
            "targets/mcu".into(),
            "dagon.toml".into(),
        ] {
            assert!(
                local.is_empty() || !plain.contains(&local),
                "{what} JSON has {local}"
            );
        }
        for word in [secret, "doctor", "rustup", "tool.exe", "# "] {
            assert!(!json.contains(word), "{what} JSON has {word}");
        }
    }
    // Labels say where a definition came from without saying where it is.
    let form: serde_json::Value = serde_json::from_str(&project.to_json()).unwrap();
    let labels: Vec<&str> = form["mcus"]
        .as_array()
        .unwrap()
        .iter()
        .chain(form["boards"].as_array().unwrap())
        .map(|d| d["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"source-tree MCU rp2040"), "{labels:?}");
    assert!(labels.contains(&"project MCU fake-m0"), "{labels:?}");
    assert!(labels.contains(&"project board fake-board"), "{labels:?}");
    std::fs::remove_dir_all(dir).unwrap();
}
