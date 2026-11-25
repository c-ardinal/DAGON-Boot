//! The isolation schema: kind records, register blocks, required classes,
//! watchdog, boot record placement, and the board's boot and capability
//! tables.

use super::isolation::{Kind, Level, Why};
use super::load::parse_mcu_text;
use super::tests::{mcu_err, FAKE_BOARD, FAKE_MCU};
use super::{requirements, BoardDef, PortKind, Registry};

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

/// The fake MCU with an armv7m-mpu record, its blocks and a watchdog
/// (every numeric isolation field appears).
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
trial_attempts = 3
flag = "register"
range_ms = {{ min = 1, max = 32_768 }}

[[slot_layout]]
target = "bootcore"
slot = "a"
start = 0x0010_0000
capacity = 0x7E00
header = {{ detached = 0x0010_7E00 }}

[[slot_layout]]
target = "app"
slot = "a"
start = 0x0011_0000
capacity = 0x3_FE00
header = "inline"

[[slot_layout]]
target = "app"
slot = "b"
start = 0x0015_0000
capacity = 0x3_FE00
header = "inline"

[execution]
masked_max_us = 50
flash_op = "fetch-stall"
write_unit = 4
erased_value = 0xFF
erase_regions = [
    {{ start = 0x0010_0000, len = 0x8000, unit = 0x4000, unit_max_ms = 500 }},
    {{ start = 0x0010_8000, len = 0xF_8000, unit = 0x4000, unit_max_ms = 1_000 }},
]

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
max_transfer = 1
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
fn every_builtin_states_its_isolation_and_watchdog() {
    for m in Registry::builtin().mcus() {
        let i = m.isolation();
        assert!(m.isolation.is_some(), "{}", m.id);
        assert!(m.watchdog.is_some(), "{}", m.id);
        assert!(!m.cores.is_empty(), "{}", m.id);
        // Only a DAGON boot stage runs boot extensions.
        assert!(
            !i.boot_extensions || m.port == PortKind::SelfBoot,
            "{}",
            m.id
        );
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

/// `[watchdog] range_ms`: the periods the MCU's watchdog can run. A trial
/// or confirmed period on either boundary loads; one past either is
/// refused, naming the MCU, the value and the range; a definition without
/// the range is refused.
#[test]
fn the_watchdog_periods_are_inside_the_range_the_mcu_can_run() {
    let with = |range: &str, trial: &str, confirmed: &str| {
        isolated("range_ms = { min = 1, max = 32_768 }", range)
            .replace("trial_ms = 4_000", trial)
            .replace("confirmed_ms = 8_000", confirmed)
    };
    let range = "range_ms = { min = 2_000, max = 9_000 }";
    for (trial, confirmed) in [("2_000", "9_000"), ("9_000", "2_000")] {
        parse_mcu_text(
            &with(
                range,
                &format!("trial_ms = {trial}"),
                &format!("confirmed_ms = {confirmed}"),
            ),
            &[],
        )
        .unwrap();
    }
    for (trial, confirmed, says) in [
        ("1_999", "8_000", "watchdog.trial_ms 1999"),
        ("2_000", "9_001", "watchdog.confirmed_ms 9001"),
    ] {
        let e = mcu_err(&with(
            range,
            &format!("trial_ms = {trial}"),
            &format!("confirmed_ms = {confirmed}"),
        ));
        assert!(e.contains(says), "{e}");
        assert!(
            e.contains("MCU 'fake-m0'") && e.contains("2000..=9000 ms"),
            "{e}"
        );
    }
    let e = mcu_err(&with("", "trial_ms = 4_000", "confirmed_ms = 8_000"));
    assert!(e.contains("range_ms"), "{e}");
    let e = mcu_err(&with(
        "range_ms = { min = 9_000, max = 2_000 }",
        "trial_ms = 4_000",
        "confirmed_ms = 8_000",
    ));
    assert!(e.contains("range_ms"), "{e}");
}

/// `[execution]`: two erases fit every watchdog period; `[conformance]`: the
/// scratch never touches an image slot.
#[test]
fn execution_budgets_and_conformance_scratch_are_checked() {
    let with = |execution: &str, conformance: &str| {
        isolated(
            "unit_max_ms = 1_000 },
]
",
            &format!(
                "unit_max_ms = {execution} }},
]

[conformance]
scratch = {conformance}
"
            ),
        )
    };
    let free = "{ at = 0x0010_A000, size = 0x1000 }";
    parse_mcu_text(&with("1_000", free), &[]).unwrap();
    // The fake's trial period is 4000 ms: two erases of 2001 ms do not fit.
    parse_mcu_text(&with("2_000", free), &[]).unwrap();
    let e = mcu_err(&with("2_001", free));
    assert!(
        e.contains("watchdog.trial_ms 4000 must be at least 2 x"),
        "{e}"
    );
    let slot = parse_mcu_text(&with("1_000", free), &[]).unwrap().slots[0].start;
    let e = mcu_err(&with(
        "1_000",
        &format!("{{ at = {slot:#x}, size = 0x100 }}"),
    ));
    assert!(e.contains("overlaps a slot"), "{e}");
    let e = mcu_err(&with("1_000", "{ at = 0x7000_0000, size = 0x100 }"));
    assert!(e.contains("outside the default flash map"), "{e}");
    // Facts not yet established: an unverified value is still checked, an
    // unmeasured erase time is refused (the watchdog check needs one).
    let m = parse_mcu_text(&with("{ unverified = 1_000 }", free), &[]).unwrap();
    assert!(!m.execution.unwrap().erase_regions[1].unit_max_ms.is_known());
    let e = mcu_err(&with("{ unverified = 9_000 }", free));
    assert!(e.contains("must be at least 2 x"), "{e}");
    let e = mcu_err(&with("\"unmeasured\"", free));
    assert!(e.contains("needs a value"), "{e}");
    let unmeasured = isolated("masked_max_us = 50", "masked_max_us = \"unmeasured\"");
    let m = parse_mcu_text(&unmeasured, &[]).unwrap();
    assert_eq!(m.execution.unwrap().masked_max_us.value(), None);
}

