//! Manifests, node checks, the two PMSA kinds' maps for the shipped
//! definitions and synthetic ones, and the generated files.

use dagon_catalog::Layer;
use dagon_topology::defs::{Binding, BoardDef, McuDef, Registry};

use crate::kind::{self, Access, Memory, Purpose, Space};
use crate::node::{self, Ext, Image};
use crate::{gen, EventKind, Known, Manifest};

const RP2040: &str = include_str!("../../../targets/mcu/rp2040/dagon-mcu.toml");
const STM32: &str = include_str!("../../../targets/mcu/stm32f401re/dagon-mcu.toml");
const ESP32S3: &str = include_str!("../../../targets/mcu/esp32s3/dagon-mcu.toml");
const PARTITIONS: &str = include_str!("../../../targets/mcu/esp32s3/partitions.csv");

/// A catalog with the boot and app kinds the examples observe.
fn known() -> Known {
    let k = |layer, kind, action| EventKind {
        layer,
        kind,
        live: true,
        action,
    };
    Known {
        kinds: vec![
            k(Layer::Boot, "watchdog_armed", true),
            k(Layer::Boot, "before_jump", true),
            k(Layer::Boot, "entry", false),
            k(Layer::App, "update_offered", false),
            k(Layer::App, "transfer_started", false),
            k(Layer::App, "transfer_done", false),
        ],
        ports: dagon_catalog::PORTS,
        decisions: dagon_catalog::DECISIONS,
    }
}

/// The design's UDS transport manifest.
const UDS: &str = r#"
format = 1
id = "uds-transport"
layers = ["boot", "app"]
provides = ["UpdateTransport", "UpdateProtocol"]
observes = ["boot.*", "app.update_*", "app.transfer_*"]
decides = ["accept_update"]
trusted = false
[capabilities]
peripherals = ["can0"]
pins = []
buses = []
timer = true
[limits]
max_run_us = 2000
stack_bytes = 1024
ram_bytes = 4096
text_bytes = 16384
"#;

fn manifest(text: &str) -> Result<Manifest, String> {
    Manifest::parse(text, "test", &known()).map_err(|e| e.msg)
}

fn uds(from: &str, to: &str) -> Result<Manifest, String> {
    assert!(UDS.contains(from), "{from}");
    manifest(&UDS.replace(from, to))
}

#[test]
fn the_design_example_manifest_is_valid() {
    let m = manifest(UDS).unwrap();
    assert_eq!(m.layers, [Layer::Boot, Layer::App]);
    assert!(m.sandboxed());
}

#[test]
fn manifests_are_checked_against_the_catalogs() {
    for (from, to, want) in [
        ("format = 1", "format = 2", "format 2"),
        ("id = \"uds-transport\"", "id = \"UDS\"", "a-z 0-9 -"),
        (
            "[\"boot\", \"app\"]",
            "[\"boot\", \"cloud\"]",
            "layer 'cloud'",
        ),
        ("\"UpdateProtocol\"]", "\"NoSuchPort\"]", "no such port"),
        (
            "\"UpdateProtocol\"]",
            "\"SelfCheck\"]",
            "does not exist in layer boot",
        ),
        (
            "\"UpdateProtocol\"]",
            "\"ImageVerifier\"]",
            "trusted-only port",
        ),
        ("\"UpdateProtocol\"]", "\"ImageLifecycle\"]", "no such port"),
        ("\"boot.*\"", "\"boot.nope\"", "no event of the catalog"),
        ("\"boot.*\"", "\"disk.*\"", "no layer 'disk'"),
        ("\"boot.*\"", "\"boot.*x\"", "only at the end"),
        ("[\"accept_update\"]", "[\"confirm_plan\"]", "layer host"),
        (
            "[\"accept_update\"]",
            "[\"nope\"]",
            "no such decision point",
        ),
        ("max_run_us = 2000", "max_run_us = 60000", "max_run_us"),
        ("stack_bytes = 1024", "stack_bytes = 64", "below 256"),
        ("text_bytes = 16384", "", "text_bytes"),
        (
            "timer = true",
            "timer = true\nclock = true",
            "unknown field",
        ),
    ] {
        let e = uds(from, to).unwrap_err();
        assert!(e.contains(want), "{to}: {e}");
    }
    // A trusted verifier: no decisions, no limits needed.
    let verifier = "format = 1\nid = \"hsm-verify\"\nlayers = [\"boot\"]\nprovides = [\"ImageVerifier\"]\ntrusted = true\n";
    assert!(manifest(verifier).unwrap().trusted);
    let e = manifest(&verifier.replace("[\"ImageVerifier\"]", "[]")).unwrap_err();
    assert!(e.contains("no trusted-only port"), "{e}");
}

