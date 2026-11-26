//! Exhaustive exploration of a whole node's boot state on the RP2040's
//! layout: every sequence of what the host and the node do (an app or a
//! boot core written through the app's agent and set pending, a CONFIRM,
//! a boot, a power loss) up to a bound, with a reset or a power loss after
//! any flash operation of any of them, over the RP2040's production code:
//! stage0's start (`bootcore::start` over `bl::STAGE0`, the confirmed app
//! included), the boot
//! core's hand-over check (`bl::handed_over`), its app decision
//! (`boot::decide` as `BootRun` runs it, the log's rebuild included) and
//! its settle (`bl::settle_core`), the agent (`dagon_agent::Agent`) over
//! the store (`store::Store`), the trial cell in retained memory (kept by
//! a reset, cleared by a power loss). The F401's search
//! (`targets/mcu/stm32f401re/tests/explore.rs`) over this MCU's two logs
//! (the boot core log and the app log).
//!
//! States are compared by their flash and retained contents, so each
//! distinct state is expanded once. After every step these hold:
//!
//! - N1: stage0 always starts something (a boot core, or the confirmed
//!   app when no core may start).
//! - N2: no broken image (an app that never answers, a boot core that
//!   fails its own check) is ever confirmed.
//! - N3: an app stays confirmed in the app log.
//! - N4: from every state, boots with no further fault end within a
//!   bound at a good app that runs with no trial open (a good app on
//!   trial that the host does not confirm is rolled back too).

mod cut;

use cut::*;
use dagon_agent::{Agent, Config};
use dagon_core::boot::{decide, rebuild, BootConfig, Decision};
use dagon_core::bootcore::{self, Apps, Boot, MustReset, Settled, CLEARED};
use dagon_core::log::{Log, LogError, LogState};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::slot;
use dagon_format::SigVerifier;
use dagon_mcu_rp2040::bl;
use dagon_mcu_rp2040::layout::*;
use dagon_mcu_rp2040::store::{Chip, Store};
use dagon_mcu_rp2040::{EraseRegion, MCU_ID, TRIAL_ATTEMPTS};
use dagon_node::guard::Watch;
use dagon_port::SlotStore;
use dagon_proto::payload::*;
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

/// An image with this minor version is broken: an app that crashes before
/// it answers, a boot core that fails its own check.
const BROKEN: u8 = 9;
/// Body bytes of the images the search writes: three chunks.
const BODY: usize = 600;
/// XFER chunk bytes.
const CHUNK: usize = 250;
/// Boots with no fault after which a node has settled.
const SETTLE_BOOTS: usize = 3 * (TRIAL_ATTEMPTS as usize + 2);
/// Resets one boot may take (a core that must reset, stage0 counting).
const RESETS: usize = 2 * (bootcore::MAX_COUNT as usize + 1);

/// The MCU's default erase regions, as a board's `BoardDef` gives them.
static REGIONS: &[EraseRegion] = &dagon_mcu_rp2040::ERASE_REGIONS;

/// The guard of a store nothing watches.
struct Unwatched;

impl Watch for Unwatched {
    fn busy(&self, _: usize) {}
    fn busy_for(&self, _: usize, _: u64) {}
    fn idle(&self, _: usize) {}
    fn feed(&self) {}
    fn set_trial(&self, _: bool) {}
    fn reboot(&self) -> ! {
        unreachable!("the store never reboots")
    }
}

/// The trial cell holds, and the boot core receives.
const CHIP: Chip = Chip {
    trial_cell_holds: || true,
    boot_core_receives: || true,
};

/// The store of a node whose app runs from `running` (`slot::NONE`: none
/// runs).
fn store(f: &Cut, running: u8) -> Store<Cut> {
    Store::over(f.clone(), &Unwatched, 0, REGIONS, running, CHIP)
}

/// Ed25519 with its answers remembered: the search checks the same few
/// headers again and again.
struct Verifier(Ed25519Compact);

thread_local! {
    static SEEN: RefCell<HashMap<u64, bool>> = RefCell::new(HashMap::new());
}

