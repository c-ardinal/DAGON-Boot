//! Each node's erase wait and trial wait come from its MCU definition: a
//! new MCU is data, and the built-in MCUs' plans are pinned.

use dagon_host::{mcu_timings, node_timings, Host, HostClock, Reconnect, Transport};
use dagon_port::timeout::{
    erase_limit_ms, EraseOp, NodeTimings, TimingError, ERASE_FALLBACK_MS, ERASE_MARGIN_MS,
};
use dagon_proto::payload::target;
use dagon_topology::defs::{Registry, Source};

/// An MCU no code names: unequal erase units, its own watchdog.
const NOVA: &str = r#"
format = 4
id = "nova-m4"
name = "NOVA-M4"
port = "self-boot"

[[flash_map]]
name = "nova-1m"
base = 0x0800_0000
bytes = 0x10_0000
xip = true

[[slot]]
image = "app"
binding = "a"
header = "inline"
at = 0x0802_0000
size = 0x2_0000

[[slot]]
image = "app"
binding = "b"
header = "inline"
at = 0x0804_0000
size = 0x2_0000

[writers]
app = ["app"]

[[slot_layout]]
target = "app"
slot = "a"
start = 0x0802_0000
capacity = 0x1_FE00
header = "inline"

[[slot_layout]]
target = "app"
slot = "b"
start = 0x0804_0000
capacity = 0x1_FE00
header = "inline"

[writer]
kind = "probe-rs"
chip = "NOVA"

[watchdog]
kind = "iwdg"
trial_ms = TRIAL
confirmed_ms = 6_000
trial_attempts = ATTEMPTS
range_ms = { min = 1, max = 32_768 }
flag = "unmeasured"

[execution]
masked_max_us = "unmeasured"
flash_op = "fetch-stall"
write_unit = 4
erased_value = 0xFF
erase_regions = [
    { start = 0x0800_0000, len = 0x2_0000, unit = 0x4000, unit_max_ms = UNIT_MS },
    { start = 0x0802_0000, len = 0x2_0000, unit = 0x8000, unit_max_ms = 700 },
    { start = 0x0804_0000, len = 0xC_0000, unit = 0x2_0000, unit_max_ms = 1_500 },
]

[connection]
label = "SWD"
hint = "SWD probe"
"#;

fn nova(trial: u32, attempts: u8, unit_ms: u32) -> String {
    NOVA.replace("TRIAL", &trial.to_string())
        .replace("ATTEMPTS", &attempts.to_string())
        .replace("UNIT_MS", &unit_ms.to_string())
}

fn registry(name: &str, mcu: &str) -> Registry {
    registry_with(name, mcu, "")
}

/// A project registry with `mcu` and `boards` (`[board.<id>]` tables).
fn registry_with(name: &str, mcu: &str, boards: &str) -> Registry {
    let dir = std::env::temp_dir().join(format!("dagon-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("m")).unwrap();
    std::fs::write(dir.join("m/dagon-mcu.toml"), mcu).unwrap();
    std::fs::write(
        dir.join("dagon.toml"),
        format!("format = 1\nmcu_files = [\"m/dagon-mcu.toml\"]\n{boards}"),
    )
    .unwrap();
    let r = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]);
    std::fs::remove_dir_all(dir).unwrap();
    r.unwrap()
}

/// The simulator pair with its node made of `mcu`.
fn topology(mcu: &str) -> dagon_topology::Topology {
    let yaml = include_str!("../../../topology/sim-pair.yaml")
        .replace("mcu: stm32f401re", &format!("mcu: {mcu}"))
        .replace("    board: nucleo-f401re\n", "");
    dagon_topology::parse_str(&yaml).unwrap()
}

/// A board of the synthetic MCU carrying a flash part with `regions`.
fn board(id: &str, regions: &str) -> String {
    format!(
        "[board.{id}]\nformat = 4\nproduct = \"P\"\nmcu = \"nova-m4\"\nflash_bytes = 0x10_0000\n\
         drawing = \"nucleo-64\"\n[board.{id}.flash]\npart = \"X\"\nerase_regions = [{regions}]\n"
    )
}

/// The synthetic node on board `id`.
fn topology_on(id: &str) -> dagon_topology::Topology {
    let yaml = include_str!("../../../topology/sim-pair.yaml")
        .replace("mcu: stm32f401re", "mcu: nova-m4")
        .replace("board: nucleo-f401re", &format!("board: {id}"));
    dagon_topology::parse_str(&yaml).unwrap()
}

const SLOW: &str = "{ start = 0x0800_0000, len = 0x10_0000, unit = 0x8000, unit_max_ms = 1_400 }";
const BLOCK: &str = "{ start = 0x0800_0000, len = 0x10_0000, unit = 0x1_0000, unit_max_ms = 900 }";