#[test]
fn dagons_own_catalog_validates_observes() {
    let k = Known::dagon();
    k.observable("app.extension_overran").unwrap();
    k.observable("boot.*").unwrap();
    k.observable("app.extension_*").unwrap();
    assert!(k.observable("app.no_such_kind").is_err());
}

/// A registry of `mcu` (builtin text or synthetic) and `board`.
fn registry(mcu: &str, files: &[(&str, &str)], board: &str) -> Registry {
    let json = serde_json::json!({
        "mcus": [{ "label": "test MCU", "toml": mcu, "files": files }],
        "boards": [{ "label": "test board", "toml": board }],
    });
    Registry::from_json(&json.to_string()).unwrap()
}

fn board_for(mcu: &str, id: &str, windows: &[(&str, &str)]) -> String {
    let mut b = format!(
        "id = {id:?}\nproduct = \"Test\"\nmcu = {mcu:?}\nflash_bytes = 0x100_0000\ndrawing = \"pico\"\n\n\
         [[pin]]\npin = \"GPIO2\"\n\n[[pin]]\npin = \"GPIO5\"\nboot = true\n"
    );
    for (board_id, mcu_id) in windows {
        b.push_str(&format!(
            "\n[[peripheral]]\nid = {board_id:?}\nmcu = {mcu_id:?}\n"
        ));
    }
    b
}

fn ext_manifest(id: &str, peripherals: &[&str]) -> Manifest {
    let list: Vec<String> = peripherals.iter().map(|p| format!("{p:?}")).collect();
    manifest(&format!(
        "format = 1\nid = {id:?}\nlayers = [\"boot\", \"app\"]\nobserves = [\"boot.*\"]\n\
         [capabilities]\nperipherals = [{}]\n\
         [limits]\nmax_run_us = 1000\nstack_bytes = 1024\nram_bytes = 2048\ntext_bytes = 4096\n",
        list.join(", ")
    ))
    .unwrap()
}

fn ext(m: &Manifest) -> Ext<'_> {
    Ext {
        manifest: m,
        krate: "crate::tests::fake_ext".into(),
        sha256: None,
    }
}

fn parts(reg: &Registry) -> (&McuDef, &BoardDef) {
    let b = &reg.boards()[0];
    (reg.mcu(&b.mcu).unwrap(), b)
}

#[test]
fn stm32f401re_boot_map_holds_one_extension_with_two_windows() {
    let reg = registry(
        STM32,
        &[],
        &board_for("stm32f401re", "t", &[("serial", "usart2"), ("bus", "i2c1")]),
    );
    let (def, board) = parts(&reg);
    let m = ext_manifest("logger", &["serial", "bus"]);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap();
    let (slot, map) = &plan.maps[0];
    assert_eq!(*slot, Binding::A);
    let k = kind::for_kind(def.isolation().kind).unwrap();
    assert_eq!(k.check(def, map), []);
    let r = &map.exts[0].regions;
    let shape: Vec<(u8, u32, u32, u8, Access, bool, String)> = r
        .iter()
        .map(|r| {
            (
                r.number,
                r.base,
                r.size,
                r.disabled,
                r.access,
                r.execute,
                r.purpose.to_string(),
            )
        })
        .collect();
    // Code at the top of the boot core body (below the trailer), data and
    // stack below the boot record, windows exactly the 1 KiB blocks.
    assert_eq!(
        shape,
        [
            (
                0,
                0x0800_6000,
                0x1000,
                0,
                Access::ReadOnly,
                true,
                "text".into()
            ),
            (
                1,
                0x2001_6800,
                0x800,
                0,
                Access::ReadWrite,
                false,
                "data".into()
            ),
            (
                2,
                0x2001_6000,
                0x800,
                1,
                Access::ReadWrite,
                false,
                "stack".into()
            ),
            (
                3,
                0x4000_4400,
                0x400,
                0,
                Access::ReadWrite,
                false,
                "window usart2".into()
            ),
            (
                4,
                0x4000_5400,
                0x400,
                0,
                Access::ReadWrite,
                false,
                "window i2c1".into()
            ),
        ]
    );
    assert_eq!(map.image_flash, (0x0800_0000, 0x6000));
    assert_eq!(map.image_ram, (0x2000_0000, 0x1_6000));
    // The app image gets its own map per slot.
    let app = node::plan(
        def,
        Some(board),
        Image::App { relay: false },
        vec![ext(&m)],
        &known(),
    )
    .unwrap();
    assert_eq!(app.maps.len(), 2);
    for (_, map) in &app.maps {
        assert_eq!(k.check(def, map), []);
    }
}

