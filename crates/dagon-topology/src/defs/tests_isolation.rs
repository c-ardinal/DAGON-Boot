//! The isolation schema: kind records, register blocks, required classes,
//! watchdog, boot record placement, and the board's boot and capability
//! tables.

use super::isolation::{Kind, Level, Why, RECORD_MAX};
use super::load::parse_mcu_text;
use super::tests::{mcu_err, FAKE_BOARD, FAKE_MCU};
use super::{requirements, BoardDef, Registry};

/// Blocks of every class a protected definition needs, a bus master and
/// two window-eligible blocks.
const BLOCKS: &str = r#"
[[core]]
id = "cpu0"

[[peripheral]]
id = "flash-if"
base = 0x4000_0000
size = 0x400
privileged_why = "flash"

[[peripheral]]
id = "wdt"
base = 0x4000_0400
size = 0x400
privileged_why = "watchdog"

[[peripheral]]
id = "rcc"
base = 0x4000_0800
size = 0x400
privileged_why = ["clock", "reset"]

[[peripheral]]
id = "nvic"
base = 0x4000_0C00
size = 0x400
privileged_why = "irq-routing"

[[peripheral]]
id = "dma"
base = 0x4000_1000
size = 0x400
bus_master = true
privileged_why = "dma"

[[peripheral]]
id = "can1"
base = 0x4000_6400
size = 0x400
window_eligible = true

[[peripheral]]
id = "uart3"
base = 0x4000_4800
size = 0x400
window_eligible = true
"#;

/// The fake MCU with an armv7m-mpu record, its blocks, a watchdog and a
/// boot record (every numeric isolation field appears).
pub(super) fn isolated_mcu() -> String {
    let top = FAKE_MCU.replace(
        "gpio_count = 16\n",
        "gpio_count = 16\nram = { at = 0x2000_0000, size = 0x1_0000 }\n",
    );
    format!(
        r#"{top}
[watchdog]
kind = "iwdg"
trial_ms = 4_000
confirmed_ms = 8_000

[isolation]
level = "mpu"
kind = "armv7m-mpu"
boot_extensions = true
ext_ram_max = 16_384

[isolation.capabilities]
mpu_regions = 8
min_region = 32
subregions_from = 256
critical_section = "basepri"
np_max_us = 40
dma_stop_max_us = 10
reset_cleanup = "peripheral-reset"
background_privileged = true
unwind = "cortex-m-exception"
timer = "systick"
fault_address = true

[boot.record]
kind = "noinit-ram"
at = 0x2000_F000
size = 0x400
survives = "reset-random"
rings = [{{ writer = "bootcore", at = 0x2000_F000, size = 0x400 }}]
fuse = {{ cells = [{{ image = "app", at = 0x2000_F400, size = 0x100 }}] }}
{BLOCKS}"#
    )
}

/// The fake board with a boot table, a program and capability tables
/// (every numeric board field appears).
pub(super) fn boot_board() -> String {
    format!(
        r#"id = "fake-boot"
{FAKE_BOARD}
[boot]
led = "GPIO2"
halt = {{ pattern = "blink", period_ms = 100 }}
power_hold = {{ pin = "GPIO5", level = "high" }}

[[boot.action]]
on = "watchdog_armed"
ops = [
    {{ op = "gpio", pin = "GPIO5", level = "high" }},
    {{ op = "delay", ms = 2 }},
    {{ op = "led", state = "on" }},
    {{ op = "bus_write", device = "pmic", reg = 0x10, bytes = [0x01] }},
]

[[pin]]
pin = "GPIO2"

[[pin]]
pin = "GPIO5"
boot = true
levels = ["high"]

[[bus]]
id = "sys"
kind = "i2c"
controller = "uart3"
sda = "GPIO6"
scl = "GPIO7"

[[peripheral]]
id = "can0"
mcu = "can1"

[[device]]
id = "pmic"
bus = "sys"
addr = 0x34
writable = [0x10]
"#
    )
}

fn isolated(from: &str, to: &str) -> String {
    let text = isolated_mcu();
    assert!(text.contains(from), "{from}");
    text.replace(from, to)
}

