//! A link has two ends and no direction: each end names its node's port and,
//! on an addressed kind, the address the other end reaches it at. Every rule,
//! reachability and the compiled bytes are the same whichever end is listed
//! first.

use std::collections::BTreeSet;

use dagon_topology::defs::Registry;
use dagon_topology::{check_str_with, compile_str, parse_str, updatable, Report, Severity};

fn check_str(yaml: &str) -> Report {
    check_str_with(Registry::builtin(), yaml)
}

const SAMPLE: &str = include_str!("../../../topology/sample.yaml");

const SEG_END: &str = "{ node: seg, port: i2c3, addr: 0x42, node_pins: { sda: PB4, scl: PA8 } }";
const WATCH_END: &str = "{ node: watch, port: uart0 }";

fn mutate(yaml: &str, from: &str, to: &str) -> String {
    assert!(yaml.contains(from), "no {from:?} in\n{yaml}");
    yaml.replacen(from, to, 1)
}

/// The same 構成 with the two ends of every link listed the other way round.
fn swapped(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        out.push(lines[i]);
        if lines[i].trim() == "ends:" {
            out.push(lines[i + 2]);
            out.push(lines[i + 1]);
            i += 3;
        } else {
            i += 1;
        }
    }
    out.join("\n") + "\n"
}

/// (rule, severity, message) of every issue; paths name end positions, so they are left out.
fn issues(yaml: &str) -> BTreeSet<(String, String, String)> {
    check_str(yaml)
        .issues
        .into_iter()
        .map(|i| (i.rule.to_string(), format!("{:?}", i.severity), i.message))
        .collect()
}

fn messages(yaml: &str, rule: &str) -> Vec<String> {
    check_str(yaml)
        .issues
        .into_iter()
        .filter(|i| i.rule == rule && i.severity == Severity::Error)
        .map(|i| i.message)
        .collect()
}

/// Asserts `rule` reports an error whose message has `needle`, in both orientations.
fn assert_error(yaml: &str, rule: &str, needle: &str) {
    for y in [yaml.to_string(), swapped(yaml)] {
        let got = messages(&y, rule);
        assert!(
            got.iter().any(|m| m.contains(needle)),
            "expected {rule} with {needle:?}, got {got:?}\n{:?}",
            check_str(&y).issues
        );
    }
}

/// A second NUCLEO-F401RE on the sample's I2C bus at `addr`.
fn with_second_seg(addr: &str) -> String {
    let yaml = mutate(
        SAMPLE,
        "  - id: watch\n",
        "  - id: seg2\n    mcu: stm32f401re\n    board: nucleo-f401re\n    role: node\n    port: self-boot\n    flash_map: f401-default\n    tag: seg2\n  - id: watch\n",
    );
    let yaml = mutate(
        &yaml,
        "recovery:",
        &format!(
            "  - kind: i2c\n    bus: i2c-main\n    ends:\n      - {{ node: hub, port: i2c0 }}\n      - {{ node: seg2, port: i2c3, addr: {addr}, node_pins: {{ sda: PB4, scl: PA8 }} }}\n    control:\n      kind: cooperative\n      rom_entry: stm32-i2c-rom\n\nrecovery:"
        ),
    );
    mutate(&yaml, "order: [seg,", "order: [seg, seg2,")
}

#[test]
fn the_sample_is_clean_in_both_orientations_and_compiles_to_the_same_bytes() {
    assert!(
        check_str(SAMPLE).issues.is_empty(),
        "{:?}",
        check_str(SAMPLE).issues
    );
    assert!(check_str(&swapped(SAMPLE)).issues.is_empty());
    assert_eq!(
        compile_str(SAMPLE).unwrap(),
        compile_str(&swapped(SAMPLE)).unwrap()
    );
}

#[test]
fn an_end_names_a_port_its_nodes_mcu_defines() {
    assert_error(
        &mutate(SAMPLE, SEG_END, &SEG_END.replace("i2c3", "i2c9")),
        "V-27",
        "i2c9",
    );
}

#[test]
fn an_ends_port_is_of_the_links_kind() {
    assert_error(
        &mutate(SAMPLE, SEG_END, &SEG_END.replace("i2c3", "usart2")),
        "V-27",
        "usart2",
    );
    assert_error(
        &mutate(SAMPLE, WATCH_END, "{ node: watch, port: i2c0 }"),
        "V-27",
        "i2c0",
    );
}

