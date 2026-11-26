//! `[[relay_link]]`: the links a node of an MCU relays over, read once
//! into `dagon_port::RelayLinkDef`.

use dagon_port::{Addressing, LinkKind, RelayLinkDef};

use super::load::parse_mcu_text;
use super::tests::{mcu_err, FAKE_MCU};
use super::Registry;

const PORTS: &str = r#"
[[link_port]]
index = 0
name = "i2c0"
kind = "i2c"

[[link_port]]
index = 1
name = "uart0"
kind = "uart"

[[link_port]]
index = 2
name = "can0"
kind = "can"
"#;

const TWO: &str = r#"
[[relay_link]]
id = 1
port = "i2c0"
addressing = "seven-bit"
max_payload = 256

[[relay_link]]
id = 254
port = "uart0"
addressing = { fixed = 255 }
max_payload = 1024
"#;

fn with(links: &str) -> String {
    format!("{FAKE_MCU}\n{PORTS}\n{links}")
}

fn err(links: &str) -> String {
    mcu_err(&with(links))
}

fn ok(links: &str) -> Vec<RelayLinkDef> {
    parse_mcu_text(&with(links), &[])
        .unwrap_or_else(|e| panic!("{e}"))
        .relay_link_defs()
}

#[test]
fn relay_links_load_into_the_one_runtime_type() {
    assert_eq!(
        ok(TWO),
        [
            RelayLinkDef {
                id: 1,
                port: 0,
                kind: LinkKind::I2c,
                addressing: Addressing::SevenBit,
                max_payload: 256,
            },
            RelayLinkDef {
                id: 254,
                port: 1,
                kind: LinkKind::Uart,
                addressing: Addressing::Fixed(255),
                max_payload: 1024,
            },
        ]
    );
    // An MCU that relays over nothing has an empty table.
    assert!(ok("").is_empty());
}

#[test]
fn an_id_is_1_to_254() {
    for bad in ["0", "255"] {
        let e = err(&TWO.replace("id = 1\n", &format!("id = {bad}\n")));
        assert!(
            e.contains(&format!("relay_link id {bad} is outside 1-254")),
            "{e}"
        );
    }
    assert_eq!(ok(&TWO.replace("id = 254", "id = 2"))[1].id, 2);
}

#[test]
fn an_id_is_used_once() {
    let e = err(&TWO.replace("id = 254", "id = 1"));
    assert!(e.contains("relay_link id 1 is used twice"), "{e}");
}

#[test]
fn a_port_has_one_entry() {
    let e = err(&TWO.replace(
        "port = \"uart0\"\naddressing = { fixed = 255 }\nmax_payload = 1024",
        "port = \"i2c0\"\naddressing = \"seven-bit\"\nmax_payload = 256",
    ));
    assert!(e.contains("port 'i2c0' has two relay_link entries"), "{e}");
}

#[test]
fn the_port_is_a_link_port_of_the_same_definition() {
    let e = err(&TWO.replace("port = \"i2c0\"", "port = \"i2c9\""));
    assert!(
        e.contains("relay_link port 'i2c9' is not a [[link_port]] of this MCU"),
        "{e}"
    );
}

#[test]
fn seven_bit_is_for_an_addressed_kind_and_fixed_for_one_without_addresses() {
    let e = err(&TWO.replace("addressing = { fixed = 255 }", "addressing = \"seven-bit\""));
    assert!(
        e.contains(
            "relay_link on 'uart0': seven-bit addressing needs a kind with addresses, not uart"
        ),
        "{e}"
    );
    let e = err(&TWO.replace("addressing = \"seven-bit\"", "addressing = { fixed = 7 }"));
    assert!(
        e.contains(
            "relay_link on 'i2c0': a fixed address is for a kind without addresses, not i2c"
        ),
        "{e}"
    );
}

#[test]
fn a_fixed_address_is_0_to_255() {
    assert_eq!(
        ok(&TWO.replace("fixed = 255", "fixed = 0"))[1].addressing,
        Addressing::Fixed(0)
    );
    let e = err(&TWO.replace("fixed = 255", "fixed = 256"));
    assert!(e.contains("fixed"), "{e}");
}

#[test]
fn max_payload_is_1_up_to_the_kinds_protocol_maximum() {
    for (from, to) in [
        ("max_payload = 256", "max_payload = 0"),
        ("max_payload = 256", "max_payload = 257"),
        ("max_payload = 1024", "max_payload = 1025"),
    ] {
        let e = err(&TWO.replace(from, to));
        assert!(
            e.contains("max_payload") && e.contains("is outside 1-"),
            "{to}: {e}"
        );
    }
    let small = ok(&TWO.replace("max_payload = 256", "max_payload = 1"));
    assert_eq!(small[0].max_payload, 1);
    // A kind DAGON carries no frames over has no maximum: no relay link on it.
    let e = err(&format!(
        "{TWO}\n[[relay_link]]\nid = 3\nport = \"can0\"\naddressing = {{ fixed = 1 }}\nmax_payload = 8\n"
    ));
    assert!(
        e.contains("relay_link on 'can0': DAGON relays no frames over can"),
        "{e}"
    );
}

#[test]
fn the_rp2040_relays_over_both_i2c_buses_and_both_pio_uarts() {
    let m = Registry::builtin().mcu("rp2040").unwrap();
    let got: Vec<(u8, &str)> = m
        .relay_link_defs()
        .iter()
        .map(|l| {
            let name = m.ports.iter().find(|p| p.index == l.port).unwrap();
            (l.id, name.name.as_str())
        })
        .collect();
    assert_eq!(
        got,
        [(1, "i2c0"), (2, "pio-uart"), (3, "i2c1"), (4, "pio-uart1")]
    );
    // MCUs of nodes that relay over nothing have none.
    for id in ["stm32f401re", "esp32s3"] {
        assert!(
            Registry::builtin()
                .mcu(id)
                .unwrap()
                .relay_link_defs()
                .is_empty(),
            "{id}"
        );
    }
}
