//! A project's overrides of built-in definitions, loaded: per row of the
//! merge-key table on real and synthetic definitions, `dagon-order`,
//! `dagon-remove`, `dagon-unset`, every refusal, the merged text against
//! the parsed definition, and dagon.toml's format and keys.

use std::path::{Path, PathBuf};

use super::load::parse_mcu_text;
use super::tests::{FAKE_BOARD, FAKE_MCU};
use super::*;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dagon-merge-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The built-in definitions (or the source tree `tree`) with a dagon.toml of
/// `body`, written as is.
fn load_raw(name: &str, tree: Option<&Path>, body: &str) -> Result<Registry, DefError> {
    let dir = scratch(name);
    write(&dir.join("dagon.toml"), body);
    let toml = dir.join("dagon.toml");
    let mut sources = Vec::new();
    if let Some(t) = tree {
        sources.push(Source::Tree(t));
    }
    sources.push(Source::Project(&toml));
    let r = Registry::load(&sources);
    std::fs::remove_dir_all(dir).unwrap();
    r
}

/// [`load_raw`] of a format 1 dagon.toml holding `overrides`.
fn load(name: &str, overrides: &str) -> Result<Registry, DefError> {
    load_raw(name, None, &format!("format = 1\n{overrides}"))
}

fn refused(name: &str, overrides: &str, says: &str) {
    let e = load(name, overrides).expect_err(name).to_string();
    assert!(e.contains(says), "{name}: {e}");
    assert!(
        e.contains("dagon.toml") && e.contains("_override."),
        "{name}: the error names the override: {e}"
    );
}

fn builtin_mcu(id: &str) -> McuDef {
    Registry::builtin().mcu(id).unwrap().clone()
}

/// A built-in MCU as a project's text: what an override leaves untouched.
fn as_project(mut m: McuDef) -> McuDef {
    super::project_text::mcu(&mut m).unwrap();
    m
}

#[test]
fn a_project_without_overrides_loads_the_builtin_definitions_unchanged() {
    let r = load("none", "topology = \"t.yaml\"\n").unwrap();
    assert_eq!(&r, Registry::builtin());
}

#[test]
fn a_keyed_slot_row_changes_part_of_an_entry_adds_one_and_removes_one() {
    let r = load(
        "slots",
        r#"
# A table: only the kind given changes; app and bootcore keep theirs.
[mcu_override.rp2040.writers]
agent = ["app"]

[[mcu_override.rp2040.slot]]
image = "app"
binding = "b"
size = 0x8_0000

[[mcu_override.rp2040.slot]]
image = "bootcore"
binding = "b"
dagon-remove = true

[[mcu_override.rp2040.slot]]
image = "agent"
binding = "any"
header = "inline"
at = 0x101C_8000
size = 0x2_0000

[[mcu_override.rp2040.slot_layout]]
target = "app"
slot = "b"
capacity = 0x7_FE00

[[mcu_override.rp2040.slot_layout]]
target = "bootcore"
slot = "b"
dagon-remove = true

[[mcu_override.rp2040.slot_layout]]
target = "agent"
slot = "factory"
start = 0x101C_8000
capacity = 0x1_FE00
header = "inline"
"#,
    )
    .unwrap();
    let m = r.mcu("rp2040").unwrap();
    let mut want = as_project(builtin_mcu("rp2040"));
    let app_b = want
        .slot_defs
        .iter_mut()
        .find(|s| s.image == ImageKind::App && s.binding == Binding::B)
        .unwrap();
    app_b.size = Some(0x8_0000);
    want.slot_defs
        .retain(|s| !(s.image == ImageKind::Bootcore && s.binding == Binding::B));
    want.slot_defs.push(SlotDef {
        image: ImageKind::Agent,
        binding: Binding::Any,
        header: Placement::Inline,
        at: Some(0x101C_8000),
        size: Some(0x2_0000),
        partition: None,
        header_at: None,
        header_partition: None,
        header_offset: 0,
    });
    let lay = want
        .slot_layouts
        .iter_mut()
        .find(|l| l.target == TargetName::App && l.slot == SlotName::B)
        .unwrap();
    lay.capacity = 0x7_FE00;
    want.slot_layouts
        .retain(|l| !(l.target == TargetName::Bootcore && l.slot == SlotName::B));
    want.slot_layouts.push(SlotLayoutDef {
        target: TargetName::Agent,
        slot: SlotName::Factory,
        start: 0x101C_8000,
        capacity: 0x1_FE00,
        header: HeaderDef::Inline(slot_layout::InlineWord::Inline),
        partition: None,
    });
    want.writers.insert(ImageKind::Agent, vec![ImageKind::App]);
    want.slots = m.slots.clone();
    assert_eq!(m, &want);
    let b = r.slot("rp2040", image_kind::APP, slot_binding::B).unwrap();
    assert_eq!((b.start, b.size), (0x1014_8000, 0x8_0000));
    assert!(r
        .slot("rp2040", image_kind::BOOTCORE, slot_binding::B)
        .is_none());
    assert_eq!(r.mcu_origin("rp2040"), Some(Origin::Overridden));
    // Its text is the project's (made safe, commands confirmed), its package the built-in one.
    assert!(Origin::Overridden.is_project_text());
    assert_eq!(m.package, None);
    // Every other MCU stays as built in.
    assert_eq!(r.mcu("stm32f401re"), Registry::builtin().mcu("stm32f401re"));
}

