use std::collections::BTreeSet;

use dagon_topology::{boards, check_str, Severity};

const DEMO: &str = include_str!("../../../topology/desk-demo.yaml");

fn mutate(from: &str, to: &str) -> String {
    assert!(DEMO.contains(from), "desk-demo has no {from:?}");
    DEMO.replacen(from, to, 1)
}

fn error_rules(yaml: &str) -> BTreeSet<&'static str> {
    check_str(yaml).errors().map(|i| i.rule).collect()
}

fn assert_fires(yaml: &str, rule: &str) {
    let rules = error_rules(yaml);
    assert!(rules.contains(rule), "expected {rule}, got {rules:?}");
}

#[test]
fn desk_demo_is_clean() {
    let report = check_str(DEMO);
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert!(report.is_pushable());
}

#[test]
fn unparseable_document_is_v01() {
    assert_fires(&mutate("role: coordinator", "role: boss"), "V-01");
    assert_fires("not: [valid", "V-01");
}

#[test]
fn v01_schema_version() {
    assert_fires(&mutate("dagon: 1", "dagon: 2"), "V-01");
}

#[test]
fn v01_missing_i2c_address() {
    assert_fires(&mutate("    addr: 0x42\n", ""), "V-01");
}

#[test]
fn v02_node_ids() {
    assert_fires(&mutate("id: watch", "id: this-id-is-far-too-long"), "V-02");
}

#[test]
fn v03_one_coordinator() {
    assert_fires(&mutate("role: coordinator", "role: node"), "V-03");
}

#[test]
fn v04_single_hop() {
    assert_fires(
        &mutate("from: hub\n    to: watch", "from: seg\n    to: watch"),
        "V-04",
    );
}

#[test]
fn v05_one_link_per_node() {
    assert_fires(&mutate("to: watch", "to: seg"), "V-05");
}

#[test]
fn v06_port_matches_mcu() {
    assert_fires(&mutate("port: vendor-stage2", "port: self-boot"), "V-06");
}

#[test]
fn v07_pin_on_header() {
    assert_fires(&mutate("nrst: 18", "nrst: 5"), "V-07");
}

#[test]
fn v08_pin_used_twice() {
    assert_fires(&mutate("boot: 28", "boot: 18"), "V-08");
}

#[test]
fn v09_hw_uart0_pins() {
    assert_fires(
        &mutate(
            "iface: pio-uart\n    pins: { tx: 26, rx: 27 }",
            "iface: hw-uart0\n    pins: { tx: 16, rx: 17 }",
        ),
        "V-09",
    );
}

#[test]
fn v10_pio_state_machines() {
    let mut nodes = String::new();
    let mut links = String::new();
    for i in 1..=4 {
        nodes.push_str(&format!(
            "  - id: w{i}\n    mcu: esp32s3\n    role: node\n    port: vendor-stage2\n    tag: w{i}\n"
        ));
        links.push_str(&format!(
            "  - from: hub\n    to: w{i}\n    kind: uart\n    iface: pio-uart\n    pins: {{ tx: 26, rx: 27 }}\n    control: {{ kind: cooperative, rom_entry: force-download-boot }}\n"
        ));
    }
    let yaml = DEMO
        .replacen("\nbuses:", &format!("{nodes}\nbuses:"), 1)
        .replacen("\nrecovery:", &format!("{links}\nrecovery:"), 1);
    assert_fires(&yaml, "V-10");
}

#[test]
fn v11_cooperative_needs_rom_entry() {
    assert_fires(
        &mutate("      rom_entry: force-download-boot\n", ""),
        "V-11",
    );
}

#[test]
fn v12_tier_fits_control() {
    assert_fires(
        &mutate(
            "watch: [local, redistribute, rom-coop]",
            "watch: [local, redistribute, rom-forced]",
        ),
        "V-12",
    );
}

#[test]
fn v13_rom_pins() {
    assert_fires(
        &mutate(
            "node_pins: { sda: PB4, scl: PA8 }",
            "node_pins: { sda: PB9, scl: PB8 }",
        ),
        "V-13",
    );
}

#[test]
fn v14_update_order() {
    assert_fires(
        &mutate("order: [seg, watch, hub]", "order: [hub, seg, watch]"),
        "V-14",
    );
}

#[test]
fn v15_constraints_reference_nodes() {
    assert_fires(
        &mutate("lockstep: [hub, seg, watch]", "lockstep: [hub, seg, cam]"),
        "V-15",
    );
}

#[test]
fn v16_flash_map() {
    assert_fires(
        &mutate("flash_map: f401-default", "flash_map: esp32s3-16m"),
        "V-16",
    );
}

#[test]
fn v17_i2c_controller_pins() {
    assert_fires(&mutate("controller: i2c0", "controller: i2c1"), "V-17");
}

#[test]
fn v18_i2c_address() {
    assert_fires(&mutate("addr: 0x42", "addr: 0x39"), "V-18");
    assert_fires(&mutate("addr: 0x42", "addr: 0x78"), "V-18");
}

#[test]
fn v20_bus_speed_and_pullups() {
    assert_fires(&mutate("speed_hz: 400000", "speed_hz: 1000000"), "V-20");
    assert_fires(&mutate("pullups: external", "pullups: none"), "V-20");
}

#[test]
fn v21_i2c_target() {
    assert_fires(
        &mutate(
            "kind: uart\n    iface: pio-uart\n    pins: { tx: 26, rx: 27 }\n    baud: 115200",
            "kind: i2c\n    bus: i2c-main\n    addr: 0x43\n    node_pins: { sda: GPIO44, scl: GPIO43 }",
        ),
        "V-21",
    );
}

#[test]
fn v22_missing_reset_pin_is_only_a_warning() {
    for pin in ["      nrst: 18\n", "      boot: 28\n"] {
        let report = check_str(&mutate(pin, ""));
        assert!(report.is_pushable(), "{:?}", report.issues);
        let warnings: Vec<_> = report
            .warnings()
            .map(|i| (i.rule, i.path.as_str()))
            .collect();
        assert_eq!(warnings, [("V-22", "links[0].control")]);
        assert!(report
            .issues
            .iter()
            .all(|i| i.severity == Severity::Warning));
    }
}

#[test]
fn json_report_shape() {
    assert_eq!(
        check_str(DEMO).to_json(),
        r#"{"ok":true,"errors":0,"warnings":0,"issues":[]}"#
    );
    let json = check_str(&mutate("      nrst: 18\n", "")).to_json();
    assert!(json.starts_with(r#"{"ok":true,"errors":0,"warnings":1,"issues":[{"rule":"V-22","severity":"warning","path":"links[0].control","#));
}

#[test]
fn board_table_starts_with_demo_boards() {
    let ids: Vec<_> = boards::BOARDS.iter().map(|b| b.id).collect();
    assert_eq!(
        ids,
        [
            "waveshare-rp2040-touch-lcd-1.28",
            "nucleo-f401re",
            "m5stopwatch"
        ]
    );
    let f401 = boards::board("nucleo-f401re").unwrap();
    for pin in ["NRST", "BOOT0"] {
        let h = f401.header.iter().find(|h| h.pin == pin).unwrap();
        assert_eq!(h.position, None, "{pin} position is not confirmed yet");
    }
}