#[test]
fn an_address_is_only_on_an_addressed_kind() {
    assert_error(
        &mutate(
            SAMPLE,
            WATCH_END,
            "{ node: watch, port: uart0, addr: 0x30 }",
        ),
        "V-18",
        "uart",
    );
}

#[test]
fn an_address_is_seven_bit_from_0x08_to_0x77() {
    for bad in ["0x07", "0x78", "0x00"] {
        let yaml = mutate(SAMPLE, "addr: 0x42", &format!("addr: {bad}"));
        assert_error(&yaml, "V-18", "outside 0x08-0x77");
        // Reported on the end that gives the address.
        let paths: Vec<_> = check_str(&yaml)
            .issues
            .into_iter()
            .filter(|i| i.rule == "V-18")
            .map(|i| i.path)
            .collect();
        assert_eq!(paths, ["links[0].ends[1].addr"]);
    }
    for good in ["0x08", "0x77"] {
        let yaml = mutate(SAMPLE, "addr: 0x42", &format!("addr: {good}"));
        assert!(
            messages(&yaml, "V-18").is_empty(),
            "{good}: {:?}",
            messages(&yaml, "V-18")
        );
    }
}

#[test]
fn no_two_ends_on_one_bus_share_an_address() {
    assert!(messages(&with_second_seg("0x43"), "V-18").is_empty());
    assert_error(
        &with_second_seg("0x42"),
        "V-18",
        "already used on bus 'i2c-main'",
    );
}

#[test]
fn the_rom_loaders_address_stays_reserved_on_its_bus() {
    // seg is reached through its ROM loader (rom-forced), which answers at 0x39.
    assert_error(
        &with_second_seg("0x39"),
        "V-18",
        "reserved for the STM32 ROM bootloader",
    );
}

#[test]
fn a_child_needs_a_link_and_two_links_never_join_the_same_two_nodes() {
    let lone = mutate(
        SAMPLE,
        "    ends:\n      - { node: hub, port: pio-uart",
        "    ends:\n      - { node: seg, port: pio-uart",
    );
    // watch is still an end; seg now has two links, to hub and to watch: no V-05.
    assert!(
        messages(&lone, "V-05").is_empty(),
        "{:?}",
        messages(&lone, "V-05")
    );
    let twice = mutate(
        SAMPLE,
        "recovery:",
        "  - kind: can\n    ends:\n      - { node: seg, port: i2c1 }\n      - { node: hub, port: i2c1 }\n\nrecovery:",
    );
    assert_error(&twice, "V-05", "'hub' and 'seg'");
}

#[test]
fn a_node_below_a_child_is_skipped_whichever_end_is_listed_first() {
    let deep = mutate(
        SAMPLE,
        "      - { node: hub, port: pio-uart, pins: { tx: 26, rx: 27 } }",
        "      - { node: seg, port: usart1, pins: { tx: 9, rx: 10 } }",
    );
    for y in [deep.clone(), swapped(&deep)] {
        let infos: Vec<_> = check_str(&y)
            .issues
            .into_iter()
            .filter(|i| i.rule == "V-23")
            .map(|i| i.message)
            .collect();
        assert!(
            infos
                .iter()
                .any(|m| m.contains("'seg'") && m.contains("'watch'")),
            "{infos:?}"
        );
        let (sub, skipped) = updatable(&parse_str(&y).unwrap());
        assert_eq!(skipped, [("watch".to_string(), dagon_topology::Skip::Deep)]);
        assert_eq!(sub.links.len(), 1);
    }
}

#[test]
fn the_coordinators_own_pins_are_checked_whichever_end_it_is() {
    // GPIO 40 is no RP2040 header pin: V-07 on the uplink node's end, listed first or second.
    assert_error(
        &mutate(
            SAMPLE,
            "pins: { tx: 26, rx: 27 }",
            "pins: { tx: 40, rx: 27 }",
        ),
        "V-07",
        "GPIO40",
    );
    assert_error(&mutate(SAMPLE, "nrst: 18", "nrst: 40"), "V-07", "GPIO40");
}

