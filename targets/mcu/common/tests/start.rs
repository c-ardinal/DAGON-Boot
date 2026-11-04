//! What a node's public start gives an application, on the simulated
//! STM32F401 slot store: its extensions observe every `app.*` event of an
//! update and its trial and answer the decision points; the board's
//! bring-up runs before `app.bring_up`; the application confirms its own
//! trial with `Confirm::Own`.

use std::sync::{Arc, Mutex};

use dagon_agent::{Agent, SlotStore};
use dagon_events::{BoardBringUp, Ctx, Event, Extension, Health, Published, TrialInfo, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_common::hooks::{finish_start, node_chain, App, Confirm, NodeChain};
use dagon_proto::payload::*;
use dagon_proto::{split_status, Cmd, Frame, Status};
use dagon_sim::store::Boot;
use dagon_sim::{desk, f401_config, Layout, SimFlash, SimStore};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data; `desk::PUBKEY`).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

type Log = Arc<Mutex<Vec<String>>>;

/// An application extension: logs every event and answers `self_check`.
struct AppExt {
    log: Log,
    health: Option<Health>,
}

impl Extension for AppExt {
    fn id(&self) -> &'static str {
        "test-app"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        let k = e.kind();
        let k = if e.is_replayed() {
            format!("replay:{k}")
        } else {
            k.to_string()
        };
        self.log.lock().unwrap().push(k);
    }
    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        self.health
    }
}

/// A board bring-up that logs when it runs.
struct Port(Log);

impl BoardBringUp for Port {
    fn bring_up(&mut self) {
        self.0.lock().unwrap().push("port".into());
    }
}

fn unhealthy() -> bool {
    false
}

fn app(log: &Log, health: Option<Health>, confirm: Confirm) -> App {
    let ext: &'static mut AppExt = Box::leak(Box::new(AppExt {
        log: log.clone(),
        health,
    }));
    App {
        self_check: dagon_mcu_common::hooks::always,
        exts: Some(ext),
        confirm,
    }
}

/// A node's agent the way the MCU packages' starts build it.
fn start(store: &SimStore, app: App) -> Agent<SimStore, NodeChain> {
    let events = Box::leak(Box::new(Published::new()));
    let (chain, _reader) = node_chain(
        events,
        app.exts,
        app.self_check,
        || 0,
        Watchdog::new(4_000, 8_000),
    );
    Agent::new(f401_config(desk::PUBKEY), store.clone(), chain)
}

fn take(log: &Log) -> Vec<String> {
    let mut seen = std::mem::take(&mut *log.lock().unwrap());
    seen.dedup();
    seen
}

fn image(slot: u8, version: Version, body: &[u8]) -> Vec<u8> {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: Layout::STM32F401XE.slots[slot as usize].0 + HEADER_SIZE as u32,
        payload_size: body.len() as u32,
        version,
        build_flags: build_flags::TEST_KEY,
        release_seq: 2,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
}

struct Wire {
    seq: u8,
}

impl Wire {
    fn call(&mut self, a: &mut Agent<SimStore, NodeChain>, cmd: Cmd, payload: &[u8]) -> Status {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = vec![0u8; 300];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = a.handle(&buf[..n]).expect("reply");
        let (f, _) = Frame::decode(reply.frame, 300).unwrap();
        split_status(f.payload).unwrap().0
    }

    fn transfer(&mut self, a: &mut Agent<SimStore, NodeChain>, slot: u8, img: &[u8]) {
        let mut b = vec![0u8; 300];
        let begin = XferBegin {
            target: target::APP,
            slot,
            total_len: img.len() as u32,
            hdr_len: HEADER_SIZE as u16,
        };
        let n = begin.encode(&mut b).unwrap();
        assert_eq!(self.call(a, Cmd::XferBegin, &b[..n]), Status::Ok);
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, chunk) in part.chunks(200).enumerate() {
                let c = XferChunk {
                    offset: (i * 200) as u32,
                    data: chunk,
                };
                let n = c.encode(&mut b).unwrap();
                assert_eq!(self.call(a, cmd, &b[..n]), Status::Ok);
            }
        }
        assert_eq!(self.call(a, Cmd::XferEnd, &[]), Status::Ok);
    }
}

