//! What the definition says of the chip's clocks and ports is in ST's chip
//! data for the STM32F401RE, as stm32-metapac ships it:
//!
//! - every PLL divider, multiplier and flash wait state of a stage's
//!   `clock` is a value ST lists for its register field (RCC_PLLCFGR
//!   PLLM, PLLN, PLLP, PLLQ; FLASH_ACR LATENCY);
//! - every `[[link_port]]` is a peripheral of the chip;
//! - every `[rom_i2c]` pair is a controller's SDA and SCL pins.

use stm32_metapac::metadata::ir::Enum;
use stm32_metapac::metadata::METADATA;

fn definition() -> toml::Table {
    include_str!("../dagon-mcu.toml").parse().unwrap()
}

/// The values ST lists for field `field` of register `register` of
/// peripheral `peripheral`.
fn field_values(peripheral: &str, register: &str, field: &str) -> &'static Enum {
    let p = METADATA
        .peripherals
        .iter()
        .find(|p| p.name == peripheral)
        .unwrap();
    let ir = p.registers.as_ref().unwrap().ir;
    let set = ir.fieldsets.iter().find(|s| s.name == register).unwrap();
    let f = set.fields.iter().find(|f| f.name == field).unwrap();
    let name = f.enumm.unwrap();
    ir.enums.iter().find(|e| e.name == name).unwrap()
}

/// Whether ST lists `value` for the field: its variants are named by the
/// number they stand for (`DIV16`, `MUL336`, `WS2`).
fn listed(e: &Enum, prefix: &str, value: i64) -> bool {
    e.variants
        .iter()
        .any(|v| v.name.strip_prefix(prefix) == Some(value.to_string().as_str()))
}

/// The PLL key of a clock, its register field and the prefix of the
/// field's value names.
const PLL_FIELDS: [(&str, &str, &str, &str, &str); 5] = [
    ("m", "RCC", "Pllcfgr", "pllm", "DIV"),
    ("n", "RCC", "Pllcfgr", "plln", "MUL"),
    ("p", "RCC", "Pllcfgr", "pllp", "DIV"),
    ("q", "RCC", "Pllcfgr", "pllq", "DIV"),
    ("flash_wait_states", "FLASH", "Acr", "latency", "WS"),
];

/// What ST's data does not list among the PLL values of `pll`.
fn unlisted(stage: &str, pll: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    for (key, peripheral, register, field, prefix) in PLL_FIELDS {
        let Some(v) = pll.get(key).and_then(|v| v.as_integer()) else {
            continue;
        };
        if !listed(field_values(peripheral, register, field), prefix, v) {
            out.push(format!("{stage}: pll {key} = {v}"));
        }
    }
    out
}

#[test]
fn every_pll_value_is_one_st_lists_for_its_field() {
    let d = definition();
    let mut out = Vec::new();
    let mut plls = 0;
    for stage in ["stage0", "bootcore", "app"] {
        let clocks = d[stage]["clock"].as_array().unwrap();
        for pll in clocks.iter().filter_map(|c| c.get("pll")) {
            plls += 1;
            out.extend(unlisted(stage, pll));
        }
    }
    assert!(plls > 0);
    assert!(out.is_empty(), "{out:#?}");
}

/// The check refuses values the fields cannot hold.
#[test]
fn a_value_outside_a_field_is_not_listed() {
    let pll: toml::Table = "m = 1\nn = 433\np = 3\nq = 16\nflash_wait_states = 16"
        .parse()
        .unwrap();
    assert_eq!(unlisted("x", &toml::Value::Table(pll)).len(), 5);
}

/// ST's name of a peripheral the definition names `usb-otg-fs` or `i2c1`.
fn st_name(name: &str) -> String {
    name.replace('-', "_").to_ascii_uppercase()
}

#[test]
fn every_link_port_is_a_peripheral_of_the_chip() {
    let d = definition();
    let ports = d["link_port"].as_array().unwrap();
    assert!(!ports.is_empty());
    let missing: Vec<&str> = ports
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .filter(|n| {
            let st = st_name(n);
            !METADATA.peripherals.iter().any(|p| p.name == st)
        })
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
}

/// Whether ST's data routes `signal` of `controller` to `pin`.
fn routed(controller: &str, signal: &str, pin: &str) -> bool {
    METADATA
        .peripherals
        .iter()
        .filter(|p| p.name == st_name(controller))
        .flat_map(|p| p.pins)
        .any(|p| p.pin == pin && p.signal.eq_ignore_ascii_case(signal))
}

#[test]
fn every_rom_loader_pair_is_a_controllers_sda_and_scl() {
    let d = definition();
    let pairs = d["rom_i2c"]["pairs"].as_array().unwrap();
    assert!(!pairs.is_empty());
    let mut out = Vec::new();
    for pair in pairs {
        let controller = pair["controller"].as_str().unwrap();
        for signal in ["sda", "scl"] {
            let pin = pair[signal].as_str().unwrap();
            if !routed(controller, signal, pin) {
                out.push(format!("{controller} {signal} on {pin}"));
            }
        }
    }
    assert!(out.is_empty(), "{out:#?}");
    assert!(!routed("i2c3", "scl", "PB4"));
}
