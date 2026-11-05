use std::collections::BTreeSet;

use dagon_topology::{check_str, defs::Registry, Severity};

const DEMO: &str = include_str!("../../../topology/sample.yaml");

fn mutate(from: &str, to: &str) -> String {
    assert!(DEMO.contains(from), "sample has no {from:?}");
    DEMO.replacen(from, to, 1)
}

fn error_rules(yaml: &str) -> BTreeSet<&'static str> {
    check_str(yaml).errors().map(|i| i.rule).collect()
}

fn info_rules(yaml: &str) -> BTreeSet<&'static str> {
    check_str(yaml)
        .issues
        .into_iter()
        .filter(|i| i.severity == Severity::Info)
        .map(|i| i.rule)
        .collect()
}

fn assert_fires(yaml: &str, rule: &str) {
    let rules = error_rules(yaml);
    assert!(rules.contains(rule), "expected {rule}, got {rules:?}");
}

#[test]
fn sample_is_clean() {
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
fn v23_more_than_eight_nodes_is_described_but_not_updatable() {
    let extra: String = (0..7)
        .map(|i| format!("  - id: extra{i}\n    mcu: esp32s3\n    role: node\n    port: vendor-stage2\n    tag: extra{i}\n"))
        .collect();
    let yaml = DEMO.replacen("nodes:\n", &format!("nodes:\n{extra}"), 1);
    let report = check_str(&yaml);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.rule == "V-23" && i.path == "nodes" && i.severity == Severity::Info),
        "{:?}",
        report.issues
    );
    assert!(!report.errors().any(|i| i.rule == "V-23"));
    assert!(
        !report.errors().any(|i| i.rule == "V-02"),
        "{:?}",
        report.issues
    );
}

#[test]
fn v04_links_name_known_nodes() {
    assert_fires(
        &mutate("from: hub\n    to: watch", "from: ghost\n    to: watch"),
        "V-04",
    );
}

#[test]
fn v23_grandchild_is_described_but_not_updatable() {
    // seg → watch: a grandchild. A valid 構成 shape, not one updating reaches yet.
    let yaml = mutate("from: hub\n    to: watch", "from: seg\n    to: watch");
    assert!(info_rules(&yaml).contains("V-23"));
    let rules = error_rules(&yaml);
    assert!(rules.is_empty(), "{rules:?}");
    // Valid: it saves and builds; updating carries the hub and seg only.
    assert!(check_str(&yaml).is_pushable());
    let (sub, skipped) = dagon_topology::updatable(&dagon_topology::parse_str(&yaml).unwrap());
    assert_eq!(
        skipped,
        vec![("watch".to_string(), dagon_topology::Skip::Deep)]
    );
    assert_eq!(sub.update.order, ["seg", "hub"]);
    assert!(dagon_topology::compile_str(&yaml).is_ok());
}

#[test]
fn v23_several_coordinators_over_can() {
    let yaml = DEMO
        .replacen(
            "\nbuses:",
            "  - id: hub2\n    mcu: rp2040\n    role: coordinator\n    port: self-boot\n    tag: hub2\n\nbuses:",
            1,
        )
        .replacen("links:\n", "links:\n  - from: hub\n    to: hub2\n    kind: can\n", 1)
        .replacen("order: [seg, watch, hub]", "order: [seg, watch, hub2, hub]", 1);
    let report = check_str(&yaml);
    let v23: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.rule == "V-23" && i.severity == Severity::Info)
        .map(|i| i.path.as_str())
        .collect();
    assert!(v23.contains(&"nodes"), "{v23:?}");
    assert!(v23.contains(&"links[0].kind"), "{v23:?}");
    assert!(v23.contains(&"links[0].to"), "{v23:?}");
    // The shape itself parses: no V-01, and the coordinator count is not V-03.
    let rules = error_rules(&yaml);
    assert!(rules.is_empty(), "{rules:?}");
    // The second 親機 is skipped: the compiled topology is the sample's.
    let (_, skipped) = dagon_topology::updatable(&dagon_topology::parse_str(&yaml).unwrap());
    assert_eq!(
        skipped,
        vec![("hub2".to_string(), dagon_topology::Skip::SecondHub)]
    );
    assert_eq!(
        dagon_topology::compile_str(&yaml).unwrap(),
        dagon_topology::compile_str(DEMO).unwrap()
    );
}