#[test]
fn the_isolated_fake_loads_and_reports_its_kind() {
    let m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    let i = m.isolation();
    assert_eq!((i.kind, i.level), (Kind::Armv7mMpu, Level::Mpu));
    assert_eq!((m.np_max_us(), m.dma_stop_max_us()), (40, 10));
    assert_eq!(m.watchdog.unwrap().reboot_delay_max_ms(), 2_000);
    // Left out, [isolation] is kind none, level none.
    let plain = parse_mcu_text(FAKE_MCU, &[]).unwrap();
    assert_eq!(plain.isolation().kind, Kind::None);
    assert_eq!(plain.np_max_us(), 50);
}

#[test]
fn every_builtin_states_its_isolation_watchdog_and_record() {
    for m in Registry::builtin().mcus() {
        let i = m.isolation();
        assert!(m.isolation.is_some(), "{}", m.id);
        assert!(m.watchdog.is_some(), "{}", m.id);
        assert!(!m.cores.is_empty(), "{}", m.id);
        // A DAGON boot stage keeps a boot record; others have none.
        assert_eq!(i.boot_extensions, m.record().is_some(), "{}", m.id);
    }
}

#[test]
fn kind_records_are_checked_against_their_architecture() {
    for (from, to, want) in [
        ("mpu_regions = 8", "mpu_regions = 12", "not 12"),
        ("min_region = 32", "min_region = 48", "power of two"),
        ("min_region = 32", "min_region = 16", "at least 32"),
        (
            "critical_section = \"basepri\"",
            "critical_section = \"primask\"",
            "critical_section",
        ),
        (
            "unwind = \"cortex-m-exception\"",
            "unwind = \"riscv-trap\"",
            "unwind",
        ),
        ("timer = \"systick\"", "timer = \"riscv-mtime\"", "timer"),
        ("np_max_us = 40", "np_max_us = 60", "only lower"),
        ("dma_stop_max_us = 10", "dma_stop_max_us = 0", "only lower"),
        ("ext_ram_max = 16_384", "ext_ram_max = 0", "ext_ram_max"),
        (
            "level = \"mpu\"",
            "level = \"pms\"",
            "not one kind armv7m-mpu gives",
        ),
        (
            "mpu_regions = 8\n",
            "mpu_regions = 8\npmp_entries = 16\n",
            "not part of kind",
        ),
        ("mpu_regions = 8\n", "", "mpu_regions is required"),
        ("fault_address = true\n", "", "fault_address"),
    ] {
        let e = mcu_err(&isolated(from, to));
        assert!(
            e.contains(want) && e.contains("[protection map generator"),
            "{to}: {e}"
        );
    }
    // An armv6m-mpu record needs 256-byte regions and primask.
    let v6 = isolated("kind = \"armv7m-mpu\"", "kind = \"armv6m-mpu\"");
    assert!(mcu_err(&v6).contains("at least 256"));
    let v6 = v6
        .replace(
            "critical_section = \"basepri\"",
            "critical_section = \"primask\"",
        )
        .replace("min_region = 32", "min_region = 256");
    parse_mcu_text(&v6, &[]).unwrap();
    // Zero regions: level none only.
    let e = mcu_err(&v6.replace("mpu_regions = 8", "mpu_regions = 0"));
    assert!(e.contains("level none"), "{e}");
}

#[test]
fn missing_classes_and_bus_master_windows_are_refused() {
    for (from, to, want) in [
        (
            "privileged_why = \"flash\"",
            "privileged_why = \"shared\"",
            "\"flash\"",
        ),
        (
            "privileged_why = \"watchdog\"",
            "privileged_why = \"shared\"",
            "\"watchdog\"",
        ),
        (
            "privileged_why = [\"clock\", \"reset\"]",
            "privileged_why = \"clock\"",
            "\"reset\"",
        ),
        (
            "privileged_why = \"irq-routing\"",
            "privileged_why = \"shared\"",
            "\"irq-routing\"",
        ),
        (
            "bus_master = true\nprivileged_why = \"dma\"",
            "bus_master = true\nwindow_eligible = true",
            "bus master and can never be window-eligible",
        ),
        (
            "id = \"uart3\"\nbase = 0x4000_4800\nsize = 0x400\nwindow_eligible = true",
            "id = \"uart3\"\nbase = 0x4000_4800\nsize = 0x400",
            "say why",
        ),
        ("base = 0x4000_4800", "base = 0x4000_6600", "overlaps"),
        ("base = 0x4000_4800", "base = 0x2000_0000", "overlaps ram"),
        (
            "base = 0x4000_4800",
            "base = 0x0010_0000",
            "overlaps a flash map",
        ),
        ("[[core]]\nid = \"cpu0\"\n", "", "[[core]]"),
    ] {
        let e = mcu_err(&isolated(from, to));
        assert!(
            e.contains(want) && e.contains("[isolation checker]"),
            "{to}: {e}"
        );
    }
    // A second core needs a core-control block.
    let two = isolated(
        "[[core]]\nid = \"cpu0\"\n",
        "[[core]]\nid = \"cpu0\"\n\n[[core]]\nid = \"cpu1\"\n",
    );
    assert!(mcu_err(&two).contains("\"core-control\""));
    // Kind none needs no blocks.
    let none = FAKE_MCU.to_string() + "\n[isolation]\nkind = \"none\"\n";
    parse_mcu_text(&none, &[]).unwrap();
    assert!(Why::Protection.as_str() == "protection");
}