#[test]
fn rp2040_boot_map_uses_256_byte_regions_and_16_kib_windows() {
    let reg = registry(
        RP2040,
        &[],
        &board_for("rp2040", "t", &[("serial", "uart1"), ("pio", "pio1")]),
    );
    let (def, board) = parts(&reg);
    let m = ext_manifest("display", &["serial", "pio"]);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap();
    assert_eq!(plan.maps.len(), 2, "boot core slots A and B");
    let k = kind::for_kind(def.isolation().kind).unwrap();
    for (_, map) in &plan.maps {
        assert_eq!(k.check(def, map), []);
        let r = &map.exts[0].regions;
        assert!(r.iter().all(|r| r.size >= 256 && r.base % r.size == 0));
        let windows: Vec<_> = r
            .iter()
            .filter(|r| matches!(r.purpose, Purpose::Window(_)))
            .map(|r| (r.base, r.size))
            .collect();
        assert_eq!(windows, [(0x4003_8000, 0x4000), (0x5030_0000, 0x4000)]);
        // RAM regions sit below SRAM4, where the boot record is.
        assert!(r[1].end() <= 0x2004_0000 && r[2].end() <= 0x2004_0000);
    }
    // A privileged block is never a window.
    let reg = registry(RP2040, &[], &board_for("rp2040", "t", &[("dma", "dma")]));
    let (def, board) = parts(&reg);
    let m = ext_manifest("bad", &["dma"]);
    let e = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap_err();
    assert!(e.contains("not window-eligible"), "{e}");
}

#[test]
fn esp32s3_runs_extensions_unguarded_and_has_no_boot_stage() {
    let reg = registry(
        ESP32S3,
        &[("partitions.csv", PARTITIONS)],
        &board_for("esp32s3", "t", &[("can0", "twai")]),
    );
    let (def, board) = parts(&reg);
    let m = ext_manifest("uds", &["can0"]);
    let plan = node::plan(
        def,
        Some(board),
        Image::App { relay: false },
        vec![ext(&m)],
        &known(),
    )
    .unwrap();
    assert!(plan.maps.is_empty());
    assert!(plan.notes[0].contains("unguarded"), "{:?}", plan.notes);
    let e = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap_err();
    assert!(e.contains("no DAGON boot stage"), "{e}");
}