#[test]
fn p1_coordinator_chosen_by_mcu_is_checked_by_its_gpio_range() {
    // Without a named board the hub is checked against RP2040 GPIO0-29, not a board header.
    let by_mcu = mutate("    board: waveshare-rp2040-touch-lcd-1.28\n", "");
    let r = check_str(&by_mcu);
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert!(!error_rules(&by_mcu.replacen("nrst: 18", "nrst: 20", 1)).contains("V-07"));
    assert_fires(&by_mcu.replacen("nrst: 18", "nrst: 30", 1), "V-07");
    assert_fires(&DEMO.replacen("nrst: 18", "nrst: 20", 1), "V-07");
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
    let yaml = format!("{DEMO}\nconstraints:\n  - lockstep: [hub, seg, cam]\n");
    assert_fires(&yaml, "V-15");
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

fn messages(yaml: &str, rule: &str) -> Vec<String> {
    check_str(yaml)
        .errors()
        .filter(|i| i.rule == rule)
        .map(|i| i.message.clone())
        .collect()
}

#[test]
fn v18_v20_name_the_rom_loader_from_its_definition() {
    assert_eq!(
        messages(&mutate("addr: 0x42", "addr: 0x39"), "V-18"),
        ["0x39 is reserved for the STM32 ROM bootloader on this bus"]
    );
    assert_eq!(
        messages(&mutate("speed_hz: 400000", "speed_hz: 1000000"), "V-20"),
        ["speed_hz must be one of [100000, 400000] (STM32 ROM bootloader limit 400000)"]
    );
    // No node is reached through its ROM: no loader limit to name.
    let no_rom = mutate(
        "seg: [local, redistribute, rom-forced]",
        "seg: [local, redistribute]",
    );
    assert_eq!(
        messages(
            &no_rom.replacen("speed_hz: 400000", "speed_hz: 1000000", 1),
            "V-20"
        ),
        ["speed_hz must be one of [100000, 400000]"]
    );
    assert!(messages(&no_rom.replacen("addr: 0x42", "addr: 0x39", 1), "V-18").is_empty());
}

#[test]
fn v25_coordinator_mcu_needs_a_coordinator_section() {
    let esp_hub = mutate(
        "    mcu: rp2040\n    board: waveshare-rp2040-touch-lcd-1.28\n    role: coordinator\n    port: self-boot\n    flash_map: rp2040-4m",
        "    mcu: esp32s3\n    role: coordinator\n    port: vendor-stage2",
    );
    let r = check_str(&esp_hub);
    let v25: Vec<_> = r.errors().filter(|i| i.rule == "V-25").collect();
    assert_eq!(v25.len(), 1, "{:?}", r.issues);
    assert_eq!(v25[0].path, "nodes[0].mcu");
    assert_eq!(
        v25[0].message,
        "esp32s3 cannot be the coordinator: its MCU definition has no [coordinator] section"
    );
    // A second coordinator (here the F401) is V-23's shape, not V-25's.
    let two_hubs = DEMO.replacen("role: node", "role: coordinator", 1);
    assert!(error_rules(&two_hubs).is_empty());
    assert_eq!(info_rules(&two_hubs), BTreeSet::from(["V-23"]));
    // An unknown MCU is V-06's alone.
    let unknown = mutate("    mcu: rp2040\n", "    mcu: nope\n");
    assert!(!error_rules(&unknown).contains("V-25"));
    assert!(error_rules(&unknown).contains("V-06"));
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
fn builtin_boards_are_the_board_packages() {
    let reg = Registry::builtin();
    let mut ids: Vec<String> = reg.boards().iter().map(|b| b.id.clone()).collect();
    ids.sort();
    // Each package folder's definition id (the set; order carries no meaning).
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/boards");
    let mut want: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path().join("dagon-board.toml"))
        .filter(|p| p.is_file())
        .map(|p| {
            let t: toml::Table = std::fs::read_to_string(p).unwrap().parse().unwrap();
            t["id"].as_str().unwrap().to_string()
        })
        .collect();
    want.sort();
    assert_eq!(ids, want);
    let f401 = reg.board("nucleo-f401re").unwrap();
    for pin in ["NRST", "BOOT0"] {
        let h = f401.header.iter().find(|h| h.pin == pin).unwrap();
        assert_eq!(h.label, None, "{pin} position is not confirmed yet");
    }
}

/// sample with the hub's uplink set to `kind` (host `host`).
fn with_uplink(kind: &str, host: &str) -> String {
    mutate(
        "    tag: hub-rp2040-lcd
",
        &format!(
            "    tag: hub-rp2040-lcd
    uplink: {{ kind: {kind}, host: {host} }}
"
        ),
    )
}

fn warnings(yaml: &str) -> Vec<(String, String, String)> {
    check_str(yaml)
        .issues
        .into_iter()
        .filter(|i| i.severity == Severity::Warning)
        .map(|i| (i.rule.to_string(), i.path, i.message))
        .collect()
}

#[test]
fn every_link_kind_and_uplink_round_trips_through_the_schema() {
    for k in dagon_topology::schema::LinkKind::ALL {
        let t = dagon_topology::parse_str(&with_uplink(k.name(), "browser")).unwrap();
        let hub = t.node("hub").unwrap();
        let up = dagon_topology::Topology::uplink(hub);
        assert_eq!(
            (up.kind, up.host),
            (*k, dagon_topology::schema::UplinkHost::Browser)
        );
    }
    let t = dagon_topology::parse_str(DEMO).unwrap();
    let up = dagon_topology::Topology::uplink(t.node("hub").unwrap());
    assert_eq!(
        up.kind,
        dagon_topology::schema::LinkKind::UsbCdc,
        "omitted: usb-cdc"
    );
    assert!(check_str(&with_uplink("usb-cdc", "pc-cli"))
        .issues
        .is_empty());
}

#[test]
fn the_uplink_does_not_change_the_compiled_topology() {
    let golden = dagon_topology::compile_str(DEMO).unwrap();
    for k in ["usb-cdc", "uart", "wifi-tcp", "ble"] {
        assert_eq!(
            dagon_topology::compile_str(&with_uplink(k, "embedded")).unwrap(),
            golden,
            "{k}"
        );
    }
}

#[test]
fn v24_names_a_hop_its_firmware_cannot_do_yet_without_blocking() {
    let yaml = with_uplink("wifi-tcp", "browser");
    let w = warnings(&yaml);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].0, "V-24");
    assert!(
        w[0].1.ends_with("uplink.kind") && w[0].2.contains("hub") && w[0].2.contains("wifi-tcp"),
        "{w:?}"
    );
    assert!(check_str(&yaml).is_pushable(), "a warning only");
    // A link the 親機 or the 子機 cannot do: V-24 names both ends, V-23 says
    // it cannot be updated.
    // The link's kind (the first `kind: i2c` is the bus's).
    let ble = mutate(
        "    kind: i2c
    bus: ",
        "    kind: ble
    bus: ",
    );
    let w = warnings(&ble);
    assert!(
        w.iter()
            .any(|x| x.0 == "V-24" && x.2.contains("hub -> seg") && x.2.contains("ble")),
        "{w:?}"
    );
    assert!(info_rules(&ble).contains("V-23"));
}