impl SigVerifier for Verifier {
    fn key_id(&self) -> [u8; 8] {
        self.0.key_id()
    }
    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        let mut h = DefaultHasher::new();
        (domain, msg, sig).hash(&mut h);
        let k = h.finish();
        if let Some(v) = SEEN.with(|s| s.borrow().get(&k).copied()) {
            return v;
        }
        let v = self.0.verify(domain, msg, sig);
        SEEN.with(|s| s.borrow_mut().insert(k, v));
        v
    }
}

fn verifier() -> Verifier {
    Verifier(Ed25519Compact::new(*KeyPair::from_seed(Seed::new(SEED)).pk))
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

fn chain() -> Chain {
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        // Leaked: a few bytes per agent the search starts.
        Box::leak(Box::new(Published::new())),
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 8000)),
    )
}

fn config() -> Config {
    Config {
        mcu_id: MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag(TAG),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    }
}

type NodeAgent = Agent<Store<Cut>, Chain, Verifier>;

/// The image of `kind_` for `slot` with minor version `minor`.
fn image(kind_: u8, slot: u8, minor: u8) -> Vec<u8> {
    let base = match kind_ {
        image_kind::BOOTCORE => BL_SLOT[usize::from(slot)],
        _ => APP_PLACES[usize::from(slot)].header,
    };
    signed(kind_, slot, base, minor, BODY)
}

/// The digests of every broken image the search can write.
fn broken() -> HashSet<[u8; 16]> {
    let mut set = HashSet::new();
    for k in [image_kind::APP, image_kind::BOOTCORE] {
        for slot in 0..2u8 {
            if k == image_kind::BOOTCORE && usize::from(slot) >= BL_SLOT.len() {
                continue;
            }
            set.insert(head(&image(k, slot, BROKEN), slot).digest16);
        }
    }
    set
}

/// What the node runs between steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Running {
    /// Off, or reset and not yet booted.
    Down,
    /// A good app, which answers the host, running from APP slot `slot`.
    App { slot: u8 },
    /// Something that does not answer: a broken app, or a boot core with
    /// no app to start.
    Silent,
}

impl Running {
    /// The APP slot the app runs from, `slot::NONE` when none answers.
    fn slot(self) -> u8 {
        match self {
            Running::App { slot } => slot,
            _ => slot::NONE,
        }
    }
}

#[derive(Clone)]
struct Node {
    f: Cut,
    cell: [u32; 2],
    running: Running,
}

impl Node {
    fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.f.mem.borrow().hash(&mut h);
        (self.cell, self.running).hash(&mut h);
        h.finish()
    }

    fn copy(&self) -> Node {
        Node {
            f: self.f.copy(),
            cell: self.cell,
            running: self.running,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// The host writes an app into the slot the app does not run from and
    /// sets it pending (`good`: not broken).
    App { good: bool },
    /// The host writes a boot core into the slot the agent takes it into
    /// and sets it pending.
    Core { good: bool },
    /// The host confirms the app on trial.
    Confirm,
    /// A reset, then stage0, the boot core and the app as far as they go.
    Boot,
    /// Power lost with no flash operation under way.
    PowerLoss,
}

const OPS: [Op; 7] = [
    Op::App { good: true },
    Op::App { good: false },
    Op::Core { good: true },
    Op::Core { good: false },
    Op::Confirm,
    Op::Boot,
    Op::PowerLoss,
];

/// A fault in a step: the flash operation `at` lands (`half`: half of it),
/// then the power goes (`keep`: a reset, which keeps retained memory). Or,
/// with `stay`, the flash operation `at` alone fails (`half`: half of it
/// lands) and answers an error, the power stays and the step goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fault {
    at: usize,
    half: bool,
    keep: bool,
    stay: bool,
}

#[derive(Debug)]
struct Broke(String);

fn agent(n: &Node) -> Option<NodeAgent> {
    let store = store(&n.f, n.running.slot());
    Some(Agent::new(config(), verifier(), store, chain()))
}

thread_local! {
    static SEQ: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn call(a: &mut NodeAgent, cmd: Cmd, payload: &[u8]) -> Status {
    let mut buf = vec![0u8; 300];
    // A new sequence number per request: a repeated one is a retry.
    let seq = SEQ.with(|s| {
        s.set(s.get().wrapping_add(1));
        s.get()
    });
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut buf)
    .unwrap();
    let Some(reply) = a.handle(&buf[..n]) else {
        return Status::State;
    };
    let (f, _) = Frame::decode(reply.frame, 300).unwrap();
    split_status(f.payload).unwrap().0
}

fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 300];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The host's write of `img` into `t`/`slot` and its SET_PENDING; stops at
/// the first refusal.
fn write(a: &mut NodeAgent, t: u8, slot: u8, img: &[u8]) {
    let begin = XferBegin {
        target: t,
        slot,
        total_len: img.len() as u32,
        hdr_len: HEADER_SIZE as u16,
    };
    if call(a, Cmd::XferBegin, &wire(|b| begin.encode(b))) != Status::Ok {
        return;
    }
    let (hdr, body) = img.split_at(HEADER_SIZE);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, data) in part.chunks(CHUNK).enumerate() {
            let c = XferChunk {
                offset: (i * CHUNK) as u32,
                data,
            };
            if call(a, cmd, &wire(|b| c.encode(b))) != Status::Ok {
                return;
            }
        }
    }
    if call(a, Cmd::XferEnd, &[]) != Status::Ok {
        return;
    }
    let p = SetPending { target: t, slot };
    call(a, Cmd::SetPending, &wire(|b| p.encode(b)));
}

/// The slot a new image of `t` goes into: the one the agent does not run
/// from (the only one for a single boot core slot).
fn idle(n: &Node, t: u8) -> u8 {
    if t == target::BOOTCORE && BL_SLOT.len() == 1 {
        return 0;
    }
    let st = store(&n.f, n.running.slot()).target_status(t);
    match st {
        Some(st) if st.active < 2 => 1 - st.active,
        _ => 1,
    }
}

fn minor_of(f: &mut Cut, base: u32) -> Option<u8> {
    let mut raw = [0u8; HEADER_SIZE];
    dagon_port::Flash::read(f, base, &mut raw).ok()?;
    ImageHeader::parse(&raw).ok().map(|h| h.version.minor())
}

/// The apps stage0 starts the confirmed one of (`bl::APPS`) in a build
/// for this node: its tag comes from `DAGON_TARGET_TAG` at build time,
/// which a test build lacks.
const APPS: Apps<'static> = Apps {
    log: bl::APPS.log,
    slots: bl::APPS.slots,
    target_tag: target_tag(TAG),
    entry: bl::APPS.entry,
};

const BOOT: BootConfig = BootConfig {
    mcu_id: MCU_ID,
    target_tag: target_tag(TAG),
    max_attempts: TRIAL_ATTEMPTS,
};

/// What the node runs once it starts APP slot `slot`.
fn started(n: &mut Node, slot: u8) -> Running {
    if minor_of(&mut n.f, APP_PLACES[usize::from(slot)].header) != Some(BROKEN) {
        Running::App { slot }
    } else {
        Running::Silent
    }
}

/// One boot: stage0, then the boot core it starts (its hand-over check,
/// its app decision, then its settle), then the app; a core that must
/// reset resets (stage0 counts the start). Stops where the power goes.
fn boot(n: &mut Node) -> Result<(), Broke> {
    let v = verifier();
    for _ in 0..RESETS {
        if !n.f.powered() {
            return Ok(());
        }
        let started_ = bootcore::start(
            &mut n.f.clone(),
            &bl::STAGE0,
            &APPS,
            n.cell,
            &v,
            &mut bootcore::NoDiag,
        );
        let start = match started_ {
            Ok(Boot::Core(s)) => s,
            Ok(Boot::App { slot, .. }) => {
                n.running = started(n, slot);
                if matches!(n.running, Running::App { .. }) {
                    // The app's start (its agent makes the logs' room).
                    let _ = agent(n);
                }
                return Ok(());
            }
            Err(no) => return Err(Broke(format!("N1: stage0 starts nothing: {no:?}"))),
        };
        n.cell = start.cell;
        let core_good = minor_of(&mut n.f, BL_SLOT[usize::from(start.slot)]) != Some(BROKEN);
        // The host asks for no update mode: the core finds no hand-over.
        if bl::handed_over(&mut n.f) {
            return Err(Broke("the boot core finds a hand-over nobody made".into()));
        }
        let loaded = match Log::load(&mut n.f, &APP_LOG_LAYOUT) {
            Err(LogError::NoValidFace) => rebuild(&mut n.f, &APP_LOG_LAYOUT, &APP_SLOTS, &BOOT, &v),
            other => other,
        };
        let decision = match loaded {
            Ok(mut l) => decide(&mut n.f, &mut l, &APP_SLOTS, &BOOT, &v),
            Err(_) => Decision::NoImage,
        };
        if !n.f.powered() {
            return Ok(());
        }
        let healthy = core_good && matches!(decision, Decision::Boot { .. });
        match bl::settle_core(&mut n.f, start.slot, (n.cell, true), healthy) {
            Ok(Settled::Nothing) => {}
            Ok(Settled::Confirmed | Settled::RolledBack) => {
                if n.f.powered() {
                    n.cell = CLEARED;
                }
            }
            Err(MustReset) => continue,
        }
        if !n.f.powered() {
            return Ok(());
        }
        n.running = match decision {
            Decision::Boot { slot, .. } => started(n, slot),
            Decision::NoImage => Running::Silent,
        };
        if matches!(n.running, Running::App { .. }) {
            // The app's start (its agent makes the logs' room).
            let _ = agent(n);
        }
        return Ok(());
    }
    Err(Broke("a boot core resets for ever".into()))
}