/// A board's `[flash]` replaces its MCU's erase regions whole: a slower
/// part and a part erased in larger blocks each change the plan the node
/// erases and the wait the host allows.
#[test]
fn a_boards_flash_part_changes_the_plan_and_the_wait() {
    let boards = format!("{}{}", board("nova-slow", SLOW), board("nova-block", BLOCK));
    let reg = registry_with("nova-boards", &nova(6_000, 2, 300), &boards);
    let host_of = |t: &dagon_topology::Topology| {
        let mut h = Host::new(Idle, Idle);
        h.timings = node_timings(&reg, t).unwrap();
        h
    };
    let ops = |h: &Host<Idle, Idle>| -> Vec<EraseOp> {
        let t = h.timings_of(NODE);
        let layout = t.erase.unwrap();
        layout.plan(target::APP, 0, 512, 0x5000).unwrap().collect()
    };
    // The MCU alone: one 32 KiB unit of 700 ms.
    let h = host_of(&topology("nova-m4"));
    assert_eq!(ops(&h), [op(0x0802_0000, 0x8000, 700)]);
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x5000),
        700 + ERASE_MARGIN_MS
    );
    let h = host_of(&topology_on("nova-slow"));
    assert_eq!(ops(&h), [op(0x0802_0000, 0x8000, 1_400)]);
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x5000),
        1_400 + ERASE_MARGIN_MS
    );
    let h = host_of(&topology_on("nova-block"));
    assert_eq!(ops(&h), [op(0x0802_0000, 0x1_0000, 900)]);
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x5000),
        900 + ERASE_MARGIN_MS
    );
    // The node composes its layout from the same regions (its board's) and
    // its MCU's slot layouts: the agent's plan is the host's.
    let m = reg.mcu("nova-m4").unwrap();
    for id in ["nova-slow", "nova-block"] {
        let layout = m.erase_layout_on(reg.board(id)).unwrap();
        let node: Vec<EraseOp> = dagon_agent::erase_plan(&layout, target::APP, 0, 0x5000)
            .unwrap()
            .collect();
        assert_eq!(node, ops(&host_of(&topology_on(id))), "{id}");
    }
}

/// A board's `[flash]` regions that break a rule of its MCU are refused,
/// naming the board.
#[test]
fn a_boards_flash_regions_are_checked_with_its_mcu() {
    for (regions, want) in [
        (
            "{ start = 0x0800_0000, len = 0x10_0000, unit = 0x8000, unit_max_ms = 3_001 }",
            "must be at least 2 x",
        ),
        (
            "{ start = 0x0803_0000, len = 0xD_0000, unit = 0x8000, unit_max_ms = 100 }",
            "lies in no",
        ),
    ] {
        let dir = std::env::temp_dir().join(format!("dagon-host-nova-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m")).unwrap();
        std::fs::write(dir.join("m/dagon-mcu.toml"), nova(6_000, 2, 300)).unwrap();
        let text = format!(
            "format = 1\nmcu_files = [\"m/dagon-mcu.toml\"]\n{}",
            board("nova-bad", regions)
        );
        std::fs::write(dir.join("dagon.toml"), text).unwrap();
        let e = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]).unwrap_err();
        std::fs::remove_dir_all(dir).unwrap();
        let msg = e.to_string();
        assert!(msg.contains(want) && msg.contains("nova-bad"), "{msg}");
    }
}

#[derive(Clone)]
struct Idle;

impl Transport for Idle {
    type Error = ();
    fn max_payload(&self) -> usize {
        64
    }
    fn reconnect_kind(&self) -> Reconnect {
        Reconnect::Stays
    }
    async fn write(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    async fn read(&mut self, _: &mut [u8]) -> Result<usize, ()> {
        Ok(0)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for Idle {
    fn now_ms(&self) -> u64 {
        0
    }
    async fn sleep_ms(&mut self, _: u32) {}
}

const NODE: u8 = 1;

#[test]
fn a_new_mcu_is_data_its_waits_follow_its_definition() {
    let reg = registry("nova", &nova(6_000, 2, 300));
    let timings = node_timings(&reg, &topology("nova-m4")).unwrap();
    let mut h = Host::new(Idle, Idle);
    h.timings = timings;
    let t = h.timings_of(NODE);
    assert_eq!((t.trial_ms, t.trial_attempts), (6_000, 2));
    // APP-A sits in 32 KiB units of 700 ms: the header and 0x5000 bytes
    // are one unit, the largest body four.
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x5000),
        700 + ERASE_MARGIN_MS
    );
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x1_FE00),
        4 * 700 + ERASE_MARGIN_MS
    );
    // APP-B sits in one 128 KiB unit of 1.5 s.
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 1, 512, 0x1_FE00),
        1_500 + ERASE_MARGIN_MS
    );
    // A node the host has no entry for waits the fallback.
    assert_eq!(
        h.timings_of(7).trial_ms,
        NodeTimings::<()>::FALLBACK.trial_ms
    );
    assert_eq!(
        h.erase_wait_ms(7, target::APP, 0, 512, 0x5000),
        ERASE_FALLBACK_MS
    );
    // The same definition with other numbers: the waits move with it.
    let reg = registry(
        "nova2",
        &nova(12_000, 1, 300).replace("unit_max_ms = 700", "unit_max_ms = 900"),
    );
    h.timings = node_timings(&reg, &topology("nova-m4")).unwrap();
    assert_eq!(h.timings_of(NODE).trial_ms, 12_000);
    assert_eq!(
        h.erase_wait_ms(NODE, target::APP, 0, 512, 0x5000),
        900 + ERASE_MARGIN_MS
    );
}

