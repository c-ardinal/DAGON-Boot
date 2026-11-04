//! Every numeric definition field has a declared engine requirement, and
//! every requirement rejects what breaks its engine.

use std::collections::BTreeSet;

use super::load::{builtin_texts, parse_mcu_text};
use super::requirements::{self, LOG_FACE_MIN, MCU};
use super::tests::{mcu_err, FAKE_BOARD, FAKE_MCU};
use super::tests_isolation::{boot_board, isolated_mcu};
use super::{BoardDef, Registry};

/// The fake MCU written to a UF2 drive.
fn uf2_mcu(sector: &str) -> String {
    FAKE_MCU.replace(
        "kind = \"probe-rs\"\nchip = \"FAKE0\"",
        &format!(
            "kind = \"uf2-drive\"\nfamily = 0x1234_5678\nsector = {sector}\n\
             info_file = \"INFO_UF2.TXT\"\ninfo_header = \"UF2 Bootloader\"\nboard_id = \"FAKE\""
        ),
    )
}

/// The fake MCU with a detached header at an absolute address.
fn detached_mcu() -> String {
    FAKE_MCU.replace(
        "header = \"inline\"\nat = 0x0015_0000",
        "header = \"detached\"\nheader_at = 0x0019_0000\nat = 0x0015_0000",
    )
}

/// Numeric leaves as `a.b[].c` paths.
fn numeric_paths(v: &toml::Value, at: &str, out: &mut BTreeSet<String>) {
    let join = |k: &str| {
        if at.is_empty() {
            k.to_string()
        } else {
            format!("{at}.{k}")
        }
    };
    match v {
        toml::Value::Integer(_) | toml::Value::Float(_) => {
            out.insert(at.to_string());
        }
        toml::Value::Table(t) => t.iter().for_each(|(k, v)| numeric_paths(v, &join(k), out)),
        toml::Value::Array(a) => a
            .iter()
            .for_each(|v| numeric_paths(v, &format!("{at}[]"), out)),
        _ => {}
    }
}

fn paths(texts: &[&str]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for t in texts {
        numeric_paths(&t.parse::<toml::Value>().unwrap(), "", &mut out);
    }
    out
}

#[test]
fn every_numeric_field_has_a_declared_requirement_and_back() {
    let (mcu, board) = requirements::declared_fields();
    let uf2 = uf2_mcu("0x1000");
    let detached = detached_mcu();
    let isolated = isolated_mcu();
    let (mut mcu_texts, mut board_texts) = builtin_texts();
    mcu_texts.extend([FAKE_MCU, uf2.as_str(), detached.as_str(), isolated.as_str()]);
    let board_fake = format!("id = \"x\"\n{FAKE_BOARD}");
    let boot_fake = boot_board();
    board_texts.extend([board_fake.as_str(), boot_fake.as_str()]);
    for (texts, declared) in [(mcu_texts, mcu), (board_texts, board)] {
        let found = paths(&texts);
        let declared: BTreeSet<String> = declared
            .iter()
            .filter(|f| !f.starts_with("partitions.csv"))
            .map(|f| f.to_string())
            .collect();
        let undeclared: Vec<_> = found.difference(&declared).collect();
        assert!(
            undeclared.is_empty(),
            "no engine requirement for {undeclared:?}"
        );
        let unused: Vec<_> = declared.difference(&found).collect();
        assert!(
            unused.is_empty(),
            "requirements name unknown fields {unused:?}"
        );
    }
}

#[test]
fn builtin_definitions_meet_every_requirement() {
    for m in Registry::builtin().mcus() {
        requirements::check_mcu(m).unwrap();
    }
    for b in Registry::builtin().boards() {
        requirements::check_board(b, Registry::builtin().mcu(&b.mcu).unwrap()).unwrap();
    }
}

#[test]
fn uf2_sector_must_suit_the_uf2_builder() {
    for bad in ["0", "0x80", "0xFF", "0x300", "0x1001"] {
        let e = mcu_err(&uf2_mcu(bad));
        assert!(
            e.contains("power of two") && e.contains("[UF2 builder]"),
            "{bad}: {e}"
        );
    }
    for ok in ["0x100", "0x1000", "0x1_0000"] {
        parse_mcu_text(&uf2_mcu(ok), &[]).unwrap();
    }
    // A last sector ending at 2^32 would overflow the builder's u32 range.
    let mut m = parse_mcu_text(&uf2_mcu("0x2_0000"), &[]).unwrap();
    m.flash_maps[0].base = 0xFFFF_0000;
    m.flash_maps[0].bytes = 0xF000;
    let uf2 = MCU
        .iter()
        .find(|r| r.fields.contains(&"writer.sector"))
        .unwrap();
    let e = (uf2.check)(&m).unwrap_err();
    assert!(e.contains("past 2^32"), "{e}");
    m.flash_maps[0].bytes = 0x8000;
    m.flash_maps[0].base = 0xFFFC_0000;
    assert!((uf2.check)(&m).is_ok());
}

#[test]
fn log_faces_hold_a_provisioned_face() {
    let small = FAKE_MCU.replace(
        "face_size = 0x1000",
        &format!("face_size = {}", LOG_FACE_MIN - 1),
    );
    let e = mcu_err(&small);
    assert!(e.contains("face_size") && e.contains("96"), "{e}");
    let exact = FAKE_MCU.replace("face_size = 0x1000", &format!("face_size = {LOG_FACE_MIN}"));
    parse_mcu_text(&exact, &[]).unwrap();
}