/// A node running 1.0.0 from slot A, confirmed.
fn provisioned() -> SimStore {
    let mut store = SimStore::new(SimFlash::stm32f401xe(1), Layout::STM32F401XE, desk::PUBKEY);
    store.provision(0, desk::SEG_1_0_0).unwrap();
    assert_eq!(store.boot(), Boot::Run(0));
    store
}

/// 1.1.0 sent to slot B and made pending; the next reset tries it.
fn update(store: &mut SimStore, log: &Log, confirm: Confirm, health: Option<Health>) {
    let mut a = start(store, app(log, health, confirm));
    let body: Vec<u8> = (0..900u32).map(|i| (i * 7) as u8).collect();
    let mut w = Wire { seq: 0 };
    w.transfer(&mut a, 1, &image(1, Version::new(1, 1, 0), &body));
    assert_eq!(
        w.call(&mut a, Cmd::SetPending, &[target::APP, 1]),
        Status::Ok
    );
    assert_eq!(
        w.call(&mut a, Cmd::Reboot, &[reboot_mode::NORMAL]),
        Status::Ok
    );
    assert_eq!(store.boot(), Boot::Run(1));
}

#[test]
fn an_apps_extensions_see_the_update_its_trial_and_its_own_confirm() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    let seen = take(&log);
    for want in [
        "starting",
        "update_offered",
        "transfer_started",
        "transfer_progress",
        "verify_ok",
        "written",
        "pending_set",
        "reboot_requested",
    ] {
        assert!(seen.iter().any(|k| k == want), "{want} in {seen:?}");
    }

    // The trial boot: the start confirms the app's own trial.
    let mut a = start(&store, app(&log, None, Confirm::Own));
    let mut port = Port(log.clone());
    let on_trial = finish_start(&mut a, Some(("nucleo-f401re", &mut port)), Confirm::Own);
    assert!(!on_trial);
    let seen = take(&log);
    let from = seen.iter().position(|k| k == "replay:trial_started");
    let from = from.unwrap_or_else(|| panic!("trial_started in {seen:?}"));
    assert_eq!(
        seen[from..],
        [
            "replay:trial_started",
            "port",
            "bring_up",
            "self_check",
            "confirmed",
            "demoted"
        ]
    );
    assert_eq!(store.clone().status().slots[1].state, SlotState::Confirmed);
}

#[test]
fn by_host_leaves_the_trial_and_a_board_without_bring_up_emits_nothing() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    take(&log);
    let mut a = start(&store, app(&log, None, Confirm::ByHost));
    assert!(finish_start(&mut a, None, Confirm::ByHost));
    let seen = take(&log);
    assert!(
        !seen.iter().any(|k| k == "bring_up" || k == "confirmed"),
        "{seen:?}"
    );
    assert_eq!(store.clone().status().slots[1].state, SlotState::Trying);
}

/// An unhealthy answer (the app's extension, or its function) keeps the
/// trial: the boot core rolls it back, and the next start replays that.
#[test]
fn an_unhealthy_own_confirm_keeps_the_trial_until_it_rolls_back() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    take(&log);
    let mut a = start(&store, app(&log, Some(Health::Unhealthy), Confirm::Own));
    assert!(finish_start(&mut a, None, Confirm::Own));
    let seen = take(&log);
    assert!(
        seen.ends_with(&["self_check".into(), "confirm_refused".into()]),
        "{seen:?}"
    );

    // The function answers after the extensions: an abstaining extension
    // leaves the decision to it.
    let mut fn_app = app(&log, None, Confirm::Own);
    fn_app.self_check = unhealthy;
    let mut a = start(&store, fn_app);
    assert!(finish_start(&mut a, None, Confirm::Own));
    assert!(take(&log).ends_with(&["self_check".into(), "confirm_refused".into()]));

    // Attempts run out: back on 1.0.0, and the start says so.
    while store.boot() == Boot::Run(1) {}
    let _a = start(&store, app(&log, None, Confirm::Own));
    let seen = take(&log);
    assert!(seen.iter().any(|k| k == "replay:rolled_back"), "{seen:?}");
}

/// Without extensions, the start's chain still answers with the app's
/// function, and an application with no `App` fields set is `App::new`.
#[test]
fn an_app_without_extensions_is_its_self_check() {
    let a = App::new(unhealthy);
    assert!(a.exts.is_none());
    assert_eq!(a.confirm, Confirm::ByHost);
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    let mut agent = start(&store, App::new(unhealthy));
    assert!(finish_start(&mut agent, None, Confirm::Own));
}