/// A synthetic MCU of `kind` with `regions` and `windows` eligible 1 KiB
/// blocks; `min` the smallest region.
fn synthetic(kind: &str, regions: u32, min: u32, windows: u32, extra: &str) -> String {
    let (cs, unwind, timer, caps) = match kind {
        "armv6m-mpu" => (
            "primask",
            "cortex-m-exception",
            "systick",
            format!("mpu_regions = {regions}\nmin_region = {min}\nsubregions_from = 256"),
        ),
        "armv7m-mpu" => (
            "basepri",
            "cortex-m-exception",
            "systick",
            format!("mpu_regions = {regions}\nmin_region = {min}\nsubregions_from = 256"),
        ),
        _ => (
            "mstatus-mie",
            "riscv-trap",
            "riscv-mtime",
            format!("pmp_entries = {regions}\ngrain = {min}"),
        ),
    };
    let mut t = format!(
        r#"id = "synth"
name = "SYNTH"
wire_id = 0x7F02
port = "self-boot"
ram = {{ at = 0x2000_0000, size = 0x2_0000 }}

[[flash_map]]
name = "synth-1m"
base = 0x0800_0000
bytes = 0x10_0000
xip = true

[[slot]]
image = "bootcore"
binding = "a"
header = "trailer"
at = 0x0800_0000
size = 0x2_0000

[[slot]]
image = "app"
binding = "a"
header = "inline"
at = 0x0802_0000
size = 0x4_0000

[writer]
kind = "probe-rs"
chip = "SYNTH"

[connection]
label = "SWD"
hint = "SWD"

[watchdog]
kind = "iwdg"
trial_ms = 4_000
confirmed_ms = 4_000

[isolation]
level = "mpu"
kind = "{kind}"
boot_extensions = true
ext_ram_max = 65_536

[isolation.capabilities]
{caps}
critical_section = "{cs}"
reset_cleanup = "peripheral-reset"
background_privileged = true
unwind = "{unwind}"
timer = "{timer}"
fault_address = true

[[core]]
id = "cpu0"

[[peripheral]]
id = "privileged"
base = 0x4000_0000
size = 0x1000
privileged_why = ["flash", "watchdog", "clock", "reset", "irq-routing", "protection"]
{extra}"#
    );
    for i in 0..windows {
        t.push_str(&format!(
            "\n[[peripheral]]\nid = \"w{i}\"\nbase = {:#x}\nsize = 0x400\nwindow_eligible = true\n",
            0x4001_0000 + i * 0x1000
        ));
    }
    t
}

fn synthetic_plan(
    kind: &str,
    regions: u32,
    min: u32,
    windows: u32,
    extra: &str,
    want: usize,
) -> Result<usize, String> {
    let names: Vec<(String, String)> = (0..want)
        .map(|i| (format!("b{i}"), format!("w{i}")))
        .collect();
    let pairs: Vec<(&str, &str)> = names
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let reg = registry(
        &synthetic(kind, regions, min, windows, extra),
        &[],
        &board_for("synth", "t", &pairs),
    );
    let (def, board) = parts(&reg);
    let ids: Vec<&str> = names.iter().map(|(a, _)| a.as_str()).collect();
    let m = ext_manifest("many", &ids);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known())?;
    let k = kind::for_kind(def.isolation().kind).unwrap();
    for (_, map) in &plan.maps {
        assert_eq!(k.check(def, map), []);
    }
    Ok(plan.maps[0].1.exts[0].regions.len())
}

#[test]
fn region_budgets_follow_the_capability_record() {
    // Eight regions: five windows fit after code, data and stack.
    assert_eq!(synthetic_plan("armv6m-mpu", 8, 256, 6, "", 5).unwrap(), 8);
    let e = synthetic_plan("armv6m-mpu", 8, 256, 6, "", 6).unwrap_err();
    assert_eq!(
        e,
        "extension many needs 6 windows; 5 fit after code, data and stack on an 8-region armv6m-mpu"
    );
    // A 16-region ARMv7-M unit (Cortex-M7) holds thirteen.
    assert_eq!(
        synthetic_plan("armv7m-mpu", 16, 32, 13, "", 13).unwrap(),
        16
    );
    assert!(synthetic_plan("armv7m-mpu", 8, 32, 13, "", 6).is_err());
    // A kind with no generator is refused by name, not mapped.
    let e = synthetic_plan("riscv-pmp", 16, 8, 1, "", 1).unwrap_err();
    assert!(
        e.contains("kind riscv-pmp, which has no map generator"),
        "{e}"
    );
}

#[test]
fn a_window_never_opens_a_neighbouring_block() {
    // A 1 KiB block whose 256-byte-minimum region would be fine, but on a
    // unit whose smallest region is 32 KiB the window also covers the
    // privileged block beside it, inside the same subregion.
    let extra = "\n[[peripheral]]\nid = \"rcc\"\nbase = 0x4001_0400\nsize = 0x400\nprivileged_why = \"clock\"\n";
    assert!(synthetic_plan("armv6m-mpu", 8, 256, 1, extra, 1).is_ok());
    let e = synthetic_plan("armv7m-mpu", 8, 32768, 1, extra, 1).unwrap_err();
    assert!(e.contains("a window for w0 would also open rcc"), "{e}");
}

