//! The bus pins of every board with this MCU (`i2c` and `[[bus]]` in its
//! `dagon-board.toml`) carry the signal the board gives them in ST's chip
//! data for the STM32F401RE, as stm32-metapac ships it: `sda = "PB4"` on
//! `i2c3` is a pin ST lists for I2C3's SDA.

use std::path::Path;
use stm32_metapac::metadata::METADATA;

/// Whether ST's data routes `signal` of `controller` to `pin`.
fn routed(controller: &str, signal: &str, pin: &str) -> bool {
    METADATA
        .peripherals
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(controller))
        .flat_map(|p| p.pins)
        .any(|p| p.pin == pin && p.signal.eq_ignore_ascii_case(signal))
}

/// The boards of this MCU: (id, definition).
fn boards() -> Vec<(String, toml::Table)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../boards");
    let mut found = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let file = e.unwrap().path().join("dagon-board.toml");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let t: toml::Table = text.parse().unwrap();
        if t["mcu"].as_str() == Some("stm32f401re") {
            found.push((t["id"].as_str().unwrap().to_string(), t));
        }
    }
    found
}

/// What is wrong with the signal pins of `row`, a table naming a
/// `controller` and a pin per signal.
fn wrong(board: &str, row: &toml::Value, signals: &[&str], out: &mut Vec<String>) {
    let Some(controller) = row.get("controller").and_then(|c| c.as_str()) else {
        return;
    };
    for signal in signals {
        let Some(pin) = row.get(signal).and_then(|p| p.as_str()) else {
            continue;
        };
        if !routed(controller, signal, pin) {
            out.push(format!("{board}: {controller} {signal} on {pin}"));
        }
    }
}

const SIGNALS: [&str; 6] = ["sda", "scl", "tx", "rx", "rts", "cts"];

#[test]
fn every_bus_pin_carries_its_signal_in_st_chip_data() {
    let boards = boards();
    assert!(!boards.is_empty());
    let mut out = Vec::new();
    let mut rows = 0;
    for (id, t) in &boards {
        let lists = ["i2c", "bus"].map(|k| t.get(k).and_then(|v| v.as_array()));
        for row in lists.into_iter().flatten().flatten() {
            rows += 1;
            wrong(id, row, &SIGNALS, &mut out);
        }
    }
    assert!(rows > 0);
    assert!(out.is_empty(), "{out:#?}");
}

/// The check refuses a pin ST's data gives another signal.
#[test]
fn a_pin_with_another_signal_is_not_routed() {
    assert!(routed("i2c3", "sda", "PB4"));
    assert!(!routed("i2c3", "scl", "PB4"));
    assert!(!routed("i2c1", "sda", "PB4"));
}