#[test]
fn the_record_lies_inside_ram_within_its_limit() {
    for (from, to, want) in [
        (
            "size = 0x400\nsurvives",
            &*format!("size = {:#x}\nsurvives", RECORD_MAX + 1),
            "boot.record.size",
        ),
        (
            "at = 0x2000_F000\nsize",
            "at = 0x2001_0000\nsize",
            "outside its memory",
        ),
        (
            "{ writer = \"bootcore\", at = 0x2000_F000, size = 0x400 }",
            "{ writer = \"stage0\", at = 0x2000_F000, size = 0x400 }",
            "no such boot stage",
        ),
        (
            "{ writer = \"bootcore\", at = 0x2000_F000, size = 0x400 }",
            "{ writer = \"bootcore\", at = 0x2000_F200, size = 0x400 }",
            "outside the record",
        ),
        (
            "{ image = \"app\", at = 0x2000_F400",
            "{ image = \"app\", at = 0x2000_F300",
            "overlaps the record",
        ),
        (
            "ram = { at = 0x2000_0000, size = 0x1_0000 }\n",
            "",
            "needs the MCU's ram",
        ),
    ] {
        let e = mcu_err(&isolated(from, to));
        assert!(e.contains(want), "{to}: {e}");
    }
    let e = mcu_err(&isolated("trial_ms = 4_000", "trial_ms = 0"));
    assert!(e.contains("[watchdog driver"), "{e}");
}

fn board(text: &str) -> BoardDef {
    toml::from_str(text).unwrap()
}

#[test]
fn board_boot_tables_are_checked_against_board_and_mcu() {
    let mut m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    let text = boot_board();
    // bus_write needs the MCU's i2c boot capability.
    let e = requirements::check_board(&board(&text), &m).unwrap_err();
    assert!(e.contains("boot_capabilities"), "{e}");
    m.boot_capabilities = vec![super::isolation::BootCapability::I2c];
    requirements::check_board(&board(&text), &m).unwrap();
    for (from, to, want) in [
        (
            "\"GPIO5\", level = \"high\" },",
            "\"GPIO5\", level = \"low\" },",
            "does not allow",
        ),
        (
            "{ op = \"delay\", ms = 2 }",
            "{ op = \"delay\", ms = 21 }",
            "delay 21",
        ),
        ("reg = 0x10, bytes", "reg = 0x11, bytes", "not writable"),
        (
            "bytes = [0x01]",
            "bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9]",
            "1..=8 bytes",
        ),
        ("led = \"GPIO2\"", "led = \"GPIO9\"", "not in [[pin]]"),
        ("mcu = \"can1\"", "mcu = \"can9\"", "no [[peripheral]]"),
        ("addr = 0x34", "addr = 0x03", "outside 0x08..=0x77"),
        ("period_ms = 100", "period_ms = 0", "period_ms"),
    ] {
        let t = text.replace(from, to);
        assert_ne!(t, text, "{from}");
        let e = requirements::check_board(&board(&t), &m).unwrap_err();
        assert!(e.contains(want), "{to}: {e}");
    }
    // Ten delays of 6 ms: over the per-event total.
    let long = text.replace(
        "{ op = \"delay\", ms = 2 },",
        &"{ op = \"delay\", ms = 6 },".repeat(10),
    );
    let e = requirements::check_board(&board(&long), &m).unwrap_err();
    assert!(e.contains("over the 50 ms"), "{e}");
}