#[test]
fn check_finds_what_a_bad_map_breaks() {
    let reg = registry(
        STM32,
        &[],
        &board_for("stm32f401re", "t", &[("serial", "usart2")]),
    );
    let (def, board) = parts(&reg);
    let m = ext_manifest("logger", &["serial"]);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap();
    let k = kind::for_kind(def.isolation().kind).unwrap();
    let good = plan.maps[0].1.clone();
    let broken = |f: &dyn Fn(&mut kind::Map)| {
        let mut m = good.clone();
        f(&mut m);
        k.check(def, &m)
            .into_iter()
            .map(|v| v.0)
            .collect::<Vec<_>>()
            .join("; ")
    };
    let e = broken(&|m| m.exts[0].regions[0].base += 0x20);
    assert!(e.contains("not aligned"), "{e}");
    let e = broken(&|m| m.exts[0].regions[0].access = Access::ReadWrite);
    assert!(e.contains("text must be ro"), "{e}");
    let e = broken(&|m| {
        let w = &mut m.exts[0].regions[3];
        w.base = 0x4002_3C00; // the flash interface
    });
    assert!(e.contains("opens peripheral flash"), "{e}");
    let e = broken(&|m| m.exts[0].regions[1].base = 0x2001_7000);
    assert!(e.contains("overlaps the boot record"), "{e}");
    let e = broken(&|m| m.exts[0].regions.retain(|r| r.purpose != Purpose::Stack));
    assert!(e.contains("exactly one stack"), "{e}");
}

#[test]
fn a_node_set_is_checked_as_a_whole() {
    let reg = registry(
        STM32,
        &[],
        &board_for("stm32f401re", "t", &[("serial", "usart2")]),
    );
    let (def, board) = parts(&reg);
    let a = manifest(&UDS.replace("peripherals = [\"can0\"]", "peripherals = []")).unwrap();
    let b = manifest(
        &UDS.replace("uds-transport", "other")
            .replace("peripherals = [\"can0\"]", "peripherals = []")
            .replace(", \"UpdateProtocol\"", ""),
    )
    .unwrap();
    let e = node::plan(
        def,
        Some(board),
        Image::Bootcore,
        vec![ext(&a), ext(&b)],
        &known(),
    )
    .unwrap_err();
    assert_eq!(
        e,
        "port UpdateTransport is provided by both uds-transport and other"
    );
    let e = node::plan(
        def,
        Some(board),
        Image::Bootcore,
        vec![ext(&a), ext(&a)],
        &known(),
    )
    .unwrap_err();
    assert!(e.contains("listed twice"), "{e}");
    // Capabilities name the board's tables.
    let e = node::plan(
        def,
        Some(board),
        Image::Bootcore,
        vec![ext(&manifest(UDS).unwrap())],
        &known(),
    )
    .unwrap_err();
    assert!(
        e.contains("peripheral can0 is not in board t's [[peripheral]]"),
        "{e}"
    );
    // A trusted extension needs its accepted hash.
    let v = manifest("format = 1\nid = \"hsm\"\nlayers = [\"boot\"]\nprovides = [\"ImageVerifier\"]\ntrusted = true\n").unwrap();
    let e = node::plan(def, Some(board), Image::Bootcore, vec![ext(&v)], &known()).unwrap_err();
    assert!(e.contains("artifact SHA-256"), "{e}");
    let ok = Ext {
        sha256: Some("ab".repeat(32)),
        ..ext(&v)
    };
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ok], &known()).unwrap();
    assert!(
        plan.maps.is_empty(),
        "a trusted extension runs in DAGON's regions"
    );
    // The boot budget: 1/4 of the trial period over every boot call (three
    // boot events and two ports: five calls of 50 ms).
    let slow = manifest(
        &UDS.replace("peripherals = [\"can0\"]", "peripherals = []")
            .replace("max_run_us = 2000", "max_run_us = 50000"),
    )
    .unwrap();
    let mut short = def.clone();
    short.watchdog.as_mut().unwrap().trial_ms = 400;
    let e = node::plan(
        &short,
        Some(board),
        Image::Bootcore,
        vec![ext(&slow)],
        &known(),
    )
    .unwrap_err();
    assert!(
        e.contains(
            "may take 250000 us per boot; 1/4 of stm32f401re's 400 ms trial watchdog is 100000 us"
        ),
        "{e}"
    );
    // Code that would take the whole boot core body is refused.
    let big = manifest(
        &UDS.replace("peripherals = [\"can0\"]", "peripherals = []")
            .replace("text_bytes = 16384", "text_bytes = 32768"),
    )
    .unwrap();
    let e = node::plan(def, Some(board), Image::Bootcore, vec![ext(&big)], &known()).unwrap_err();
    assert!(e.contains("bytes of flash"), "{e}");
    // An app-only extension is not for the boot core.
    let app = manifest(
        &UDS.replace("[\"boot\", \"app\"]", "[\"app\"]")
            .replace("peripherals = [\"can0\"]", "peripherals = []"),
    )
    .unwrap();
    let e = node::plan(def, Some(board), Image::Bootcore, vec![ext(&app)], &known()).unwrap_err();
    assert!(e.contains("not for the boot image"), "{e}");
}