/// `[execution] erase_regions` and `[[slot_layout]]`: regions are whole
/// units apart from each other, every target and slot the node receives
/// into has one layout that agrees with its `[[slot]]`, and the erase of
/// its largest body lies in the regions.
#[test]
fn erase_regions_and_slot_layouts_are_checked() {
    let m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    let layout = m.erase_layout().unwrap();
    assert_eq!((layout.regions.len(), layout.slots.len()), (2, 3));
    for (from, to, want) in [
        (
            "len = 0x8000, unit = 0x4000",
            "len = 0x9000, unit = 0x4000",
            "whole number of units",
        ),
        (
            "len = 0x8000, unit = 0x4000",
            "len = 0x8000, unit = 0",
            "whole number of units",
        ),
        (
            "start = 0x0010_8000, len",
            "start = 0x0010_4000, len",
            "overlaps another erase region",
        ),
        (
            "start = 0x0010_8000, len = 0xF_8000",
            "start = 0x0010_8000, len = 0x8000",
            "lies in no execution.erase_regions",
        ),
        (
            "start = 0x0010_0000, len = 0x8000",
            "start = 0xFFFF_C000, len = 0x8000",
            "runs past 2^32",
        ),
        (
            "unit_max_ms = 500",
            "unit_max_ms = 2_001",
            "must be at least 2 x",
        ),
        (
            "unit_max_ms = 500",
            "unit_max_ms = \"unmeasured\"",
            "needs a value",
        ),
        (
            "target = \"app\"\nslot = \"b\"",
            "target = \"app\"\nslot = \"a\"",
            "is given twice",
        ),
        (
            "target = \"app\"\nslot = \"b\"",
            "target = \"topology\"\nslot = \"b\"",
            "target `topology` was removed",
        ),
        (
            "target = \"app\"\nslot = \"b\"",
            "target = \"topology\"\nslot = \"a\"",
            "target `topology` was removed: the compiled topology stays with the host",
        ),
        (
            "target = \"app\"\nslot = \"b\"",
            "target = \"apps\"\nslot = \"b\"",
            "unknown target `apps`, expected one of `app`, `bootcore`, `agent`",
        ),
        (
            "target = \"app\"\nslot = \"b\"",
            "target = \"agent\"\nslot = \"factory\"",
            "needs a slot_layout for app/b",
        ),
        (
            "start = 0x0015_0000\ncapacity = 0x3_FE00",
            "start = 0x0015_0000\ncapacity = 0x3_FC00",
            "must be slot app/b's",
        ),
        (
            "capacity = 0x7E00\nheader = { detached = 0x0010_7E00 }",
            "capacity = 0x7E00\nheader = \"inline\"",
            "must be slot bootcore/a's",
        ),
        (
            "start = 0x0011_0000\ncapacity = 0x3_FE00",
            "start = 0x0011_0000\ncapacity = 0x10_0000",
            "lies in no execution.erase_regions",
        ),
    ] {
        let text = isolated_mcu();
        assert!(text.contains(from), "{from}");
        let e = mcu_err(&text.replacen(from, to, 1));
        assert!(e.contains(want), "{to}: {e}");
    }
    // Layouts need the regions to plan from.
    let bare = isolated_mcu();
    let at = bare.find("[execution]").unwrap();
    let end = bare[at..].find("\n[isolation]").unwrap();
    let e = mcu_err(&format!("{}{}", &bare[..at], &bare[at + end..]));
    assert!(e.contains("needs [execution] erase_regions"), "{e}");
}