#[test]
fn a_definition_breaking_a_timing_rule_is_refused_naming_its_mcu() {
    for (trial, attempts, rule) in [
        (900, 1, TimingError::PollMissesTrial),
        (3_000, 1, TimingError::ConfirmMissesTrial),
        (6_000, 5, TimingError::RebootWaitMissesRollback),
        (13_001, 2, TimingError::RebootWaitMissesRollback),
    ] {
        let reg = registry(
            &format!("bad{trial}-{attempts}"),
            &nova(trial, attempts, 100)
                .replace("unit_max_ms = 700", "unit_max_ms = 100")
                .replace("unit_max_ms = 1_500", "unit_max_ms = 100"),
        );
        let e = node_timings(&reg, &topology("nova-m4")).unwrap_err();
        assert_eq!(
            (e.mcu.as_str(), e.rule),
            ("nova-m4", rule),
            "{trial} {attempts}"
        );
        assert!(e.to_string().contains("nova-m4"), "{e}");
    }
    // At each rule's limit the definition passes.
    for (trial, attempts) in [(3_001, 1), (6_000, 4), (13_000, 2)] {
        let reg = registry(
            &format!("ok{trial}-{attempts}"),
            &nova(trial, attempts, 100)
                .replace("unit_max_ms = 700", "unit_max_ms = 100")
                .replace("unit_max_ms = 1_500", "unit_max_ms = 100"),
        );
        node_timings(&reg, &topology("nova-m4")).unwrap();
    }
}

/// An MCU missing from the registry gets the fallback.
#[test]
fn an_unknown_mcu_gets_the_fallback() {
    let t = node_timings(Registry::builtin(), &topology("not-defined")).unwrap();
    let node = t.iter().find(|(n, _)| *n == NODE).unwrap();
    assert_eq!(node.1.trial_ms, NodeTimings::<()>::FALLBACK.trial_ms);
    assert!(node.1.erase.is_none());
}

fn op(addr: u32, len: u32, max_ms: u32) -> EraseOp {
    EraseOp { addr, len, max_ms }
}

/// The 4 KiB sector erase time the RP2040 and ESP32-S3 definitions take
/// for a board flash part (the largest tSE max of their candidate parts).
const SECTOR_MS: u32 = 500;

/// `n` sectors of 4 KiB from `addr`, each `SECTOR_MS`.
fn sectors(addr: u32, n: u32) -> Vec<EraseOp> {
    (0..n)
        .map(|i| op(addr + i * 0x1000, 0x1000, SECTOR_MS))
        .collect()
}

/// Target, slot, header and body bytes, the planned erase, its limit.
type Pin = (u8, u8, u32, u32, Vec<EraseOp>, u32);