/// Runs `op` on `n`; `None` when it cannot happen in this state.
fn step(n: &mut Node, op: Op) -> Option<Result<(), Broke>> {
    match op {
        Op::App { good } | Op::Core { good } => {
            if !matches!(n.running, Running::App { .. }) {
                return None;
            }
            let t = if matches!(op, Op::App { .. }) {
                target::APP
            } else {
                target::BOOTCORE
            };
            let slot = idle(n, t);
            let mut a = agent(n)?;
            let k = if t == target::APP {
                image_kind::APP
            } else {
                image_kind::BOOTCORE
            };
            let minor = if good { 1 + slot } else { BROKEN };
            write(&mut a, t, slot, &image(k, slot, minor));
        }
        Op::Confirm => {
            let Running::App { slot } = n.running else {
                return None;
            };
            let mut a = agent(n)?;
            if !a.images().on_trial() {
                return None;
            }
            let active = a.images().active();
            if active != Some(slot) {
                return Some(Err(Broke(format!(
                    "the agent takes slot {active:?} for the running one, {slot}"
                ))));
            }
            call(
                &mut a,
                Cmd::Confirm,
                &wire(|b| encode_confirm(target::APP, b)),
            );
        }
        Op::Boot => {
            n.running = Running::Down;
            return Some(boot(n));
        }
        Op::PowerLoss => {
            if n.running == Running::Down && n.cell == CLEARED {
                return None;
            }
            n.running = Running::Down;
            n.cell = CLEARED;
        }
    }
    Some(Ok(()))
}

/// Flash operations `op` makes from `n` with no fault.
fn ops_of(n: &Node, op: Op) -> Option<usize> {
    let mut m = n.copy();
    let _ = step(&mut m, op)?;
    let k = *m.f.ops.borrow();
    Some(k)
}

/// What `run` leaves: the node, the step's result, and what the cut
/// flash operation was.
type Ran = (Node, Result<(), Broke>, Option<Site>);

/// `op` from `n`, cut by `fault`.
fn run(n: &Node, op: Op, fault: Option<Fault>) -> Option<Ran> {
    let mut m = n.copy();
    match fault {
        Some(x) if x.stay => m.f.fail = Some((x.at, x.half)),
        _ => m.f.cut = fault.map(|x| (x.at, x.half)),
    }
    let r = step(&mut m, op)?;
    let mut site = None;
    if let Some(x) = fault {
        if !x.stay {
            m.running = Running::Down;
            if !x.keep {
                m.cell = CLEARED;
            }
        }
        site = m.f.site(x.at);
    }
    let f = m.f.copy();
    Some((
        Node {
            f,
            cell: m.cell,
            running: m.running,
        },
        r,
        site,
    ))
}