#[test]
fn v24_an_uplink_belongs_to_a_coordinator() {
    let yaml = mutate(
        "    tag: seg-f401-7seg
",
        "    tag: seg-f401-7seg
    uplink: { kind: usb-cdc }
",
    );
    assert_fires(&yaml, "V-24");
}

#[test]
fn v26_uart_baud_must_be_within_two_percent_on_both_ends() {
    // RP2040's link UART: 125 MHz / 8 / a whole divider; ESP32-S3's:
    // 40 MHz / (n + k/16).
    for ok in ["115200", "230400"] {
        let yaml = mutate("baud: 115200", &format!("baud: {ok}"));
        assert!(check_str(&yaml).issues.is_empty(), "{ok}");
    }
    assert_eq!(
        messages(&mutate("baud: 115200", "baud: 460800"), "V-26"),
        ["baud 460800: hub (RP2040) runs at 473485 bit/s, +2.75 %; each end of a UART link must be within 2 %"]
    );
    for bad in ["921600", "1000000"] {
        let r = check_str(&mutate("baud: 115200", &format!("baud: {bad}")));
        let v26: Vec<_> = r.errors().filter(|i| i.rule == "V-26").collect();
        assert_eq!(v26.len(), 1, "{bad}: {:?}", r.issues);
        assert_eq!(v26[0].path, "links[1].baud");
        assert!(
            v26[0].message.contains("hub (RP2040)"),
            "{}",
            v26[0].message
        );
    }
    // Neither end can divide its clock down to these.
    for none in ["0", "100000000"] {
        assert_eq!(
            messages(&mutate("baud: 115200", &format!("baud: {none}")), "V-26"),
            [
                format!("baud {none}: hub (RP2040) cannot make it"),
                format!("baud {none}: watch (ESP32-S3) cannot make it")
            ]
        );
    }
}