#[test]
fn board_wiring_is_checked_for_the_code_generated_from_it() {
    let mut m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    m.boot_capabilities = vec![super::isolation::BootCapability::I2c];
    let text = boot_board();
    let named = text.replace("pin = \"GPIO2\"\n", "pin = \"GPIO2\"\nname = \"led\"\n");
    requirements::check_board(&board(&named), &m).unwrap();
    for (from, to, want) in [
        ("sda = \"GPIO6\"", "sda = \"GPIO2\"", "pin 'GPIO2' is both"),
        ("sda = \"GPIO6\"\n", "", "names its sda pin"),
        (
            "scl = \"GPIO7\"\n",
            "scl = \"GPIO7\"\ntx = \"GPIO8\"\n",
            "has no tx pin",
        ),
        (
            "scl = \"GPIO7\"\n",
            "scl = \"GPIO7\"\nbaud = 9600\n",
            "has no baud",
        ),
        ("scl = \"GPIO7\"\n", "scl = \"GPIO7\"\nhz = 0\n", "speed 0"),
        ("name = \"led\"", "name = \"Led\"", "must be a-z"),
        ("id = \"sys\"", "id = \"9sys\"", "ids are a-z"),
        (
            "mcu = \"can1\"",
            "mcu = \"uart3\"",
            "drives DAGON's link (bus 'sys')",
        ),
    ] {
        let t = named
            .replace("scl = \"GPIO7\"\n", "scl = \"GPIO7\"\ndagon = true\n")
            .replace(from, to);
        // The device follows its bus's new id.
        let t = match to.strip_prefix("id = ") {
            Some(id) => t.replace("bus = \"sys\"", &format!("bus = {id}")),
            None => t,
        };
        let e = requirements::check_board(&board(&t), &m).unwrap_err();
        assert!(e.contains(want), "{to}: {e}");
    }
}

#[test]
fn coordinator_pins_are_dagons_link_buses() {
    let reg = Registry::builtin();
    let hub = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
    let m = reg.mcu(&hub.mcu).unwrap();
    let mut moved = hub.clone();
    moved.buses.iter_mut().for_each(|b| {
        if b.id == "child-i2c" {
            b.sda = Some("GPIO20".into());
        }
    });
    let e = requirements::check_board(&moved, m).unwrap_err();
    assert!(e.contains("coordinator.i2c is no dagon = true"), "{e}");
    let mut uart = hub.clone();
    uart.buses
        .iter_mut()
        .for_each(|b| b.dagon &= b.kind != super::board_boot::BusKind::Uart);
    let e = requirements::check_board(&uart, m).unwrap_err();
    assert!(e.contains("coordinator.pio_uart"), "{e}");
}

#[test]
fn kind_names_round_trip_through_the_definition_format() {
    #[derive(serde::Deserialize)]
    struct K {
        kind: Kind,
    }
    for r in super::isolation::KIND_RULES {
        let k: K = toml::from_str(&format!("kind = {:?}", r.name)).unwrap();
        assert_eq!(k.kind, r.kind, "{}", r.name);
        assert_eq!(r.kind.as_str(), r.name);
    }
}

#[test]
fn flash_offsets_of_an_mmu_mapped_mcu_share_no_space_with_register_blocks() {
    // The same block at an address inside the fake MCU's flash map:
    // refused when the definition says the CPU reads flash there (xip),
    // accepted when its flash addresses are offsets (xip = false), whatever
    // the port kind.
    let low = isolated("base = 0x4000_4800", "base = 0x0010_0000");
    assert!(mcu_err(&low).contains("overlaps a flash map"));
    let offsets = low.replace("xip = true\n", "xip = false\n");
    // Every flash map says which: no silent default.
    let e = mcu_err(&low.replace("xip = true\n", ""));
    assert!(e.contains("missing field `xip`"), "{e}");
    let m = parse_mcu_text(&offsets, &[]).unwrap();
    assert!(!m.flash_is_cpu_mapped());
    // The shipped MCUs: the two that run from flash in place, and not the
    // MMU-mapped one.
    let mapped: Vec<(&str, bool)> = Registry::builtin()
        .mcus()
        .iter()
        .map(|m| (m.id.as_str(), m.flash_is_cpu_mapped()))
        .collect();
    for want in [("rp2040", true), ("stm32f401re", true), ("esp32s3", false)] {
        assert!(mapped.contains(&want), "{want:?}");
    }
}