fn board(text: &str) -> BoardDef {
    toml::from_str(text).unwrap()
}

/// Stage0 blinks its code in tenths of the board's halt period: on an MCU
/// with a DAGON stage0, a blinking halt pattern on the boot LED needs at
/// least 10 ms (9 is refused, 10 taken); without a stage0, or with the
/// pattern off, any period stands.
#[test]
fn a_stage0_halt_blink_period_is_at_least_ten_ms() {
    let mut m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    m.boot_capabilities = vec![super::isolation::BootCapability::I2c];
    let at =
        |ms: u32| board(&boot_board().replace("period_ms = 100", &format!("period_ms = {ms}")));
    m.stage0 = Some(super::Stage0Def {
        key: "flash".into(),
        region_bytes: 0x4000,
        head_bytes: 0,
        clock: Vec::new(),
    });
    let e = requirements::check_board(&at(9), &m).unwrap_err();
    assert!(e.contains("at least 10 ms"), "{e}");
    requirements::check_board(&at(10), &m).unwrap();
    let off = boot_board().replace(
        "{ pattern = \"blink\", period_ms = 100 }",
        "{ pattern = \"off\", period_ms = 1 }",
    );
    requirements::check_board(&board(&off), &m).unwrap();
    m.stage0 = None;
    requirements::check_board(&at(9), &m).unwrap();
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

/// `text` with its `GPIO2` pin named `name`.
fn pin_named(text: &str, name: &str) -> String {
    let named = text.replace(
        "pin = \"GPIO2\"\n",
        &format!("pin = \"GPIO2\"\nname = {name:?}\n"),
    );
    assert_ne!(named, text);
    named
}

#[test]
fn definition_names_become_code_names_or_are_refused() {
    let mut m = parse_mcu_text(&isolated_mcu(), &[]).unwrap();
    m.boot_capabilities = vec![super::isolation::BootCapability::I2c];
    let text = boot_board();
    let named = text.replace(
        "writable = [0x10]",
        "regs = [{ name = \"ctl\", reg = 0x10 }]\nwritable = [\"ctl\"]",
    );
    let b = board(&named);
    requirements::check_board(&b, &m).unwrap();
    assert_eq!(b.devices[0].writable_regs(), [0x10]);
    let second_bus = "\n[[bus]]\nid = \"sys-2\"\nkind = \"i2c\"\ncontroller = \"uart3\"\nsda = \"GPIO8\"\nscl = \"GPIO9\"\n";
    // Every device states its maximum transfer, and the bus port and boot
    // programs keep to it.
    let missing = named.replace("max_transfer = 1\n", "");
    let e = toml::from_str::<BoardDef>(&missing).unwrap_err();
    assert!(e.to_string().contains("max_transfer"), "{e}");
    let zero = named.replace("max_transfer = 1\n", "max_transfer = 0\n");
    let e = requirements::check_board(&board(&zero), &m).unwrap_err();
    assert!(e.contains("max_transfer is at least 1"), "{e}");
    let long = named.replace("bytes = [0x01]", "bytes = [0x01, 0x02]");
    let e = requirements::check_board(&board(&long), &m).unwrap_err();
    assert!(e.contains("takes at most 1 bytes"), "{e}");
    let second_device =
        "\n[[device]]\nid = \"pmic-2\"\nbus = \"sys\"\naddr = 0x35\nmax_transfer = 1\n";
    for (t, want) in [
        (
            named
                .replace("id = \"pmic\"", "id = \"pmic.v2\"")
                .replace("device = \"pmic\"", "device = \"pmic.v2\""),
            "ids are a-z",
        ),
        (
            named.replace("writable = [\"ctl\"]", "writable = [\"nope\"]"),
            "writable 'nope' is not in its regs",
        ),
        (
            named
                .replace("name = \"ctl\"", "name = \"Ctl\"")
                .replace("[\"ctl\"]", "[\"Ctl\"]"),
            "register name 'Ctl'",
        ),
        (
            named.clone()
                + second_bus
                + &second_bus
                    .replace("sys-2", "sys_2")
                    .replace("GPIO8", "GPIO10")
                    .replace("GPIO9", "GPIO11"),
            "generated name",
        ),
        (
            named.clone()
                + second_device
                + &second_device
                    .replace("pmic-2", "pmic_2")
                    .replace("0x35", "0x36"),
            "device id after - becomes _",
        ),
        // One type name from two code names: `foo_bar` and `foo__bar`.
        (
            named.clone()
                + &second_bus.replace("sys-2", "foo_bar")
                + &second_bus
                    .replace("sys-2", "foo__bar")
                    .replace("GPIO8", "GPIO10")
                    .replace("GPIO9", "GPIO11"),
            "generated type name",
        ),
        // ... and `a_1` and `a1`.
        (
            pin_named(&named, "a_1") + "\n[[pin]]\npin = \"GPIO12\"\nname = \"a1\"\n",
            "generated type name",
        ),
        // Keywords, in the code form (`fn`) and in the type form (`self`
        // becomes `Self`).
        (pin_named(&named, "fn"), "is a Rust keyword"),
        // Not code names: a leading underscore, a raw identifier, upper case.
        (pin_named(&named, "_led"), "must be a-z"),
        (pin_named(&named, "r#led"), "must be a-z"),
        (pin_named(&named, "Led"), "must be a-z"),
        // Mixed separators meet after `-` becomes `_`: `sys-2` and `sys_2`
        // above; `a-b_c` and `a_b-c` here.
        (
            named.clone()
                + &second_bus.replace("sys-2", "a-b_c")
                + &second_bus
                    .replace("sys-2", "a_b-c")
                    .replace("GPIO8", "GPIO10")
                    .replace("GPIO9", "GPIO11"),
            "generated name",
        ),
        // A digit boundary in the type form: `led_2` and `led2` are both `Led2`.
        (
            pin_named(&named, "led_2") + "\n[[pin]]\npin = \"GPIO12\"\nname = \"led2\"\n",
            "generated type name",
        ),
        // Upper-case constants: devices `io-x` and `io_x` would both be
        // `DEVICE_IO_X_*`.
        (
            named.clone()
                + &second_device.replace("pmic-2", "io-x")
                + &second_device
                    .replace("pmic-2", "io_x")
                    .replace("0x35", "0x36"),
            "device id after - becomes _",
        ),
        (pin_named(&named, "self"), "is a Rust keyword"),
        (
            named.clone() + &second_bus.replace("sys-2", "type"),
            "is a Rust keyword",
        ),
        (
            named
                .replace("id = \"pmic\"", "id = \"match\"")
                .replace("device = \"pmic\"", "device = \"match\""),
            "device id 'match' is a Rust keyword",
        ),
        (
            named
                .replace("name = \"ctl\"", "name = \"loop\"")
                .replace("[\"ctl\"]", "[\"loop\"]"),
            "is a Rust keyword",
        ),
    ] {
        let e = requirements::check_board(&board(&t), &m).expect_err(want);
        assert!(e.contains(want), "{want}: {e}");
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
    assert_eq!(moved.coordinator_pins(m).unwrap().i2c.sda, 20);
    let mut uart = hub.clone();
    uart.buses
        .iter_mut()
        .for_each(|b| b.dagon &= b.kind != super::board_boot::BusKind::Uart);
    assert_eq!(uart.coordinator_pins(m), None);
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

#[test]
fn the_ram_and_the_watchdog_are_checked() {
    let e = mcu_err(&isolated("size = 0x1_0000 }", "size = 0 }"));
    assert!(e.contains("is empty or past 2^32"), "{e}");
    let e = mcu_err(&isolated(
        "ram = { at = 0x2000_0000, size",
        "ram = { at = 0xFFFF_F000, size",
    ));
    assert!(e.contains("is empty or past 2^32"), "{e}");
    let e = mcu_err(&isolated("trial_ms = 4_000", "trial_ms = 0"));
    assert!(e.contains("[watchdog driver"), "{e}");
    let e = mcu_err(&isolated("trial_attempts = 3", "trial_attempts = 0"));
    assert!(e.contains("trial_attempts must be above 0"), "{e}");
}

/// `[watchdog] flag`: required, a fact with an establishment state.
#[test]
fn the_watchdog_flag_is_a_stated_fact() {
    use super::execution::Fact;
    use super::isolation::WatchdogFlag;

    let reg = Registry::builtin();
    let flag = |id: &str| reg.mcu(id).unwrap().watchdog.unwrap().flag;
    assert_eq!(
        flag("stm32f401re"),
        Fact::Unverified {
            unverified: WatchdogFlag::Register
        }
    );
    assert_eq!(
        flag("rp2040"),
        Fact::Unverified {
            unverified: WatchdogFlag::Package
        }
    );
    assert_eq!(flag("esp32s3").value(), None, "unmeasured");

    // A required key; unknown words.
    let e = mcu_err(&isolated("flag = \"register\"\n", ""));
    assert!(e.contains("flag"), "{e}");
    assert!(parse_mcu_text(&isolated("\"register\"", "\"fuse\""), &[]).is_err());
    for word in ["\"none\"", "\"unmeasured\"", "{ unverified = \"package\" }"] {
        parse_mcu_text(&isolated("\"register\"", word), &[]).unwrap();
    }
}