#[test]
fn v26_refuses_a_uart_end_whose_mcu_states_no_link_uart() {
    // An ESP32-S3 definition without [link_uart]: its firmware runs the
    // UART link, but no speed on it is known to work, so none is accepted,
    // the default included.
    let block = "[link_uart]\\nclock_hz = 40000000\\nclocks_per_bit = 1\\nfrac_bits = 4\\n";
    let json = Registry::builtin().to_json();
    assert!(json.contains(block), "the esp32s3 [link_uart] block moved");
    let reg = Registry::from_json(&json.replacen(block, "", 1)).unwrap();
    assert!(reg.mcu("esp32s3").unwrap().link_uart.is_none());
    for baud in ["115200", "230400"] {
        let yaml = mutate("baud: 115200", &format!("baud: {baud}"));
        let got: Vec<_> = dagon_topology::check_str_with(&reg, &yaml)
            .errors()
            .filter(|i| i.rule == "V-26")
            .map(|i| i.message.clone())
            .collect();
        assert_eq!(
            got,
            [format!(
                "baud {baud}: watch (ESP32-S3) cannot be checked: its MCU definition has no [link_uart]"
            )]
        );
    }
    // An end whose firmware runs no UART link (STM32F401RE: I2C up only)
    // runs no speed: V-24 reports the link, not V-26.
    let yaml = mutate(
        "  - id: watch\n    mcu: esp32s3\n",
        "  - id: watch\n    mcu: stm32f401re\n",
    );
    assert!(messages(&yaml, "V-26").is_empty());
}

#[test]
fn both_ends_of_a_uart_link_take_its_baud_from_the_topology() {
    let t = dagon_topology::parse_str(&mutate("baud: 115200", "baud: 230400")).unwrap();
    assert_eq!(t.uart_baud("hub"), Ok(Some(230_400)));
    assert_eq!(t.uart_baud("watch"), Ok(Some(230_400)));
    assert_eq!(t.uart_baud("seg"), Ok(None));
    // Left out: the default, the same on both ends.
    let t = dagon_topology::parse_str(&mutate("    baud: 115200\n", "")).unwrap();
    assert_eq!(
        t.uart_baud("hub"),
        Ok(Some(dagon_topology::schema::DEFAULT_BAUD))
    );
    assert_eq!(t.uart_baud("watch"), t.uart_baud("hub"));
}