/// The pinned plans of each built-in MCU's targets and slots.
fn pins(mcu: &str) -> Vec<Pin> {
    let lim = |ms: u32| ms + ERASE_MARGIN_MS;
    match mcu {
        // STM32F401: sectors 16 KiB x4, 64 KiB, 128 KiB x3 (unequal units).
        "stm32f401re" => vec![
            (
                target::APP,
                0,
                512,
                3_000,
                vec![op(0x0802_0000, 0x2_0000, 4_000)],
                5_000,
            ),
            (
                target::APP,
                1,
                512,
                0x1_FE00,
                vec![op(0x0804_0000, 0x2_0000, 4_000)],
                5_000,
            ),
            (
                target::BOOTCORE,
                0,
                512,
                0xFE00,
                vec![op(0x0801_0000, 0x1_0000, 2_400)],
                3_400,
            ),
            (
                target::BOOTCORE,
                1,
                512,
                0x1_FE00,
                vec![op(0x0806_0000, 0x2_0000, 4_000)],
                5_000,
            ),
        ],
        // RP2040: 4 KiB sectors over the slot's used part.
        "rp2040" => vec![
            (
                target::APP,
                0,
                512,
                3_000,
                sectors(0x1004_8000, 1),
                lim(SECTOR_MS),
            ),
            (
                target::APP,
                1,
                512,
                3_700,
                sectors(0x1014_8000, 2),
                lim(2 * SECTOR_MS),
            ),
            (
                target::APP,
                0,
                512,
                0xF_FE00,
                sectors(0x1004_8000, 256),
                lim(256 * SECTOR_MS),
            ),
            (
                target::BOOTCORE,
                0,
                512,
                2_000,
                sectors(0x1000_8000, 1),
                lim(SECTOR_MS),
            ),
            (
                target::BOOTCORE,
                1,
                512,
                0x1_FE00,
                sectors(0x1002_8000, 32),
                lim(32 * SECTOR_MS),
            ),
        ],
        // ESP32-S3: the detached header's sector first, then the body's
        // 4 KiB sectors (no block erase assumed).
        "esp32s3" => {
            let with_header = |hdr: u32, body: u32, n: u32| {
                let mut v = sectors(hdr, 1);
                v.extend(sectors(body, n));
                v
            };
            vec![
                (
                    target::APP,
                    0,
                    512,
                    3_000,
                    with_header(0x71_0000, 0x11_0000, 1),
                    lim(2 * SECTOR_MS),
                ),
                (
                    target::APP,
                    1,
                    512,
                    0x1_0001,
                    with_header(0x71_1000, 0x41_0000, 17),
                    lim(18 * SECTOR_MS),
                ),
                (
                    target::APP,
                    0,
                    512,
                    0x30_0000,
                    with_header(0x71_0000, 0x11_0000, 768),
                    lim(769 * SECTOR_MS),
                ),
                (
                    target::AGENT,
                    2,
                    512,
                    3_000,
                    with_header(0x71_2000, 0x1_0000, 1),
                    lim(2 * SECTOR_MS),
                ),
                (
                    target::AGENT,
                    2,
                    512,
                    0x10_0000,
                    with_header(0x71_2000, 0x1_0000, 256),
                    lim(257 * SECTOR_MS),
                ),
            ]
        }
        other => panic!("no pins for {other}"),
    }
}

/// Every built-in MCU alone and on each built-in board of it: the
/// effective regions (a built-in board carries no `[flash]`, so its MCU's),
/// and the plan and limit of each target and slot; a body one byte past a
/// slot has no plan.
#[test]
fn the_builtin_plans_are_pinned() {
    let reg = Registry::builtin();
    for m in reg.mcus() {
        let boards: Vec<_> = reg.boards().iter().filter(|b| b.mcu == m.id).collect();
        let pairs = std::iter::once(None).chain(boards.iter().map(|b| Some(*b)));
        for board in pairs {
            let at = format!("{} on {:?}", m.id, board.map(|b| &b.id));
            assert_eq!(m.erase_regions_on(board), m.erase_regions_on(None), "{at}");
            let timings = mcu_timings(m, board).unwrap();
            let layout = timings.erase.as_ref().unwrap();
            for (t, slot, hdr, body, ops, limit) in pins(&m.id) {
                let plan = layout.plan(t, slot, hdr, body).unwrap();
                assert_eq!(
                    plan.clone().collect::<Vec<_>>(),
                    ops,
                    "{at} {t} {slot} {body}"
                );
                assert_eq!(erase_limit_ms(plan), limit, "{at} {t} {slot} {body}");
                assert_eq!(timings.erase_limit_ms(t, slot, hdr, body), limit, "{at}");
            }
            for l in layout.slots.iter() {
                assert!(
                    layout.plan(l.target, l.slot, 512, l.capacity).is_ok(),
                    "{at}"
                );
                assert!(
                    layout.plan(l.target, l.slot, 512, l.capacity + 1).is_err(),
                    "{at}"
                );
            }
        }
    }
    // The fallback's trial is the longest and its attempts the most of the
    // built-in definitions.
    let watchdogs = || reg.mcus().iter().filter_map(|m| m.watchdog);
    let fallback = NodeTimings::<()>::FALLBACK;
    assert_eq!(
        watchdogs().map(|w| w.trial_ms).max(),
        Some(fallback.trial_ms)
    );
    assert_eq!(
        watchdogs().map(|w| w.trial_attempts).max(),
        Some(fallback.trial_attempts)
    );
    // The trial periods are the definitions'.
    for (mcu, trial, attempts) in [
        ("stm32f401re", 8_000, 3),
        ("rp2040", 8_000, 3),
        ("esp32s3", 4_000, 1),
    ] {
        let t = mcu_timings(reg.mcu(mcu).unwrap(), None).unwrap();
        assert_eq!((t.trial_ms, t.trial_attempts), (trial, attempts), "{mcu}");
    }
}