#[test]
fn dagon_order_gives_the_default_flash_map_and_dagons_core() {
    let r = load(
        "order",
        r#"
[mcu_override.rp2040]
dagon-order.flash_map = ["rp2040-16m", "rp2040-4m"]
dagon-order.core = ["core1", "core0"]

[[mcu_override.rp2040.flash_map]]
name = "rp2040-16m"
base = 0x1000_0000
bytes = 0x100_0000
xip = true
"#,
    )
    .unwrap();
    let m = r.mcu("rp2040").unwrap();
    assert_eq!(m.default_flash_map().name, "rp2040-16m");
    assert_eq!(m.flash_maps[1], builtin_mcu("rp2040").flash_maps[0]);
    let cores: Vec<&str> = m.cores.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(cores, ["core1", "core0"]);
    // Without an order, a new entry comes after the built-in ones.
    let r = load(
        "append",
        "[[mcu_override.rp2040.flash_map]]\nname = \"rp2040-16m\"\nbase = 0x1000_0000\nbytes = 0x100_0000\nxip = true\n",
    )
    .unwrap();
    assert_eq!(
        r.mcu("rp2040").unwrap().default_flash_map().name,
        "rp2040-4m"
    );
}

#[test]
fn a_dagon_order_that_misses_repeats_or_invents_a_key_is_refused() {
    for (name, order, says) in [
        ("order-miss", r#"["core1"]"#, "leaves out"),
        ("order-twice", r#"["core1", "core0", "core1"]"#, "twice"),
        ("order-new", r#"["core1", "core0", "core9"]"#, "core9"),
    ] {
        refused(
            name,
            &format!("[mcu_override.rp2040]\ndagon-order.core = {order}\n"),
            says,
        );
    }
    refused(
        "order-unordered",
        "[mcu_override.rp2040]\ndagon-order.slot = []\n",
        "dagon-order.slot",
    );
}

#[test]
fn add_or_delete_rows_take_whole_entries_and_refuse_a_partial_change() {
    let r = load(
        "cores",
        "[[mcu_override.rp2040.core]]\nid = \"core1\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.part]]\nname = \"stage0\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.part]]\nname = \"boot\"\nsub = \"bootcore\"\nbin = \"other\"\n",
    )
    .unwrap();
    let m = r.mcu("rp2040").unwrap();
    assert_eq!(m.cores, [Core { id: "core0".into() }]);
    let parts: Vec<(&str, &str)> = m
        .parts
        .iter()
        .map(|p| (p.name.as_str(), p.sub.as_str()))
        .collect();
    assert_eq!(parts, [("bootcore", "bootcore"), ("boot", "bootcore")]);
    refused(
        "core-partial",
        "[[mcu_override.rp2040.core]]\nid = \"core0\"\n",
        "added or deleted whole",
    );
    refused(
        "part-partial",
        "[[mcu_override.rp2040.part]]\nname = \"stage0\"\nbin = \"x\"\n",
        "added or deleted whole",
    );
    // A part names only a folder the built-in definition's parts name.
    refused(
        "part-folder",
        "[[mcu_override.rp2040.part]]\nname = \"extra\"\nsub = \"elsewhere\"\n",
        "part.sub",
    );
    // An existing part keeps its folder, even removed and added again:
    // swapping two parts' folders is refused, though each folder is the
    // definition's.
    refused(
        "part-swap",
        "[[mcu_override.rp2040.part]]\nname = \"stage0\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.part]]\nname = \"bootcore\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.part]]\nname = \"stage0\"\nsub = \"bootcore\"\n\n\
         [[mcu_override.rp2040.part]]\nname = \"bootcore\"\nsub = \"stage0\"\n",
        "part.sub",
    );
    // Added again with its own folder, a part loads.
    let r = load(
        "part-again",
        "[[mcu_override.rp2040.part]]\nname = \"stage0\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.part]]\nname = \"stage0\"\nsub = \"stage0\"\nbin = \"s0\"\n",
    )
    .unwrap();
    let m = r.mcu("rp2040").unwrap();
    let p = m.parts.iter().find(|p| p.name == "stage0").unwrap();
    assert_eq!((p.sub.as_str(), p.bin.as_deref()), ("stage0", Some("s0")));
}

#[test]
fn keyed_peripheral_rom_pair_and_erase_rows_merge_by_key() {
    let f401 = builtin_mcu("stm32f401re");
    let pair = f401.rom_i2c.as_ref().unwrap().pairs[0].clone();
    let with_dma = f401
        .peripherals
        .iter()
        .find(|p| p.dma_requests.len() > 1)
        .unwrap()
        .clone();
    let r = load(
        "keyed",
        &format!(
            "[[mcu_override.stm32f401re.rom_i2c.pairs]]\nsda = {:?}\nscl = {:?}\ncontroller = {:?}\n\n\
             [[mcu_override.stm32f401re.peripheral]]\nid = {:?}\nwindow_eligible = false\nprivileged_why = \"shared\"\n\
             dma_requests = [{{ signal = {:?}, controller = {:?}, stream = {}, channel = {} }}]\n\n\
             [[mcu_override.rp2040.execution.erase_regions]]\nstart = 0x1000_0000\nunit_max_ms = 400\n",
            pair.sda,
            pair.scl,
            pair.controller,
            with_dma.id,
            with_dma.dma_requests[1].signal,
            with_dma.dma_requests[1].controller,
            with_dma.dma_requests[1].stream,
            with_dma.dma_requests[1].channel,
        ),
    )
    .unwrap();
    let m = r.mcu("stm32f401re").unwrap();
    let p = m.peripherals.iter().find(|p| p.id == with_dma.id).unwrap();
    // dma_requests is replaced whole: its order is a priority.
    assert_eq!(p.dma_requests, [with_dma.dma_requests[1].clone()]);
    assert!(!p.window_eligible);
    assert_eq!((p.base, p.size), (with_dma.base, with_dma.size));
    assert_eq!(
        m.rom_i2c.as_ref().unwrap().pairs,
        f401.rom_i2c.unwrap().pairs
    );
    let rp = r.mcu("rp2040").unwrap();
    let er = &rp.execution.as_ref().unwrap().erase_regions[0];
    assert_eq!(er.unit_max_ms, Fact::Known(400));
    assert_eq!(
        (er.start, er.len, er.unit),
        (0x1000_0000, 0x40_0000, 0x1000)
    );
}

#[test]
fn link_ports_merge_by_name() {
    let r = load(
        "link-ports",
        "[[mcu_override.stm32f401re.link_port]]\nname = \"usart2\"\nindex = 9\n\n\
         [[mcu_override.stm32f401re.link_port]]\nname = \"usart1\"\ndagon-remove = true\n\n\
         [[mcu_override.stm32f401re.link_port]]\nname = \"uart-extra\"\nindex = 10\nkind = \"uart\"\n",
    )
    .unwrap();
    let base = builtin_mcu("stm32f401re").ports;
    let got = &r.mcu("stm32f401re").unwrap().ports;
    let mut want: Vec<PortDef> = base
        .iter()
        .filter(|p| p.name != "usart1")
        .cloned()
        .collect();
    want.iter_mut().find(|p| p.name == "usart2").unwrap().index = 9;
    want.push(PortDef {
        index: 10,
        name: "uart-extra".into(),
        kind: LinkKind::Uart,
    });
    assert_eq!(got, &want);
    // The loader's own rule still holds on the merged ports: an index once.
    let e = load(
        "link-port-index",
        "[[mcu_override.stm32f401re.link_port]]\nname = \"usart2\"\nindex = 0\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("mcu_override.stm32f401re"), "{e}");
}

/// `[[relay_link]]` merges by `port`: an entry changes its id or addressing,
/// one is removed, a new port is added; the loader's rules hold on the result.
#[test]
fn relay_links_merge_by_port() {
    let r = load(
        "relay-links",
        "[[mcu_override.rp2040.relay_link]]\nport = \"pio-uart\"\nid = 7\naddressing = { fixed = 0x10 }\n\n\
         [[mcu_override.rp2040.relay_link]]\nport = \"i2c0\"\ndagon-remove = true\n\n\
         [[mcu_override.rp2040.relay_link]]\nport = \"uart0\"\nid = 9\naddressing = { fixed = 0x11 }\nmax_payload = 128\n",
    )
    .unwrap();
    let got: Vec<_> = r
        .mcu("rp2040")
        .unwrap()
        .relay_link_defs()
        .iter()
        .map(|l| (l.id, l.port, l.addressing, l.max_payload))
        .collect();
    let rp = builtin_mcu("rp2040");
    let at = |name: &str| rp.port(name).unwrap().index;
    assert_eq!(
        got,
        [
            (7, at("pio-uart"), dagon_port::Addressing::Fixed(0x10), 1024),
            (3, at("i2c1"), dagon_port::Addressing::SevenBit, 256),
            (
                4,
                at("pio-uart1"),
                dagon_port::Addressing::Fixed(0xFF),
                1024
            ),
            (9, at("uart0"), dagon_port::Addressing::Fixed(0x11), 128),
        ]
    );
    // A merged entry still keeps the rules: its id once.
    let e = load(
        "relay-link-id",
        "[[mcu_override.rp2040.relay_link]]\nport = \"pio-uart\"\nid = 1\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("relay_link id 1 is used twice"), "{e}");
}

#[test]
fn a_tagged_table_is_replaced_whole_when_given() {
    let r = load(
        "writer",
        "[mcu_override.rp2040.writer]\nkind = \"probe-rs\"\nchip = \"RP2040\"\n\n\
         [mcu_override.rp2040.watchdog]\nflag = \"register\"\n",
    )
    .unwrap();
    let m = r.mcu("rp2040").unwrap();
    assert_eq!(
        m.writer,
        Writer::ProbeRs {
            chip: "RP2040".into()
        }
    );
    let w = m.watchdog.unwrap();
    assert_eq!(w.flag, Fact::Known(isolation::WatchdogFlag::Register));
    assert_eq!(w.trial_ms, builtin_mcu("rp2040").watchdog.unwrap().trial_ms);
}

#[test]
fn doctor_identity_and_file_fields_are_refused() {
    refused(
        "doctor",
        "[[mcu_override.rp2040.doctor]]\nwhat = \"x\"\nprobe = [\"x\"]\nexpect = \"runs\"\n",
        "doctor",
    );
    for (name, body, field) in [
        ("id", "[mcu_override.rp2040]\nid = \"rp2041\"\n", "`id`"),
        (
            "port",
            "[mcu_override.rp2040]\nport = \"vendor-stage2\"\n",
            "port",
        ),
        ("format", "[mcu_override.rp2040]\nformat = 1\n", "format"),
        (
            "partitions",
            "[mcu_override.esp32s3]\npartitions = \"other.csv\"\n",
            "partitions",
        ),
        (
            "bootloader",
            "[mcu_override.esp32s3.writer]\nkind = \"espflash\"\nchip = \"esp32s3\"\n\
             flash_size = \"16mb\"\ntable_at = 0x8000\nbootloader = \"mine.bin\"\n\
             checksums = \"vendor/SHA256SUMS\"\n",
            "writer.bootloader",
        ),
        (
            "unset-partitions",
            "[mcu_override.esp32s3]\ndagon-unset = [\"partitions\"]\n",
            "partitions",
        ),
        (
            "board-mcu",
            "[board_override.nucleo-f401re]\nmcu = \"rp2040\"\n",
            "mcu",
        ),
        (
            "board-id",
            "[board_override.nucleo-f401re]\nid = \"other\"\n",
            "`id`",
        ),
    ] {
        refused(name, body, field);
    }
}

#[test]
fn removal_and_unset_refuse_what_the_definition_does_not_have() {
    refused(
        "remove-missing",
        "[[mcu_override.rp2040.slot]]\nimage = \"agent\"\nbinding = \"a\"\ndagon-remove = true\n",
        "no entry",
    );
    refused(
        "remove-extra",
        "[[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"b\"\nsize = 1\ndagon-remove = true\n",
        "key fields only",
    );
    refused(
        "unset-missing",
        "[[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"b\"\ndagon-unset = [\"partition\"]\n",
        "partition",
    );
    refused(
        "unset-key",
        "[[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"b\"\ndagon-unset = [\"image\"]\n",
        "key",
    );
    refused(
        "remove-required-table",
        "[mcu_override.rp2040.connection]\ndagon-remove = true\n",
        "connection",
    );
    refused(
        "unset-required",
        "[mcu_override.rp2040.watchdog]\ndagon-unset = [\"trial_ms\"]\n",
        "trial_ms",
    );
    refused(
        "remove-absent-table",
        "[mcu_override.esp32s3.coordinator]\ndagon-remove = true\n",
        "coordinator",
    );
    // An optional table goes with dagon-remove.
    let r = load(
        "remove-table",
        "[mcu_override.rp2040.link_i2c]\ndagon-remove = true\n\n\
         [mcu_override.rp2040.links]\ndown = [\"uart\"]\n",
    )
    .unwrap();
    assert!(r.mcu("rp2040").unwrap().link_i2c.is_none());
    // A table the schema gives a default for goes back to that default.
    let r = load(
        "remove-default",
        "[mcu_override.esp32s3.links]
dagon-remove = true
",
    )
    .unwrap();
    let m = r.mcu("esp32s3").unwrap();
    assert_eq!(m.links, Links::default());
    assert_eq!(
        m.link_uart,
        builtin_mcu("esp32s3").link_uart,
        "the rest stays"
    );
}

#[test]
fn unset_moves_a_slot_from_a_partition_to_an_address() {
    let r = load(
        "unset-slot",
        "[[mcu_override.esp32s3.slot]]\nimage = \"app\"\nbinding = \"any\"\n\
         dagon-unset = [\"partition\"]\nat = 0x11_0000\nsize = 0x30_0000\n",
    )
    .unwrap();
    let s = r
        .mcu("esp32s3")
        .unwrap()
        .slot_defs
        .iter()
        .find(|s| s.image == ImageKind::App)
        .unwrap()
        .clone();
    assert_eq!(
        (s.at, s.size, s.partition),
        (Some(0x11_0000), Some(0x30_0000), None)
    );
}

#[test]
fn an_override_error_names_the_override_file_and_key() {
    refused(
        "unknown-field",
        "[mcu_override.rp2040]\nflash_mapz = 1\n",
        "[mcu_override.rp2040]",
    );
    refused(
        "unknown-id",
        "[mcu_override.no-such-mcu]\nname = \"x\"\n",
        "no-such-mcu",
    );
    refused(
        "whole-remove",
        "[mcu_override.rp2040]\ndagon-remove = true\n",
        "dagon-remove",
    );
    refused(
        "unknown-marker",
        "[mcu_override.rp2040]\ndagon-keep = true\n",
        "dagon-keep",
    );
}

#[test]
fn a_projects_own_definition_is_changed_in_its_own_file_not_overridden() {
    let e = load(
        "own",
        "[board.mine]\nformat = 4\nproduct = \"Mine\"\nmcu = \"rp2040\"\n\
         flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\n\
         [board_override.mine]\nproduct = \"x\"\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("own"), "{e}");
}

/// A DAGON source tree with one synthetic MCU and board no code names.
fn synthetic_tree(name: &str) -> PathBuf {
    let root = scratch(name);
    let mcu = root.join("targets/mcu/fake-m0");
    write(
        &mcu.join("dagon-mcu.toml"),
        &(FAKE_MCU.replace(
            "port = \"self-boot\"",
            "port = \"self-boot\"\npartitions = \"partitions.csv\"",
        ) + "\n[[peripheral]]\nid = \"i2c0\"\nbase = 0x4000_0000\nsize = 0x1000\n\
               window_eligible = true\n\n[[peripheral]]\nid = \"uart0\"\nbase = 0x4000_1000\n\
               size = 0x1000\nwindow_eligible = true\n"),
    );
    write(
        &mcu.join("partitions.csv"),
        "# Name, Type, SubType, Offset, Size\nota_1, app, ota_1, 0x150000, 0x40000\n",
    );
    std::fs::create_dir_all(mcu.join("bootcore")).unwrap();
    let board = root.join("targets/boards/fake-board");
    write(
        &board.join("dagon-board.toml"),
        &format!(
            "id = \"fake-board\"\n{FAKE_BOARD}\n\
             [[pin]]\npin = \"GPIO6\"\nname = \"led\"\nboot = true\n\n\
             [[bus]]\nid = \"sys\"\nkind = \"i2c\"\ncontroller = \"i2c0\"\nsda = \"GPIO4\"\n\
             scl = \"GPIO5\"\nhz = 100_000\n\n\
             [[peripheral]]\nid = \"expander\"\nmcu = \"uart0\"\n\n\
             [[device]]\nid = \"ioe\"\nbus = \"sys\"\naddr = 0x20\nmax_transfer = 2\n\
             regs = [{{ name = \"out\", reg = 1 }}, {{ name = \"dir\", reg = 3 }}]\nwritable = [\"out\"]\n"
        ),
    );
    root
}

#[test]
fn a_synthetic_mcu_and_board_are_overridden_the_same_way() {
    let root = synthetic_tree("synthetic");
    let r = load_raw(
        "synthetic-p",
        Some(&root),
        "format = 1\n\
         [[mcu_override.fake-m0.slot]]\nimage = \"app\"\nbinding = \"b\"\n\
         dagon-unset = [\"at\", \"size\"]\npartition = \"ota_1\"\n\n\
         [[board_override.fake-board.bus]]\nid = \"sys\"\nkind = \"uart\"\n\
         dagon-unset = [\"hz\", \"sda\", \"scl\"]\ntx = \"GPIO4\"\nrx = \"GPIO5\"\nbaud = 115_200\n\n\
         [[board_override.fake-board.device]]\nid = \"ioe\"\nbus = \"sys\"\n\
         regs = [{ name = \"dir\", reg = 4 }]\n\n\
         [[board_override.fake-board.pin]]\npin = \"GPIO7\"\nname = \"button\"\n\n\
         [[board_override.fake-board.peripheral]]\nid = \"expander\"\ndagon-remove = true\n\n\
         [[board_override.fake-board.header]]\npin = \"GPIO2\"\nlabel = \"D9\"\n\n\
         [[board_override.fake-board.i2c]]\nsda = \"GPIO2\"\nscl = \"GPIO3\"\ncontroller = \"i2c0\"\n",
    );
    let r = r.unwrap();
    let app_b = r.slot("fake-m0", image_kind::APP, slot_binding::B).unwrap();
    assert_eq!((app_b.start, app_b.size), (0x15_0000, 0x4_0000));
    let b = r.board("fake-board").unwrap();
    let bus = b.buses.iter().find(|x| x.id == "sys").unwrap();
    assert_eq!(
        (bus.hz, bus.baud, bus.sda.as_deref(), bus.tx.as_deref()),
        (None, Some(115_200), None, Some("GPIO4"))
    );
    let dev = &b.devices[0];
    // regs merge by name: `dir` changes, `out` stays.
    let regs: Vec<(&str, u8)> = dev.regs.iter().map(|g| (g.name.as_str(), g.reg)).collect();
    assert_eq!(regs, [("out", 1), ("dir", 4)]);
    let pins: Vec<&str> = b.pins.iter().map(|p| p.pin.as_str()).collect();
    assert_eq!(pins, ["GPIO6", "GPIO7"]);
    assert!(b.peripherals.is_empty());
    assert_eq!(b.header[0].label.as_deref(), Some("D9"));
    assert_eq!(r.board_origin("fake-board"), Some(Origin::Overridden));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn board_flash_merges_as_a_table_and_a_new_part_brings_its_whole_table() {
    let flash = "[board_override.m5stopwatch.flash]\npart = \"W25Q128JV\"\n\
                 erase_regions = [{ start = 0x0, len = 0x100_0000, unit = 0x1000, unit_max_ms = 400 }]\n";
    let r = load(
        "flash",
        &format!(
            "{flash}\n[[mcu_override.esp32s3.execution.erase_regions]]\nstart = 0x0\nunit_max_ms = 450\n"
        ),
    )
    .unwrap();
    let m = r.mcu("esp32s3").unwrap();
    let b = r.board("m5stopwatch").unwrap();
    // Each definition merges on its own; the board's regions then replace the MCU's.
    assert_eq!(
        m.erase_regions_on(None).unwrap()[0].unit_max_ms,
        Fact::Known(450)
    );
    assert_eq!(
        m.erase_regions_on(Some(b)).unwrap()[0].unit_max_ms,
        Fact::Known(400)
    );
    // A board that has [flash] (here: an inline board of the project, which
    // the override cannot reach, so a tree board) changes its part only with
    // the whole table.
    let root = synthetic_tree("flash-tree");
    let board = root.join("targets/boards/fake-board/dagon-board.toml");
    let text = std::fs::read_to_string(&board).unwrap();
    std::fs::write(
        &board,
        text + "\n[flash]\npart = \"A\"\nerase_regions = [{ start = 0x10_0000, len = 0x10_0000, unit = 0x1000, unit_max_ms = 300 }]\n",
    )
    .unwrap();
    let e = load_raw(
        "flash-part",
        Some(&root),
        "format = 1\n[board_override.fake-board.flash]\npart = \"B\"\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("flash") && e.contains("board_override.fake-board"),
        "{e}"
    );
    let r = load_raw(
        "flash-same",
        Some(&root),
        "format = 1\n[board_override.fake-board.flash]\n\
         erase_regions = [{ start = 0x10_0000, len = 0x10_0000, unit = 0x1000, unit_max_ms = 350 }]\n",
    )
    .unwrap();
    let f = r.board("fake-board").unwrap().flash.as_ref().unwrap();
    assert_eq!(
        (f.part.as_str(), f.erase_regions[0].unit_max_ms),
        ("A", Fact::Known(350))
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn open_facts_are_the_execution_facts_not_checked_on_the_nodes_flash() {
    let mcu = "[mcu_override.esp32s3.execution]\nmasked_max_us = \"unmeasured\"\n\
               [[mcu_override.esp32s3.execution.erase_regions]]\nstart = 0x0\n\
               unit_max_ms = { unverified = 450 }\n";
    let r = load("open", mcu).unwrap();
    let m = r.mcu("esp32s3").unwrap();
    let open = |key: &str, value| OpenFact {
        key: key.into(),
        value,
    };
    assert_eq!(
        m.open_facts_on(None),
        [
            open("execution.masked_max_us", None),
            open("execution.erase_regions[0].unit_max_ms", Some(450)),
        ]
    );
    // A board with its own [flash] replaces the MCU's erase times.
    let flash = "[board_override.m5stopwatch.flash]\npart = \"W25Q128JV\"\n\
                 erase_regions = [{ start = 0x0, len = 0x100_0000, unit = 0x1000, unit_max_ms = 400 }]\n";
    let r = load("open-board", &format!("{mcu}{flash}")).unwrap();
    let m = r.mcu("esp32s3").unwrap();
    let b = r.board("m5stopwatch");
    assert_eq!(m.open_facts_on(b), [open("execution.masked_max_us", None)]);
    let r = load(
        "open-none",
        "[mcu_override.esp32s3.execution]\nmasked_max_us = 20\n\
         [[mcu_override.esp32s3.execution.erase_regions]]\nstart = 0x0\nunit_max_ms = 450\n",
    )
    .unwrap();
    assert_eq!(r.mcu("esp32s3").unwrap().open_facts_on(None), []);
    assert_eq!(
        open("k", None).to_string() + " / " + &open("k", Some(5)).to_string(),
        "k is unmeasured / k = 5 is not checked against its source"
    );
}

#[test]
fn the_merged_text_is_what_the_registry_stores_parses_and_sends() {
    let r = load(
        "text",
        "[[mcu_override.esp32s3.slot]]\nimage = \"agent\"\nbinding = \"any\"\nheader_offset = 0x3000\n\n\
         [[mcu_override.esp32s3.slot_layout]]\ntarget = \"agent\"\nslot = \"factory\"\n\
         header = { detached = 0x71_3000 }\n",
    )
    .unwrap();
    let i = r.mcus.iter().position(|m| m.id == "esp32s3").unwrap();
    let src = &r.mcu_sources[i];
    let files: Vec<(&str, &str)> = src
        .files
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let parsed = as_project(parse_mcu_text(&src.toml, &files).unwrap());
    assert_eq!(&parsed, r.mcu("esp32s3").unwrap());
    assert!(!src.toml.contains("dagon-"));
    // The Console gets the same definition.
    let back = Registry::from_json(&r.to_json()).unwrap();
    assert_eq!(
        back.mcu("esp32s3").unwrap().slot_defs,
        r.mcu("esp32s3").unwrap().slot_defs
    );
    // Labelled as overridden, it is made safe again when read back.
    assert!(r.to_json().contains("overridden MCU esp32s3"));
    assert_eq!(back.mcu("esp32s3").unwrap().connection.hint, PROJECT_TEXT);
}

/// The text a node's firmware builds from (`Registry::merged_mcu_toml`)
/// is the merged definition: it parses back to the same definition, with
/// the overridden slot; for a definition no override names it is the
/// built-in one.
#[test]
fn the_text_the_firmware_builds_from_is_the_merged_definition() {
    let r = load(
        "firmware-text",
        "[[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"b\"\nsize = 0x8_0000\n\n\
         [[mcu_override.rp2040.slot_layout]]\ntarget = \"app\"\nslot = \"b\"\ncapacity = 0x7_FE00\n",
    )
    .unwrap();
    let text = r.merged_mcu_toml("rp2040").unwrap();
    let parsed = as_project(parse_mcu_text(&text, &[]).unwrap());
    assert_eq!(&parsed, r.mcu("rp2040").unwrap());
    let t: toml::Table = text.parse().unwrap();
    let app_b = t["slot"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["image"].as_str() == Some("app") && s["binding"].as_str() == Some("b"))
        .unwrap();
    assert_eq!(app_b["size"].as_integer(), Some(0x8_0000));
    let builtin = Registry::builtin();
    let text = builtin.merged_mcu_toml("stm32f401re").unwrap();
    assert_eq!(
        &parse_mcu_text(&text, &[]).unwrap(),
        builtin.mcu("stm32f401re").unwrap()
    );
    assert_eq!(builtin.merged_mcu_toml("no-such-mcu"), None);
}

/// A node's board as its firmware builds from it
/// (`Registry::merged_board_toml`) is the merged board in the canonical
/// form (keys sorted, LF line endings, one trailing newline), as the MCU's
/// text is; a board no override names gives the built-in one.
#[test]
fn the_board_text_the_firmware_builds_from_is_the_merged_board() {
    let id = "waveshare-rp2040-touch-lcd-1.28";
    let r = load(
        "firmware-board",
        &format!(
            "[[board_override.\"{id}\".pin]]\npin = \"GPIO22\"\ndagon-remove = true\n\n\
             [[board_override.\"{id}\".pin]]\npin = \"GPIO21\"\nname = \"touch_rst\"\n"
        ),
    )
    .unwrap();
    let canonical = |text: &str| {
        let t: toml::Table = text.parse().unwrap();
        assert_eq!(toml::to_string(&t).unwrap(), text);
        assert!(text.ends_with('\n') && !text.ends_with("\n\n") && !text.contains('\r'));
    };
    let text = r.merged_board_toml(id).unwrap();
    canonical(&text);
    canonical(&r.merged_mcu_toml("rp2040").unwrap());
    let b: BoardDef = toml::from_str(&text).unwrap();
    let pin = |b: &BoardDef, name: &str| {
        b.pins
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .map(|p| p.pin.clone())
    };
    assert_eq!(pin(&b, "touch_rst").as_deref(), Some("GPIO21"));
    assert_eq!(pin(&b, "backlight").as_deref(), Some("GPIO25"));
    assert_eq!(b.pins, r.board(id).unwrap().pins);
    let builtin = Registry::builtin();
    let text = builtin.merged_board_toml("nucleo-f401re").unwrap();
    canonical(&text);
    assert_eq!(
        &toml::from_str::<BoardDef>(&text).unwrap(),
        builtin.board("nucleo-f401re").unwrap()
    );
    assert_eq!(builtin.merged_board_toml("no-such-board"), None);
}

#[test]
fn a_new_uniqueness_check_refuses_a_header_pin_or_i2c_pair_given_twice() {
    for (name, extra) in [
        ("header-twice", "header = [{ pin = \"GPIO2\" }, { pin = \"GPIO2\" }]"),
        (
            "i2c-twice",
            "i2c = [{ controller = \"i2c0\", sda = \"GPIO2\", scl = \"GPIO3\" }, { controller = \"i2c1\", sda = \"GPIO2\", scl = \"GPIO3\" }]",
        ),
    ] {
        let key = extra.split(' ').next().unwrap();
        let body = format!(
            "format = 1\n[board.dup]\nformat = 4\nproduct = \"Dup\"\nmcu = \"rp2040\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n{extra}\n"
        );
        let e = load_raw(name, None, &body).unwrap_err().to_string();
        assert!(e.contains(key) && e.contains("twice"), "{name}: {e}");
    }
}

#[test]
fn dagon_toml_declares_its_format_and_only_known_keys() {
    let e = load_raw("no-format", None, "topology = \"t.yaml\"\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("format = 1"), "{e}");
    let e = load_raw("format-2", None, "format = 2\n")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("format 2") && e.contains("reads dagon.toml format 1"),
        "{e}"
    );
    let e = load_raw("unknown", None, "format = 1\nmcu_overide = {}\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("mcu_overide"), "{e}");
    // Another reader's tables are left to it.
    load_raw(
        "others",
        None,
        "format = 1\ntopology = \"t.yaml\"\n[build.seg]\npackage = \"x\"\n[dagon]\npath = \"..\"\n",
    )
    .unwrap();
}

/// Each built-in MCU states the periods its watchdog can run: an override
/// on the upper boundary loads, one past either boundary is refused with
/// the MCU, the value and the range.
#[test]
fn each_mcu_s_watchdog_range_bounds_an_override() {
    let mut seen = 0;
    for m in Registry::builtin().mcus() {
        let Some(w) = m.watchdog else { continue };
        seen += 1;
        let (min, max) = (w.range_ms.min, w.range_ms.max);
        let over = |trial: u32| format!("[mcu_override.{}.watchdog]\ntrial_ms = {trial}\n", m.id);
        let r = load(&format!("wd-max-{}", m.id), &over(max)).unwrap();
        assert_eq!(r.mcu(&m.id).unwrap().watchdog.unwrap().trial_ms, max);
        for past in [max + 1, min - 1] {
            let e = load(&format!("wd-past-{}", m.id), &over(past))
                .expect_err(&m.id)
                .to_string();
            let says = format!(
                "watchdog.trial_ms {past} is outside the periods MCU '{}'s watchdog can run, \
                 {min}..={max} ms",
                m.id
            );
            assert!(e.contains(&says), "{e}");
        }
    }
    assert!(seen >= 3, "{seen}");
}

/// The ESP32-S3's watchdog range ends at 3,600,000 ms, where the RTC
/// watchdog's tick count still fits 32 bits at any calibrated slow clock:
/// an override on it loads, one past it is refused.
#[test]
fn the_esp32s3_watchdog_range_ends_at_3_600_000_ms() {
    let over = |ms: u32| format!("[mcu_override.esp32s3.watchdog]\ntrial_ms = {ms}\n");
    let r = load("wd-esp-max", &over(3_600_000)).unwrap();
    assert_eq!(
        r.mcu("esp32s3").unwrap().watchdog.unwrap().trial_ms,
        3_600_000
    );
    let e = load("wd-esp-past", &over(3_600_001))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("watchdog.trial_ms 3600001 is outside the periods MCU 'esp32s3's watchdog can run, 1..=3600000 ms"),
        "{e}"
    );
}

#[test]
fn a_rom_erase_sector_past_the_erase_regions_is_refused() {
    // The STM32F401RE has 8 erase units (sectors 0-7); the ROM loader's
    // wait for a sector comes from its unit.
    refused(
        "rom-sector",
        "[mcu_override.stm32f401re.rom_i2c]\nerase_sectors = [0, 8]\n",
        "sector 8 is not an erase unit",
    );
    // An erase unit of 0 is refused as such, not divided by.
    let e = load(
        "rom-sector-unit",
        "[[mcu_override.stm32f401re.execution.erase_regions]]\nstart = 0x0800_0000\nunit = 0\n",
    )
    .expect_err("unit 0")
    .to_string();
    assert!(!e.contains("is not an erase unit"), "{e}");
    load(
        "rom-sector-ok",
        "[mcu_override.stm32f401re.rom_i2c]\nerase_sectors = [0, 7]\n",
    )
    .unwrap();
}

/// The boards scripts/import_zephyr_board.py made from Zephyr's files load as
/// a project's own boards, beside the built-in ones.
#[test]
fn boards_imported_from_zephyr_load_as_project_boards() {
    let pico = include_str!("../../../../scripts/fixtures/imported/rpi-pico.toml");
    let nucleo = include_str!("../../../../scripts/fixtures/imported/nucleo-f401re-zephyr.toml");
    // Zephyr does not say where DAGON's slots go: the built-in RP2040 map
    // needs 4 MiB, so the 2 MiB board is refused until the project gives one.
    let e = load("zephyr-as-is", pico).unwrap_err().to_string();
    assert!(e.contains("V-16"), "{e}");
    let two_mib = "[[mcu_override.rp2040.flash_map]]\nname = \"rp2040-4m\"\nbytes = 0x20_0000\n\n\
        [[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"a\"\nsize = 0xD_C000\n\n\
        [[mcu_override.rp2040.slot]]\nimage = \"app\"\nbinding = \"b\"\nat = 0x1012_4000\n\
        size = 0xD_C000\n\n\
        [[mcu_override.rp2040.slot_layout]]\ntarget = \"app\"\nslot = \"a\"\n\
        capacity = 0xD_BE00\n\n\
        [[mcu_override.rp2040.slot_layout]]\ntarget = \"app\"\nslot = \"b\"\n\
        start = 0x1012_4000\ncapacity = 0xD_BE00\n";
    let reg = load("zephyr", &format!("{pico}\n{nucleo}\n{two_mib}")).unwrap();
    let b = reg.board("rpi-pico").unwrap();
    assert_eq!((b.mcu.as_str(), b.flash_bytes), ("rp2040", 0x20_0000));
    assert_eq!(b.header.len(), 26);
    let b = reg.board("nucleo-f401re-zephyr").unwrap();
    assert_eq!((b.mcu.as_str(), b.flash_bytes), ("stm32f401re", 0x8_0000));
}
