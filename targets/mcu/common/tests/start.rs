//! What a node's public start gives an application, on the simulated
//! STM32F401 slot store: its extensions observe every `app.*` event of an
//! update and its trial and answer the decision points; the board's
//! bring-up runs before `app.bring_up`; the application confirms its own
//! trial with `Confirm::Own`; the board package's extension sits between
//! the publisher and the app's; the published snapshot follows an update.

use std::sync::{Arc, Mutex};

use dagon_agent::snapshot::{AppSnapshot, Phase};
use dagon_agent::{Agent, SlotStore};
use dagon_events::{BoardBringUp, Ctx, Event, Extension, Health, Published, TrialInfo, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_common::hooks::{
    finish_start, node_chain, App, AppEvents, BoardExtension, Confirm, NodeChain,
};
use dagon_proto::payload::*;
use dagon_proto::{split_status, Cmd, Frame, Status};
use dagon_sim::store::Boot;
use dagon_sim::{f401_config, sample, Layout, SimFlash, SimStore};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data; `sample::PUBKEY`).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

type Log = Arc<Mutex<Vec<String>>>;

/// An application (or, with a `prefix`, a board package) extension: logs
/// every event and answers `self_check`.
struct AppExt {
    log: Log,
    health: Option<Health>,
    prefix: &'static str,
}

impl Extension for AppExt {
    fn id(&self) -> &'static str {
        "test-app"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        let k = e.kind();
        let k = if e.is_replayed() {
            format!("{}replay:{k}", self.prefix)
        } else {
            format!("{}{k}", self.prefix)
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
        prefix: "",
    }));
    App {
        self_check: dagon_mcu_common::hooks::always,
        exts: Some(ext),
        confirm,
    }
}

/// A node's agent the way the MCU packages' starts build it, with the
/// board package's extension `board`, and the app's events.
fn start_with(
    store: &SimStore,
    board: Option<BoardExtension>,
    app: App,
) -> (Agent<SimStore, NodeChain>, AppEvents) {
    let events = Box::leak(Box::new(Published::new()));
    let (chain, reader) = node_chain(
        events,
        board,
        app.exts,
        app.self_check,
        || 0,
        Watchdog::new(4_000, 8_000),
    );
    let agent = Agent::new(f401_config(sample::PUBKEY), store.clone(), chain);
    (agent, reader)
}

fn start(store: &SimStore, app: App) -> Agent<SimStore, NodeChain> {
    start_with(store, None, app).0
}

fn snapshot(events: &AppEvents) -> AppSnapshot {
    events.latest().expect("events were published").state
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

    /// Sends `part` from chunk `from` on, 200 bytes a chunk.
    fn chunks(&mut self, a: &mut Agent<SimStore, NodeChain>, cmd: Cmd, part: &[u8], from: usize) {
        let mut b = vec![0u8; 300];
        for (i, chunk) in part.chunks(200).enumerate().skip(from) {
            let c = XferChunk {
                offset: (i * 200) as u32,
                data: chunk,
            };
            let n = c.encode(&mut b).unwrap();
            assert_eq!(self.call(a, cmd, &b[..n]), Status::Ok);
        }
    }

    /// The transfer up to its first body chunk.
    fn begin(&mut self, a: &mut Agent<SimStore, NodeChain>, slot: u8, img: &[u8]) {
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
        self.chunks(a, Cmd::XferHdr, hdr, 0);
        self.chunks(a, Cmd::XferData, &body[..200], 0);
    }

    /// The rest of a transfer [`Wire::begin`] started.
    fn rest(&mut self, a: &mut Agent<SimStore, NodeChain>, img: &[u8]) {
        self.chunks(a, Cmd::XferData, &img[HEADER_SIZE..], 1);
        assert_eq!(self.call(a, Cmd::XferEnd, &[]), Status::Ok);
    }

    fn transfer(&mut self, a: &mut Agent<SimStore, NodeChain>, slot: u8, img: &[u8]) {
        self.begin(a, slot, img);
        self.rest(a, img);
    }
}