fn log_state(n: &Node) -> (LogState, LogState) {
    let app = Log::load(&mut n.f.clone(), &APP_LOG_LAYOUT).map_or(Default::default(), |l| l.state);
    let core = Log::load(&mut n.f.clone(), &BL_LOG_LAYOUT).map_or(Default::default(), |l| l.core);
    (app, core)
}

/// N2 and N3.
fn holds(n: &Node, broken: &HashSet<[u8; 16]>) -> Result<(), Broke> {
    let (app, core) = log_state(n);
    let Some(c) = app.confirmed else {
        return Err(Broke("N3: no app confirmed".into()));
    };
    if broken.contains(&c.digest16) {
        return Err(Broke("N2: a broken app is confirmed".into()));
    }
    if core.confirmed.is_some_and(|c| broken.contains(&c.digest16)) {
        return Err(Broke("N2: a broken boot core is confirmed".into()));
    }
    Ok(())
}

/// N4: boots with no fault end at a good app with no trial open.
fn settles(n: &Node) -> Result<(), Broke> {
    let mut m = n.copy();
    for _ in 0..SETTLE_BOOTS {
        boot(&mut m)?;
        let (app, core) = log_state(&m);
        // With one boot core slot, a cut core write or a new core that
        // fails leaves no core to start: stage0 starts the confirmed app,
        // which writes the core again (the limitations page). Until then
        // a pending core stays, and so does a pending app no core has
        // tried; no app trial is open (stage0 starts no app under one).
        let settled = if BL_SLOT.len() == 1 {
            !app.on_trial()
        } else {
            app.pending.is_none() && core.pending.is_none()
        };
        if matches!(m.running, Running::App { .. }) && settled {
            return Ok(());
        }
    }
    let (app, core) = log_state(&m);
    Err(Broke(format!(
        "N4: no settled good app after {SETTLE_BOOTS} boots: {:?}, app {app:?}, core {core:?}",
        m.running
    )))
}

/// Core A and app A installed, the app running.
fn installed_node() -> Node {
    let mut n = Node {
        f: installed(),
        cell: CLEARED,
        running: Running::Down,
    };
    boot(&mut n).unwrap();
    assert_eq!(n.running, Running::App { slot: 0 });
    n.f = n.f.copy();
    n
}

/// How far the search goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Bound {
    /// Steps in a sequence.
    steps: u8,
    /// Images written (App and Core steps).
    writes: u8,
    /// Faults inside steps.
    faults: u8,
}

/// The steps that led to a state.
type Path = Vec<(Op, Option<Fault>)>;

/// The search: breadth first over distinct states from `start`; returns
/// the states seen and the flash operations it cut.
fn explore(start: Node, bound: Bound, kind: Faults) -> (usize, Coverage) {
    let broken = broken();
    let mut covered = Coverage::default();
    let mut seen: HashSet<(u64, u8, u8, u8)> = HashSet::new();
    let at = |n: &Node, b: Bound| (n.key(), b.steps, b.writes, b.faults);
    let mut settled: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<(Node, Bound, Path)> = VecDeque::new();
    seen.insert(at(&start, bound));
    queue.push_back((start, bound, Vec::new()));
    while let Some((n, left, path)) = queue.pop_front() {
        let fail = |e: Broke| -> ! {
            panic!("{} after {path:#?}", e.0);
        };
        holds(&n, &broken).unwrap_or_else(|e| fail(e));
        if settled.insert(n.key()) {
            settles(&n).unwrap_or_else(|e| fail(e));
        }
        if left.steps == 0 {
            continue;
        }
        for op in OPS {
            let writes = matches!(op, Op::App { .. } | Op::Core { .. }) as u8;
            if writes > left.writes {
                continue;
            }
            let Some(count) = ops_of(&n, op) else {
                continue;
            };
            let mut faults = vec![None];
            if left.faults > 0 {
                for at in 0..count {
                    for half in [false, true] {
                        match kind {
                            Faults::PowerCuts => {
                                for keep in [false, true] {
                                    faults.push(Some(Fault {
                                        at,
                                        half,
                                        keep,
                                        stay: false,
                                    }));
                                }
                            }
                            Faults::FailedOperations => faults.push(Some(Fault {
                                at,
                                half,
                                keep: true,
                                stay: true,
                            })),
                        }
                    }
                }
            }
            for fault in faults {
                let Some((m, r, site)) = run(&n, op, fault) else {
                    continue;
                };
                if let Some(x) = fault.filter(|x| !x.stay) {
                    let site = site.expect("the cut operation was asked for");
                    covered.add(
                        site,
                        Way {
                            half: x.half,
                            keep: x.keep,
                        },
                    );
                }
                let mut p = path.clone();
                p.push((op, fault));
                if let Err(e) = r {
                    panic!("{} after {p:#?}", e.0);
                }
                let next = Bound {
                    steps: left.steps - 1,
                    writes: left.writes - writes,
                    faults: left.faults - fault.is_some() as u8,
                };
                if seen.insert(at(&m, next)) {
                    queue.push_back((m, next, p));
                }
            }
        }
    }
    (seen.len(), covered)
}

