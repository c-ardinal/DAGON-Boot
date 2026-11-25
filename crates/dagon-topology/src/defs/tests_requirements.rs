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

/// A board `[flash]` table with a checked and an unverified erase time.
const FLASH: &str = "\n[flash]\npart = \"W25Q16JV\"\nerase_regions = [\n\
    { start = 0x0010_0000, len = 0x8000, unit = 0x1000, unit_max_ms = 400 },\n\
    { start = 0x0010_8000, len = 0x8000, unit = 0x1000, unit_max_ms = { unverified = 400 } },\n]\n";

#[test]
fn every_numeric_field_has_a_declared_requirement_and_back() {
    let (mcu, board) = requirements::declared_fields();
    let uf2 = uf2_mcu("0x1000");
    let detached = detached_mcu();
    let isolated = isolated_mcu();
    let (clock_mcu, clock_board) = super::tests_clock::clock_texts();
    let hal_clock_mcu = super::tests_hal_clock::hal_clock_text();
    let (mut mcu_texts, mut board_texts) = builtin_texts();
    mcu_texts.extend([
        FAKE_MCU,
        uf2.as_str(),
        detached.as_str(),
        isolated.as_str(),
        clock_mcu.as_str(),
        hal_clock_mcu.as_str(),
    ]);
    let board_fake = format!("id = \"x\"\n{FAKE_BOARD}");
    let boot_fake = boot_board();
    let flash_fake = format!("{board_fake}{FLASH}");
    board_texts.extend([
        board_fake.as_str(),
        boot_fake.as_str(),
        flash_fake.as_str(),
        clock_board.as_str(),
    ]);
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

/// An MCU with relay links keeps its peer records in the app log: a face
/// holds them with the copy of a compaction and the room of a trial. An
/// MCU without relay links needs no such room.
#[test]
fn a_relaying_mcus_log_face_holds_its_peer_records() {
    let rule = MCU
        .iter()
        .find(|r| r.engine == "relay peer records")
        .unwrap();
    let reg = super::Registry::builtin();
    let mut m = reg.mcu("rp2040").unwrap().clone();
    assert!(!m.relay_links.is_empty());
    let min = requirements::relay_log_face_min(m.watchdog.unwrap().trial_attempts);
    // 32 bytes of header and 46 records of 32 bytes, with 3 attempts.
    assert_eq!((m.watchdog.unwrap().trial_attempts, min), (3, 1504));
    assert!((rule.check)(&m).is_ok());
    let log = m.layout.app_log.as_mut().unwrap();
    log.face_size = min;
    assert!((rule.check)(&m).is_ok());
    m.layout.app_log.as_mut().unwrap().face_size = min - 1;
    let e = (rule.check)(&m).unwrap_err();
    assert!(e.contains("peer records") && e.contains("1504"), "{e}");
    m.relay_links.clear();
    assert!((rule.check)(&m).is_ok());
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
        ("[0, 1, 2, 4, 5]", "[0, 1, 2, 4, 4]"),
        // One pin pair under two controllers: which one a node uses would
        // depend on the order of the list.
        (
            "{ controller = \"i2c1\", sda = \"PB7\", scl = \"PB6\" },",
            "{ controller = \"i2c1\", sda = \"PB4\", scl = \"PA8\" },",
        ),
    ] {
        let e = parse_mcu_text(&f4.replace(from, to), &[]).unwrap_err();
        assert!(e.msg.contains("V-18/V-20"), "{to}: {e}");
    }
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
fn coordinator_pins_come_from_the_dagon_buses() {
    let rp = Registry::builtin().mcu("rp2040").unwrap();
    let lcd =
        include_str!("../../../../targets/boards/waveshare-rp2040-touch-lcd-1.28/dagon-board.toml");
    let board = |t: &str| toml::from_str::<BoardDef>(t).unwrap();
    // The pins the page used before it read them from the definition.
    let h = board(lcd).coordinator_pins(rp).unwrap();
    assert_eq!(
        (h.i2c.controller.as_str(), h.i2c.sda, h.i2c.scl),
        ("i2c0", 16, 17)
    );
    assert_eq!((h.pio_uart.tx, h.pio_uart.rx), (26, 27));
    // Moving a DAGON bus moves the coordinator pins with it.
    let moved = board(&lcd.replace("tx = \"GPIO26\"", "tx = \"GPIO29\""));
    assert_eq!(moved.coordinator_pins(rp).unwrap().pio_uart.tx, 29);
    let e = requirements::check_board(
        &board(&lcd.replace("tx = \"GPIO26\"", "tx = \"GPIO30\"")),
        rp,
    )
    .unwrap_err();
    assert!(e.contains("below gpio_count"), "{e}");
    // No PIO UART bus of DAGON's, or an MCU that cannot be the coordinator: no pins.
    let hw = board(&lcd.replace("controller = \"pio0\"", "controller = \"uart1\""));
    assert_eq!(hw.coordinator_pins(rp), None);
    let f4 = Registry::builtin().mcu("stm32f401re").unwrap();
    assert_eq!(board(lcd).coordinator_pins(f4), None);
    // The pins are not written a second time.
    let twice = format!("{lcd}\n[coordinator]\npio_uart = {{ tx = 26, rx = 27 }}\n");
    assert!(toml::from_str::<BoardDef>(&twice).is_err());
}

#[test]
fn link_rates_come_from_one_clock_within_the_definitions_bounds() {
    let rp = include_str!("../../../../targets/mcu/rp2040/dagon-mcu.toml");
    let ctl = "[link_i2c.controller]\nclock = \"app\"\nmin_period = 20\n";
    assert!(
        rp.contains(ctl),
        "the rp2040 [link_i2c.controller] block moved"
    );
    for (to, says) in [
        // Two clocks, or none.
        (
            "[link_i2c.controller]\nclock = \"app\"\nclock_hz = 125_000_000\nmin_period = 20\n",
            "exactly one of clock_hz",
        ),
        (
            "[link_i2c.controller]\nmin_period = 20\n",
            "exactly one of clock_hz",
        ),
        (
            "[link_i2c.controller]\nclock = \"app\"\nmin_period = 0\n",
            "min_period must be at least 1",
        ),
        // A fixed 6 MHz clock cannot make 400 kHz in 20 cycles a period.
        (
            "[link_i2c.controller]\nclock_hz = 6_000_000\nmin_period = 20\n",
            "cannot run SCL at 400000",
        ),
    ] {
        let e = parse_mcu_text(&rp.replace(ctl, to), &[]).unwrap_err();
        assert!(e.msg.contains("V-20") && e.msg.contains(says), "{to}: {e}");
    }
    // A fixed 125 MHz clock makes both speeds within the bounds.
    let fixed = rp.replace(
        ctl,
        "[link_i2c.controller]\nclock_hz = 125_000_000\nmin_period = 20\n",
    );
    parse_mcu_text(&fixed, &[]).unwrap();
    let e = parse_mcu_text(
        &rp.replace("slow_ppm = 1_000_000", "slow_ppm = 1_000_001"),
        &[],
    )
    .unwrap_err();
    assert!(e.msg.contains("slow_ppm must be at most 1000000"), "{e}");
    // The UART's bounds are the definition's: on a fixed 125 MHz clock
    // the RP2040's UART runs 115200 at 115741 bit/s (+0.47 %), within 2 %
    // but not within 0.1 %.
    let uart = "[link_uart]\nclock = \"app\"\n";
    assert!(rp.contains(uart), "the rp2040 [link_uart] block moved");
    let fixed = rp.replace(uart, "[link_uart]\nclock_hz = 125_000_000\n");
    parse_mcu_text(&fixed, &[]).unwrap();
    let tight = fixed.replacen("fast_ppm = 20_000", "fast_ppm = 1_000", 1);
    let e = parse_mcu_text(&tight, &[]).unwrap_err();
    assert!(
        e.msg.contains("V-26") && e.msg.contains("cannot run at 115200 from -2 % to +0.1 %"),
        "{e}"
    );
}