/// A node running 1.0.0 from slot A, confirmed.
fn provisioned() -> SimStore {
    let mut store = SimStore::new(
        SimFlash::stm32f401xe(1),
        Layout::STM32F401XE,
        sample::PUBKEY,
    );
    store.provision(0, sample::SEG_1_0_0).unwrap();
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

/// The board package's extension is in the chain at its place: after the
/// publisher, before the app's extensions. It observes every event first
/// and its decision answer stands over the app's.
#[test]
fn the_board_extension_observes_and_decides_before_the_app() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    take(&log);
    let board: &'static mut AppExt = Box::leak(Box::new(AppExt {
        log: log.clone(),
        health: Some(Health::Unhealthy),
        prefix: "board:",
    }));
    let healthy_app = app(&log, Some(Health::Healthy), Confirm::Own);
    let (mut a, _) = start_with(&store, Some(board), healthy_app);
    assert!(
        finish_start(&mut a, None, Confirm::Own),
        "the board said no"
    );
    let seen = std::mem::take(&mut *log.lock().unwrap());
    let at = seen.iter().position(|k| k == "board:starting").unwrap();
    assert_eq!(seen[at + 1], "starting", "{seen:?}");
    assert!(
        seen.ends_with(&[
            "board:self_check".into(),
            "self_check".into(),
            "board:confirm_refused".into(),
            "confirm_refused".into(),
        ]),
        "{seen:?}"
    );
}

/// The app's events fold into a snapshot (phase, version, progress,
/// trial) that a display task waits on.
#[test]
fn the_published_snapshot_follows_an_update_and_its_trial() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    let (mut a, events) = start_with(&store, None, app(&log, None, Confirm::ByHost));
    let s = snapshot(&events);
    assert_eq!(
        (s.phase, s.version, s.trial),
        (Phase::Idle, Version::new(1, 0, 0), false)
    );

    let body: Vec<u8> = (0..900u32).map(|i| (i * 7) as u8).collect();
    let img = image(1, Version::new(1, 1, 0), &body);
    let mut w = Wire { seq: 0 };
    w.begin(&mut a, 1, &img);
    let s = snapshot(&events);
    assert_eq!(s.phase, Phase::Transferring);
    assert_eq!(s.progress, Some((200, 900)), "the first body chunk");
    w.rest(&mut a, &img);
    let s = snapshot(&events);
    assert_eq!((s.phase, s.progress), (Phase::Written, None));
    assert_eq!(
        w.call(&mut a, Cmd::SetPending, &[target::APP, 1]),
        Status::Ok
    );
    assert_eq!(snapshot(&events).phase, Phase::Pending);
    assert_eq!(
        w.call(&mut a, Cmd::Reboot, &[reboot_mode::NORMAL]),
        Status::Ok
    );
    assert_eq!(snapshot(&events).phase, Phase::RebootPending);

    assert_eq!(store.boot(), Boot::Run(1));
    let (mut a, events) = start_with(&store, None, app(&log, None, Confirm::Own));
    let s = snapshot(&events);
    assert_eq!(
        (s.phase, s.version, s.trial),
        (Phase::Trial, Version::new(1, 1, 0), true)
    );
    finish_start(&mut a, None, Confirm::Own);
    let s = snapshot(&events);
    assert_eq!(
        (s.phase, s.version, s.trial),
        (Phase::Confirmed, Version::new(1, 1, 0), false)
    );
}

/// A trial that runs out is shown as rolled back to the version that runs.
#[test]
fn the_snapshot_shows_a_rollback() {
    let log: Log = Arc::default();
    let mut store = provisioned();
    update(&mut store, &log, Confirm::ByHost, None);
    while store.boot() == Boot::Run(1) {}
    let (_a, events) = start_with(&store, None, app(&log, None, Confirm::ByHost));
    let s = snapshot(&events);
    assert_eq!(
        (s.phase, s.version, s.trial),
        (Phase::RolledBack, Version::new(1, 0, 0), false)
    );
}