pub(crate) mod fake_ext {
    pub type DagonExtension = dagon_events::Nop;
    pub const DAGON_EXTENSION: DagonExtension = dagon_events::Nop;
}

const BOOT_BOARD: &str = r#"
[boot]
led = "GPIO2"
led_active = "low"
halt = { pattern = "blink", period_ms = 100 }
power_hold = { pin = "GPIO5", level = "high" }

[[boot.action]]
on = "watchdog_armed"
ops = [{ op = "gpio", pin = "GPIO5", level = "high" }, { op = "delay", ms = 2 }, { op = "led", state = "on" }]
"#;

fn golden_wiring() -> String {
    let board = board_for("rp2040", "t", &[("serial", "uart1")]) + BOOT_BOARD;
    let reg = registry(RP2040, &[], &board);
    let (def, board) = parts(&reg);
    let m = ext_manifest("display", &["serial"]);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap();
    gen::boot_wiring("hub", def, Some(board), &plan, &known()).unwrap()
}

/// The generated file is Rust that compiles (this module includes the
/// checked-in copy) and the generator still writes exactly that copy.
#[rustfmt::skip]
#[path = "../tests/golden/boot_wiring.rs"]
mod golden;

#[test]
fn boot_wiring_is_the_golden_copy_and_compiles() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/boot_wiring.rs");
    let text = golden_wiring();
    if std::env::var_os("DAGON_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(text, want, "rerun with DAGON_BLESS=1 to accept");
    assert_eq!(golden::PROGRAM.len(), 1);
    assert_eq!(golden::MAPS.len(), 2);
    assert_eq!(golden::MAPS[0].maps[0].0, "display");
    assert_eq!(golden::READ_BACK[0].expect, 8);
    let regions = golden::MAPS[0].maps[0].1;
    assert!(regions.iter().all(|r| r.device == (r.number >= 3)));
    assert!(!golden::LED.unwrap().active_high);
    let _chain: golden::BootChain = golden::BOOT_CHAIN;
}

#[test]
fn programs_run_only_on_action_capable_boot_events() {
    let board = board_for("rp2040", "t", &[]) + &BOOT_BOARD.replace("watchdog_armed", "entry");
    let reg = registry(RP2040, &[], &board);
    let (def, board) = parts(&reg);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![], &known()).unwrap();
    let e = gen::boot_wiring("hub", def, Some(board), &plan, &known()).unwrap_err();
    assert!(e.contains("no action-capable boot event"), "{e}");
}

