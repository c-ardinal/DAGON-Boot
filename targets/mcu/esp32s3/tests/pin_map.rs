//! The pins of every board with this MCU (`header`, `i2c` and `[[bus]]` in
//! its `dagon-board.toml`) are GPIOs of the ESP32-S3 in Espressif's chip
//! metadata, as esp-hal uses it (esp-metadata-generated), and can drive or
//! read what the board gives them. A UART's pins may be any GPIO (the GPIO
//! matrix routes them); the ones the chip gives the UART directly (IO MUX)
//! are told apart, which is where a board's UART0 pins come from.

use std::path::Path;

/// One GPIO of the chip: its name, the signals its IO MUX functions read
/// and drive, and whether it can be an input and an output.
struct Gpio {
    name: &'static str,
    reads: Vec<&'static str>,
    drives: Vec<&'static str>,
    input: bool,
    output: bool,
}

/// Espressif's GPIOs of the ESP32-S3.
fn gpios() -> Vec<Gpio> {
    let mut all = Vec::new();
    esp_metadata_generated::for_each_gpio! {
        ($n:literal, $gpio:ident ($($_in_fn:ident => $reads:ident)*)
            ($($_out_fn:ident => $drives:ident)*) ($([$attr:ident])*)) => {
            let attrs = [$(stringify!($attr)),*];
            all.push(Gpio {
                name: stringify!($gpio),
                reads: vec![$(stringify!($reads)),*],
                drives: vec![$(stringify!($drives)),*],
                input: attrs.contains(&"Input"),
                output: attrs.contains(&"Output"),
            });
        };
    }
    all
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
        if t["mcu"].as_str() == Some("esp32s3") {
            found.push((t["id"].as_str().unwrap().to_string(), t));
        }
    }
    found
}

/// The signal a pin carries on a bus, and whether the pin then reads and
/// drives.
const SIGNALS: [(&str, bool, bool); 6] = [
    ("sda", true, true),
    ("scl", true, true),
    ("tx", false, true),
    ("rx", true, false),
    ("rts", false, true),
    ("cts", true, false),
];

/// What is wrong with pin `pin` of `board`, which must read (`input`) and
/// drive (`output`) as said.
fn wrong(gpios: &[Gpio], board: &str, pin: &str, input: bool, output: bool) -> Option<String> {
    let Some(g) = gpios.iter().find(|g| g.name == pin) else {
        return Some(format!("{board}: no {pin} on the chip"));
    };
    ((input && !g.input) || (output && !g.output))
        .then(|| format!("{board}: {pin} cannot be used that way"))
}

/// The rows of `board` naming bus pins.
fn buses(board: &toml::Table) -> Vec<&toml::Value> {
    ["i2c", "bus"]
        .iter()
        .filter_map(|k| board.get(*k).and_then(|v| v.as_array()))
        .flatten()
        .collect()
}

#[test]
fn every_board_pin_is_a_gpio_that_can_carry_its_signal() {
    let (gpios, boards) = (gpios(), boards());
    assert!(gpios.len() > 40 && !boards.is_empty());
    let mut out = Vec::new();
    let mut pins = 0;
    for (id, t) in &boards {
        for h in t
            .get("header")
            .and_then(|h| h.as_array())
            .into_iter()
            .flatten()
        {
            pins += 1;
            out.extend(wrong(&gpios, id, h["pin"].as_str().unwrap(), false, false));
        }
        for row in buses(t) {
            for (signal, input, output) in SIGNALS {
                if let Some(pin) = row.get(signal).and_then(|p| p.as_str()) {
                    pins += 1;
                    out.extend(wrong(&gpios, id, pin, input, output));
                }
            }
        }
    }
    assert!(pins > 0);
    assert!(out.is_empty(), "{out:#?}");
}

/// The check refuses a pin the chip lacks and one that cannot drive.
#[test]
fn a_missing_pin_is_refused() {
    let gpios = gpios();
    assert!(wrong(&gpios, "x", "GPIO22", false, false).is_some());
    assert!(wrong(&gpios, "x", "GPIO49", true, true).is_some());
    assert!(wrong(&gpios, "x", "GPIO43", true, true).is_none());
}

/// Whether the chip gives `pin` the signal `signal` of UART `controller`
/// (`uart0`, `tx`) on the IO MUX, with no GPIO matrix route.
fn direct(gpios: &[Gpio], controller: &str, signal: &str, pin: &str) -> bool {
    let n = controller.trim_start_matches("uart");
    let name = format!("U{n}{}D", signal.to_ascii_uppercase());
    gpios.iter().any(|g| {
        let on = if signal == "tx" { &g.drives } else { &g.reads };
        g.name == pin && on.contains(&name.as_str())
    })
}

/// The M5StopWatch's link is UART0 on the pins the chip gives UART0
/// directly: U0TXD on GPIO43 and U0RXD on GPIO44, the pins the ROM's
/// download mode and the 2nd stage's log use.
#[test]
fn the_m5stopwatch_link_is_on_uart0s_own_pins() {
    let (gpios, boards) = (gpios(), boards());
    let (_, board) = boards.iter().find(|b| b.0 == "m5stopwatch").unwrap();
    let link = buses(board)
        .into_iter()
        .find(|b| b.get("dagon").and_then(|d| d.as_bool()) == Some(true))
        .unwrap();
    let of = |k: &str| link[k].as_str().unwrap();
    assert_eq!(
        (of("controller"), of("tx"), of("rx")),
        ("uart0", "GPIO43", "GPIO44")
    );
    assert!(direct(&gpios, "uart0", "tx", of("tx")));
    assert!(direct(&gpios, "uart0", "rx", of("rx")));
    assert!(!direct(&gpios, "uart0", "tx", of("rx")));
    assert!(!direct(&gpios, "uart1", "tx", of("tx")));
}
