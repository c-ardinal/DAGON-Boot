//! Parity with the topology tables, validation, and loading new MCUs and
//! boards without code changes. Parity with signing, provisioning, the host
//! and the CLI is in `tests/defs_parity.rs`.

use std::path::{Path, PathBuf};

use super::load::{parse_mcu_dir, parse_mcu_text};
use super::*;
use crate::{boards, check, mcu_id_of, port_kind_of};

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
fn coordinator_and_rom_rules_match_check() {
    let c = reg().mcu("rp2040").unwrap().coordinator.as_ref().unwrap();
    assert_eq!(
        usize::from(c.pio_state_machines),
        check::RP2040_PIO_STATE_MACHINES
    );
    assert_eq!(
        usize::from(c.pio_uart_state_machines),
        check::PIO_UART_STATE_MACHINES
    );
    assert_eq!(c.uart0_tx, check::RP2040_UART0_TX);
    assert_eq!(c.uart0_rx, check::RP2040_UART0_RX);
    let rom = reg().mcu("stm32f401re").unwrap().rom_i2c.as_ref().unwrap();
    assert_eq!(rom.addr, check::STM32_ROM_ADDR);
    assert_eq!(Some(&rom.max_hz), check::I2C_SPEEDS.iter().max());
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

const FAKE_MCU: &str = r#"
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

const FAKE_BOARD: &str = r#"
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

#[test]
fn fake_mcu_and_board_load_from_a_project_without_code_changes() {
    let dir = scratch("fake");
    write(&dir.join("defs/fake-m0/dagon-mcu.toml"), FAKE_MCU);
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

    write(&dir.join("m/dagon-mcu.toml"), &FAKE_MCU.replace("0x7F01", "0"));
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

fn mcu_err(text: &str) -> String {
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