#[test]
fn every_rule_says_the_same_in_both_orientations() {
    let cases = [
        SAMPLE.to_string(),
        mutate(SAMPLE, SEG_END, &SEG_END.replace("i2c3", "i2c9")),
        mutate(SAMPLE, "addr: 0x42", "addr: 0x78"),
        mutate(SAMPLE, "nrst: 18", "nrst: 40"),
        mutate(SAMPLE, "baud: 115200", "baud: 115212"),
        with_second_seg("0x42"),
        mutate(
            SAMPLE,
            "      - { node: hub, port: pio-uart, pins: { tx: 26, rx: 27 } }",
            "      - { node: seg, port: usart1, pins: { tx: 9, rx: 10 } }",
        ),
    ];
    for y in cases {
        assert_eq!(issues(&y), issues(&swapped(&y)), "{y}");
        let (a, b) = (
            updatable(&parse_str(&y).unwrap()).1,
            updatable(&parse_str(&swapped(&y)).unwrap()).1,
        );
        assert_eq!(a, b);
    }
}

#[test]
fn a_new_end_starts_at_the_port_its_definitions_name() {
    use dagon_topology::defs::Registry;
    use dagon_topology::schema::{LinkKind, UartIface};
    let reg = Registry::builtin();
    let port = |mcu, board, kind, iface| dagon_topology::default_port(reg, mcu, board, kind, iface);
    let hub = Some("waveshare-rp2040-touch-lcd-1.28");
    assert_eq!(port("rp2040", hub, LinkKind::I2c, None), Some("i2c0"));
    assert_eq!(
        port("rp2040", hub, LinkKind::Uart, Some(UartIface::PioUart)),
        Some("pio-uart")
    );
    assert_eq!(
        port("rp2040", hub, LinkKind::Uart, Some(UartIface::HwUart0)),
        Some("uart0")
    );
    assert_eq!(
        port("stm32f401re", Some("nucleo-f401re"), LinkKind::I2c, None),
        Some("i2c3")
    );
    assert_eq!(
        port(
            "esp32s3",
            Some("m5stopwatch"),
            LinkKind::Uart,
            Some(UartIface::PioUart)
        ),
        Some("uart0")
    );
    assert_eq!(port("rp2040", None, LinkKind::Can, None), None);
}

#[test]
fn uart_pins_are_only_on_a_uart_link_end() {
    // On either end of an I2C link, listed first or second: refused, never dropped.
    for end in ["{ node: hub, port: i2c0,", "{ node: seg, port: i2c3,"] {
        let yaml = mutate(SAMPLE, end, &format!("{end} pins: {{ tx: 0, rx: 1 }},"));
        assert_error(
            &yaml,
            "V-01",
            "'pins' are UART pins; this i2c link has none",
        );
    }
    // On a kind the compiled topology does not carry either.
    let usb = mutate(
        SAMPLE,
        "recovery:",
        "  - kind: usb-cdc\n    ends:\n      - { node: seg, port: usb-otg-fs, pins: { tx: 0, rx: 1 } }\n      - { node: watch, port: usb-serial-jtag }\n\nrecovery:",
    );
    assert_error(&usb, "V-01", "this usb-cdc link has none");
    // The boundary: on a UART link's end, on either end, they are the pins it runs on.
    let watch = mutate(
        SAMPLE,
        "{ node: hub, port: pio-uart, pins: { tx: 26, rx: 27 } }",
        "{ node: hub, port: pio-uart }",
    );
    let watch = mutate(
        &watch,
        WATCH_END,
        "{ node: watch, port: uart0, pins: { tx: 43, rx: 44 } }",
    );
    for y in [
        SAMPLE.to_string(),
        swapped(SAMPLE),
        watch.clone(),
        swapped(&watch),
    ] {
        assert!(
            messages(&y, "V-01").is_empty(),
            "{:?}",
            messages(&y, "V-01")
        );
    }
}

#[test]
fn a_second_coordinator_without_a_link_needs_none() {
    let yaml = mutate(
        SAMPLE,
        "  - id: watch\n",
        "  - id: hub2\n    mcu: rp2040\n    role: coordinator\n    port: self-boot\n    tag: hub2\n  - id: watch\n",
    );
    let yaml = mutate(
        &yaml,
        "order: [seg, watch, hub]",
        "order: [seg, watch, hub2, hub]",
    );
    for y in [yaml.clone(), swapped(&yaml)] {
        assert!(
            messages(&y, "V-05").is_empty(),
            "{:?}",
            messages(&y, "V-05")
        );
        assert!(check_str(&y).is_pushable(), "{:?}", check_str(&y).issues);
    }
    // A node whose role is node still needs one.
    let lone = mutate(
        &yaml,
        "role: coordinator\n    port: self-boot\n    tag: hub2",
        "role: node\n    port: self-boot\n    tag: hub2",
    );
    assert_error(&lone, "V-05", "node 'hub2' needs a link, has none");
}