/// What goes wrong in a search's steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Faults {
    /// The power goes at a flash operation.
    PowerCuts,
    /// One flash operation fails (the part answers an error; a worn or
    /// disturbed sector) and the node runs on.
    FailedOperations,
}

/// The search's bound: `DAGON_EXPLORE_STEPS` / `_WRITES` / `_FAULTS`
/// override it (a longer local run).
fn bound() -> Bound {
    let get = |k: &str, d: u8| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    Bound {
        steps: get("DAGON_EXPLORE_STEPS", 6),
        writes: get("DAGON_EXPLORE_WRITES", 2),
        faults: get("DAGON_EXPLORE_FAULTS", 1),
    }
}

/// [`installed_node`] with both logs' faces filled (CONFIRMED records of
/// the running images) until they take just the trials' room: the next
/// record leaves them short, so the search compacts each log at each kind
/// of step and with each record a compaction can carry.
fn short_log_node() -> Node {
    use dagon_format::statelog::kind::CONFIRMED;
    let n = installed_node();
    let room = dagon_core::log::trial_room(TRIAL_ATTEMPTS);
    let mut f = n.f.clone();
    let app = dagon_mcu_rp2040::image::head_ref(&mut f, APP_PLACES[0].header, 0).unwrap();
    let mut log = Log::load(&mut f, &APP_LOG_LAYOUT).unwrap();
    while log.room() > room {
        log.append(&mut f, CONFIRMED, app, 0).unwrap();
    }
    let core = dagon_mcu_rp2040::image::head_ref(&mut f, BL_SLOT[0], 0).unwrap();
    let mut log = Log::load(&mut f, &BL_LOG_LAYOUT).unwrap();
    while log.room() > room {
        log.append_core(&mut f, CONFIRMED, core, 0).unwrap();
    }
    n
}

/// Every flash operation the node makes outside its first install: each
/// slot's erase, body and header; each record the agent, the boot core
/// and its settle append to either log; with `compaction`, each step of a
/// compaction of each log with each record it can carry (a torn record
/// ends the face's room, so a compaction can follow from any start).
fn inventory(compaction: bool) -> Vec<Site> {
    use dagon_format::statelog::kind::*;
    let mut v = Vec::new();
    for slot in 0..APP_PLACES.len() as u8 {
        v.extend(slot_sites(target::APP, slot));
    }
    for slot in 0..BL_SLOT.len() as u8 {
        v.extend(slot_sites(target::BOOTCORE, slot));
    }
    for k in [SLOT_PENDING, BOOT_ATTEMPT, CONFIRMED, ROLLBACK] {
        v.push(record(target::APP, k));
    }
    for k in [SLOT_PENDING, CONFIRMED, ROLLBACK] {
        v.push(record(target::BOOTCORE, k));
    }
    if compaction {
        v.extend(compaction_sites(
            target::APP,
            &[
                (target::APP, SLOT_PENDING),
                (target::APP, ROLLBACK),
                (target::APP, SNAPSHOT),
                // An open APP trial carries its attempts one record each.
                (target::APP, BOOT_ATTEMPT),
            ],
        ));
        v.extend(compaction_sites(
            target::BOOTCORE,
            &[
                (target::BOOTCORE, SLOT_PENDING),
                (target::BOOTCORE, ROLLBACK),
                (target::BOOTCORE, SNAPSHOT),
            ],
        ));
    }
    v
}

