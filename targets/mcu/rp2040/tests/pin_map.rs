//! The pins of every board with this MCU (`header`, `[[pin]]`, `i2c` and
//! `[[bus]]` in its `dagon-board.toml`) are GPIOs of the RP2040, and a bus
//! pin carries the signal the board gives it in Raspberry Pi's chip data:
//! the function-select names of each GPIO in the RP2040's SVD
//! (`fixtures/funcsel.txt`). `sda = "GPIO16"` on `i2c0` is a GPIO whose
//! functions include `i2c0_sda`. A PIO's pins may be any GPIO with that
//! PIO's function.

use std::path::Path;

/// The function-select names of each GPIO: (GPIO, names).
fn chip_data() -> Vec<(&'static str, Vec<&'static str>)> {
    include_str!("fixtures/funcsel.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let mut words = l.split(' ');
            (words.next().unwrap(), words.collect())
        })
        .collect()
}

/// The SVD's name of `signal` of `controller` on `pin`: a PIO's functions
/// are numbered by GPIO (`pio0_26`), the others named by signal.
fn function(controller: &str, signal: &str, pin: &str) -> String {
    if controller.starts_with("pio") {
        return format!("{controller}_{}", pin.trim_start_matches("GPIO"));
    }
    let signal = match (controller.starts_with("spi"), signal) {
        (true, "sck") => "sclk",
        (true, "mosi") => "tx",
        (true, "miso") => "rx",
        (true, "cs") => "ss_n",
        (_, s) => s,
    };
    format!("{controller}_{signal}")
}

/// Whether the chip data routes `signal` of `controller` to `pin`.
fn routed(controller: &str, signal: &str, pin: &str) -> bool {
    let want = function(controller, signal, pin);
    chip_data()
        .iter()
        .any(|(gpio, names)| *gpio == pin && names.iter().any(|n| *n == want))
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
        if t["mcu"].as_str() == Some("rp2040") {
            found.push((t["id"].as_str().unwrap().to_string(), t));
        }
    }
    found
}

/// The rows of list `key` of board `t`.
fn rows<'a>(t: &'a toml::Table, key: &str) -> impl Iterator<Item = &'a toml::Value> {
    t.get(key).and_then(|v| v.as_array()).into_iter().flatten()
}

const SIGNALS: [&str; 10] = [
    "sda", "scl", "tx", "rx", "rts", "cts", "sck", "mosi", "miso", "cs",
];

#[test]
fn the_chip_data_has_every_gpio_of_the_definition() {
    let t: toml::Table = include_str!("../dagon-mcu.toml").parse().unwrap();
    let count = usize::try_from(t["gpio_count"].as_integer().unwrap()).unwrap();
    let chip = chip_data();
    assert_eq!(chip.len(), count);
    for (n, (gpio, names)) in chip.iter().enumerate() {
        assert_eq!(*gpio, format!("GPIO{n}"));
        assert!(!names.is_empty(), "{gpio}");
    }
}

#[test]
fn every_board_pin_is_a_gpio_of_the_chip() {
    let chip = chip_data();
    let boards = boards();
    assert!(!boards.is_empty());
    let mut out = Vec::new();
    let mut pins = 0;
    for (id, t) in &boards {
        let named = ["header", "pin"]
            .into_iter()
            .flat_map(|k| rows(t, k))
            .filter_map(|r| r.get("pin").and_then(|p| p.as_str()));
        let led = t
            .get("boot")
            .and_then(|b| b.get("led"))
            .and_then(|p| p.as_str());
        for pin in named.chain(led) {
            pins += 1;
            if !chip.iter().any(|(gpio, _)| *gpio == pin) {
                out.push(format!("{id}: {pin}"));
            }
        }
    }
    assert!(pins > 0);
    assert!(out.is_empty(), "{out:#?}");
}

#[test]
fn every_bus_pin_carries_its_signal_in_the_chip_data() {
    let boards = boards();
    assert!(!boards.is_empty());
    let mut out = Vec::new();
    let mut checked = 0;
    for (id, t) in &boards {
        for row in ["i2c", "bus"].into_iter().flat_map(|k| rows(t, k)) {
            let Some(controller) = row.get("controller").and_then(|c| c.as_str()) else {
                continue;
            };
            for signal in SIGNALS {
                let Some(pin) = row.get(signal).and_then(|p| p.as_str()) else {
                    continue;
                };
                checked += 1;
                if !routed(controller, signal, pin) {
                    out.push(format!("{id}: {controller} {signal} on {pin}"));
                }
            }
        }
    }
    assert!(checked > 0);
    assert!(out.is_empty(), "{out:#?}");
}

/// The check refuses a pin the chip data gives another signal.
#[test]
fn a_pin_with_another_signal_is_not_routed() {
    assert!(routed("i2c0", "sda", "GPIO16"));
    assert!(!routed("i2c0", "scl", "GPIO16"));
    assert!(!routed("i2c1", "sda", "GPIO16"));
    assert!(routed("spi1", "sck", "GPIO10"));
    assert!(!routed("spi0", "sck", "GPIO10"));
    assert!(routed("pio0", "tx", "GPIO26"));
    assert!(!routed("i2c0", "sda", "GPIO30"));
}
