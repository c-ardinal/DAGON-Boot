//! `[[link_port]]`: the link interfaces an MCU definition names.

use super::tests::{mcu_err, FAKE_MCU};
use super::Registry;

fn with_ports(ports: &str) -> String {
    format!("{FAKE_MCU}\n{ports}")
}

const TWO: &str = r#"
[[link_port]]
index = 0
name = "i2c0"
kind = "i2c"

[[link_port]]
index = 1
name = "uart0"
kind = "uart"
"#;

#[test]
fn ports_load_with_index_name_and_kind() {
    let m = super::load::parse_mcu_text(&with_ports(TWO), &[]).unwrap();
    let got: Vec<_> = m
        .ports
        .iter()
        .map(|p| (p.index, p.name.as_str(), p.kind.name()))
        .collect();
    assert_eq!(got, [(0, "i2c0", "i2c"), (1, "uart0", "uart")]);
    assert_eq!(m.port("uart0").map(|p| p.index), Some(1));
    assert!(m.port("spi0").is_none());
}

#[test]
fn a_port_name_is_used_once() {
    let e = mcu_err(&with_ports(&TWO.replace("\"uart0\"", "\"i2c0\"")));
    assert!(e.contains("port name 'i2c0' is used twice"), "{e}");
}

#[test]
fn a_port_index_is_used_once() {
    let e = mcu_err(&with_ports(&TWO.replace("index = 1", "index = 0")));
    assert!(e.contains("port index 0 is used twice"), "{e}");
}

#[test]
fn a_port_name_is_lowercase_letters_digits_and_dashes() {
    for bad in ["I2C0", "i2c_0", "", "i2c 0"] {
        let e = mcu_err(&with_ports(&TWO.replace("\"i2c0\"", &format!("\"{bad}\""))));
        assert!(e.contains("a-z, 0-9 and '-'"), "{bad}: {e}");
    }
    assert!(super::load::parse_mcu_text(
        &with_ports(&TWO.replace("\"i2c0\"", "\"pio-uart-2\"")),
        &[]
    )
    .is_ok());
}

#[test]
fn a_port_kind_is_a_link_kind() {
    let e = mcu_err(&with_ports(
        &TWO.replace("kind = \"uart\"", "kind = \"spi\""),
    ));
    assert!(e.contains("i2c") && e.contains("usb-cdc"), "{e}");
}

#[test]
fn every_builtin_mcu_names_its_ports() {
    for m in Registry::builtin().mcus() {
        assert!(!m.ports.is_empty(), "{} names no [[link_port]]", m.id);
    }
}