#[test]
fn linker_fragments_and_the_readable_map_follow_the_plan() {
    let reg = registry(
        STM32,
        &[],
        &board_for("stm32f401re", "t", &[("serial", "usart2")]),
    );
    let (def, board) = parts(&reg);
    let m = ext_manifest("logger", &["serial"]);
    let plan = node::plan(def, Some(board), Image::Bootcore, vec![ext(&m)], &known()).unwrap();
    let (b, space) = plan.memory[0];
    let x = gen::linker_fragment("n1", def, &plan, b, &space);
    assert!(
        x.contains("FLASH : ORIGIN = 0x08000000, LENGTH = 0x6000"),
        "{x}"
    );
    assert!(
        x.contains("RAM : ORIGIN = 0x20000000, LENGTH = 0x16000"),
        "{x}"
    );
    assert!(
        x.contains("EXT0_TEXT : ORIGIN = 0x08006000, LENGTH = 0x1000"),
        "{x}"
    );
    assert!(
        x.contains("BOOT_RECORD : ORIGIN = 0x20017000, LENGTH = 0x800"),
        "{x}"
    );
    assert!(x.contains(".ext.logger.text"), "{x}");
    // Without extensions the image keeps everything below the record.
    let bare = node::plan(
        def,
        Some(board),
        Image::App { relay: false },
        vec![],
        &known(),
    )
    .unwrap();
    let (b, space) = bare.memory[0];
    assert_eq!(
        space,
        Space {
            flash_at: 0x0802_0200,
            flash_size: 0x1_FE00,
            ram_at: 0x2000_0000,
            ram_size: 0x1_7000
        }
    );
    assert!(!gen::linker_fragment("n1", def, &bare, b, &space).contains("SECTIONS"));
    let t = gen::protection_map("n1", def, &[plan, bare]);
    let v: toml::Table = t.parse().unwrap();
    assert_eq!(v["kind"].as_str(), Some("armv7m-mpu"));
    assert_eq!(v["image"].as_array().unwrap().len(), 2);
}

/// A synthetic cached Cortex-M7 (16-region ARMv7-M MPU) with `isolation`
/// lines added, planned with one window.
fn cortex_m7(isolation: &str) -> Result<node::ImagePlan<'static>, String> {
    let text = synthetic("armv7m-mpu", 16, 32, 1, "").replace(
        "ext_ram_max = 65_536\n",
        &format!("ext_ram_max = 65_536\n{isolation}"),
    );
    let reg = Box::leak(Box::new(registry(
        &text,
        &[],
        &board_for("synth", "t", &[("b0", "w0")]),
    )));
    let (def, board) = parts(reg);
    let m = Box::leak(Box::new(ext_manifest("cached", &["b0"])));
    node::plan(def, Some(board), Image::Bootcore, vec![ext(m)], &known())
}

#[test]
fn caches_and_tcm_are_honoured_or_refused_by_field() {
    // An instruction cache: windows are device memory, the rest normal.
    let plan = cortex_m7("cache = \"i-cache\"\n").unwrap();
    let regions = &plan.maps[0].1.exts[0].regions;
    for r in regions {
        let want = if matches!(r.purpose, Purpose::Window(_)) {
            Memory::Device
        } else {
            Memory::Normal
        };
        assert_eq!(r.memory, want, "{}", r.purpose);
    }
    // A data cache needs non-cacheable bounce buffers; TCM needs placement.
    let e = cortex_m7("cache = \"id-cache\"\n").unwrap_err();
    assert!(e.contains("isolation.cache = \"id-cache\""), "{e}");
    let e = cortex_m7("tcm = [{ at = 0x2000_0000, size = 0x1_0000 }]\n").unwrap_err();
    assert!(e.contains("isolation.tcm"), "{e}");
}

#[test]
fn check_holds_windows_to_device_memory_away_from_memory() {
    let plan = cortex_m7("").unwrap();
    let reg = registry(
        &synthetic("armv7m-mpu", 16, 32, 1, ""),
        &[],
        &board_for("synth", "t", &[("b0", "w0")]),
    );
    let (def, _) = parts(&reg);
    let k = kind::for_kind(def.isolation().kind).unwrap();
    let good = plan.maps[0].1.clone();
    assert_eq!(k.check(def, &good), []);
    let broken = |f: &dyn Fn(&mut kind::Region)| {
        let mut m = good.clone();
        f(m.exts[0].regions.last_mut().unwrap());
        k.check(def, &m)
            .into_iter()
            .map(|v| v.0)
            .collect::<Vec<_>>()
            .join("; ")
    };
    let e = broken(&|r| r.memory = Memory::Normal);
    assert!(e.contains("must be device memory"), "{e}");
    let e = broken(&|r| {
        r.base = 0x2000_0000;
        r.size = 0x400;
    });
    assert!(e.contains("window reaches RAM"), "{e}");
    let e = broken(&|r| {
        r.base = 0x0802_0000;
        r.size = 0x400;
    });
    assert!(e.contains("window reaches flash"), "{e}");
}