/// Searches from `node`, checks it cut every flash operation of the
/// inventory every way (the compactions' only when `compaction`: the
/// start runs the logs short) and none outside it, and writes the report
/// section `name`.
fn search(node: Node, compaction: bool, name: &str, title: &str) {
    let b = bound();
    let (n, c) = explore(node, b, Faults::PowerCuts);
    eprintln!("{title}: {n} states, {} cuts", c.cuts());
    assert!(n > 100, "{n} states");
    c.check(&inventory(compaction), &inventory(true), &ALL_WAYS);
    Coverage::report(
        name,
        &format!(
            "## {title}\n\n\
             探索の範囲: 手順 {} 回、イメージの書込 {} 回、手順内の電断 {} 回まで。\
             状態 {n} 件、電断 {} 件、復旧失敗 0 件。\n\n{}",
            b.steps,
            b.writes,
            b.faults,
            c.cuts(),
            c.table(&ALL_WAYS)
        ),
    );
}

#[test]
fn every_state_of_a_node_boots_a_good_app() {
    search(
        installed_node(),
        false,
        "rp2040-explore",
        "RP2040: アプリのエージェントによる更新（初回書込の直後から）",
    );
}

#[test]
fn every_state_of_a_node_with_short_logs_boots_a_good_app() {
    search(
        short_log_node(),
        true,
        "rp2040-explore-short-logs",
        "RP2040: アプリのエージェントによる更新（両ログの面の残量が試行の予約量だけの状態から）",
    );
}

/// A flash operation that fails with the power on (the part answers an
/// error for one erase or write; half of it may have landed) leaves the
/// node as safe as a power cut does: from every state such a failure
/// reaches, N1 to N4 hold. One failure per history, at every flash
/// operation of every step.
#[test]
fn a_failed_flash_operation_leaves_a_node_a_good_app() {
    for node in [installed_node(), short_log_node()] {
        let b = Bound {
            writes: bound().writes.min(1),
            ..bound()
        };
        let (n, _) = explore(node, b, Faults::FailedOperations);
        eprintln!("{n} states with a failed flash operation");
        assert!(n > 100, "{n} states");
    }
}

/// Runs `path` from the installed node; every step happens.
fn walk(path: &[(Op, Option<Fault>)]) -> Node {
    let mut n = installed_node();
    for &(op, fault) in path {
        let (m, r, _) = run(&n, op, fault).unwrap_or_else(|| panic!("{op:?} cannot happen"));
        r.unwrap_or_else(|e| panic!("{op:?}: {}", e.0));
        n = m;
    }
    n
}

const CUT_FIRST: Option<Fault> = Some(Fault {
    at: 0,
    half: true,
    keep: false,
    stay: false,
});

/// The F401's finding on this MCU's store: a broken app's second
/// BOOT_ATTEMPT torn by a power loss; the boot core cannot record the
/// next attempt, so it starts the confirmed app, whose agent must take
/// the slot the boot core started for the running one, not the log's
/// pending image, or CONFIRM confirms the broken image that never ran.
#[test]
fn a_confirmed_app_started_past_a_torn_attempt_is_not_on_trial() {
    let n = walk(&[
        (Op::App { good: false }, None),
        (Op::Boot, None),
        (Op::Boot, CUT_FIRST),
        (Op::Boot, None),
    ]);
    assert_eq!(n.running, Running::App { slot: 0 });
    let a = agent(&n).unwrap();
    assert_eq!(a.images().active(), Some(0), "the app runs from slot 0");
    assert!(!a.images().on_trial(), "the confirmed app is not on trial");
}

/// The boot core log lost (both faces erased) with a core on trial: no
/// core is named, so stage0 starts the confirmed app, and the node still
/// settles at a good app.
#[test]
fn a_node_whose_boot_core_log_is_lost_runs_the_confirmed_app() {
    let n = walk(&[(Op::Core { good: true }, None)]);
    let mut f = n.f.clone();
    for face in BL_LOG {
        dagon_port::Flash::erase(&mut f, face, LOG_FACE_SIZE).unwrap();
    }
    let mut m = n.copy();
    boot(&mut m).unwrap_or_else(|e| panic!("{}", e.0));
    assert_eq!(m.running, Running::App { slot: 0 });
}