#[test]
fn flash_maps_suit_the_flash_image() {
    let e = mcu_err(&FAKE_MCU.replace("base = 0x0010_0000", "base = 0x0010_0080"));
    assert!(e.contains("not a multiple of 0x100"), "{e}");
    let e = mcu_err(&FAKE_MCU.replace("bytes = 0x10_0000", "bytes = 0"));
    assert!(e.contains("is empty"), "{e}");
}

#[test]
fn partitions_are_not_empty_and_fit() {
    let text = include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml");
    let csv = include_str!("../../../../targets/mcu/esp32s3/partitions.csv");
    for (from, to) in [
        ("0x410000, 3M", "0x410000, 0"),
        ("0x410000, 3M", "0xFFFF0000, 0x20000"),
    ] {
        let e = parse_mcu_text(text, &[("partitions.csv", &csv.replace(from, to))])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("partition 'ota_1'") && e.contains("[partition resolution]"),
            "{e}"
        );
    }
}

#[test]
fn partition_table_sector_is_placed_like_any_region() {
    let text = include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml");
    let csv = include_str!("../../../../targets/mcu/esp32s3/partitions.csv");
    let parse = |t: &str| parse_mcu_text(t, &[("partitions.csv", csv)]);
    let e = parse(&text.replace("table_at = 0x8000", "table_at = 0x110800")).unwrap_err();
    assert!(
        e.msg.contains("partition table") && e.msg.contains("overlaps"),
        "{e}"
    );
    let e = parse(&text.replace("table_at = 0x8000", "table_at = 0xFFF800")).unwrap_err();
    assert!(e.msg.contains("outside flash map"), "{e}");
}

#[test]
fn coordinator_and_rom_numbers_are_usable() {
    let rp = include_str!("../../../../targets/mcu/rp2040/dagon-mcu.toml");
    for (from, to) in [
        ("pio_uart_state_machines = 2", "pio_uart_state_machines = 0"),
        ("pio_uart_state_machines = 2", "pio_uart_state_machines = 9"),
        ("uart0_tx = [0, 12, 16, 28]", "uart0_tx = [0, 12, 16, 30]"),
    ] {
        let e = parse_mcu_text(&rp.replace(from, to), &[]).unwrap_err();
        assert!(e.msg.contains("V-09/V-10"), "{to}: {e}");
    }
    let f4 = include_str!("../../../../targets/mcu/stm32f401re/dagon-mcu.toml");
    for (from, to) in [
        ("addr = 0x39", "addr = 0x07"),
        ("addr = 0x39", "addr = 0x78"),
        ("max_hz = 400_000", "max_hz = 0"),
        ("[0, 1, 2, 3, 5]", "[0, 1, 2, 3, 3]"),
    ] {
        let e = parse_mcu_text(&f4.replace(from, to), &[]).unwrap_err();
        assert!(e.msg.contains("V-18/V-20"), "{to}: {e}");
    }
    let e = mcu_err(&FAKE_MCU.replace("0x7F01", "0"));
    assert!(e.contains("wire_id 0"), "{e}");
}

#[test]
fn board_numbers_are_usable() {
    let m = Registry::builtin().mcu("stm32f401re").unwrap();
    let board = |t: &str| toml::from_str::<BoardDef>(t).unwrap();
    let nucleo = include_str!("../../../../targets/boards/nucleo-f401re/dagon-board.toml");
    for (from, to, what) in [
        ("addr = 0x42", "addr = 0x07", "default_link.addr"),
        (
            "flash_bytes = 0x8_0000",
            "flash_bytes = 0x4_0000",
            "flash_bytes",
        ),
    ] {
        let e = requirements::check_board(&board(&nucleo.replace(from, to)), m).unwrap_err();
        assert!(e.contains(what), "{to}: {e}");
    }
    let watch = include_str!("../../../../targets/boards/m5stopwatch/dagon-board.toml");
    let esp = Registry::builtin().mcu("esp32s3").unwrap();
    let e = requirements::check_board(&board(&watch.replace("115_200", "0")), esp).unwrap_err();
    assert!(e.contains("baud"), "{e}");
}

#[test]
fn coordinator_pins_are_usable() {
    let rp = Registry::builtin().mcu("rp2040").unwrap();
    let lcd =
        include_str!("../../../../targets/boards/waveshare-rp2040-touch-lcd-1.28/dagon-board.toml");
    let board = |t: &str| toml::from_str::<BoardDef>(t).unwrap();
    // The pins the page used before it read them from the definition.
    let h = board(lcd).coordinator.unwrap();
    assert_eq!(
        (h.i2c.controller.as_str(), h.i2c.sda, h.i2c.scl),
        ("i2c0", 16, 17)
    );
    assert_eq!((h.pio_uart.tx, h.pio_uart.rx), (26, 27));
    for (from, to, what) in [
        ("tx = 26", "tx = 30", "below gpio_count"),
        ("tx = 26", "tx = 16", "four different"),
        (
            "{ controller = \"i2c0\", sda = 16, scl = 17 }",
            "{ controller = \"i2c1\", sda = 16, scl = 17 }",
            "none of the board's i2c",
        ),
    ] {
        let e = requirements::check_board(&board(&lcd.replace(from, to)), rp).unwrap_err();
        assert!(e.contains(what), "{to}: {e}");
    }
    let f4 = Registry::builtin().mcu("stm32f401re").unwrap();
    let e = requirements::check_board(&board(lcd), f4).unwrap_err();
    assert!(e.contains("cannot be the 親機"), "{e}");
}
