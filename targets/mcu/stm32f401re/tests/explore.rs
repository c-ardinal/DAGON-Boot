//! Exhaustive exploration of a whole node's boot state: every sequence of
//! what the host and the node do (an app or a boot core written through
//! the app's agent and set pending, a CONFIRM, a boot, a power loss) up to
//! a bound, with a reset or a power loss after any flash operation of any
//! of them, over the F401's production code: stage0's decision
//! (`bootcore::start`, the confirmed app included), the boot core's app
//! decision (`boot::decide` as `BootRun` runs it) and its settle
//! (`bootcore::settle_log`), the agent (`dagon_agent::Agent`) over the
//! store (`KindSStore`), the trial cell in retained memory (kept by a
//! reset, cleared by a power loss). Both boot core layouts are explored:
//! two slots (the F401's) and one slot written in place.
//!
//! States are compared by their flash and retained contents, so each
//! distinct state is expanded once. After every step these hold:
//!
//! - N1: stage0 always starts something (a boot core, or the confirmed
//!   app when no core may start).
//! - N2: no broken image (an app that never answers, a boot core that
//!   fails its own check) is ever confirmed.
//! - N3: an app stays confirmed in the log.
//! - N4: from every state, boots with no further fault end within a
//!   bound at a good app that runs with no trial open (a good app on
//!   trial that the host does not confirm is rolled back too).
//!
//! Unlike the random worlds of `dagon-retained-model`, this search visits
//! every state within its bound; unlike `core_update.rs`, it combines
//! images, faults and trials.

mod cut;

use cut::*;
use dagon_agent::{Agent, Config, SlotStore};
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::bootcore::{self, Apps, Boot, Cell, MustReset, Settled, Stage0, CLEARED};
use dagon_core::log::{Log, LogState};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::slot;
use dagon_format::SigVerifier;
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::{KindSStore, ERASE_REGIONS, TRIAL_ATTEMPTS};
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
const TAG: &str = "seg-f401-7seg";
/// Boots with no fault after which a node has settled.
const SETTLE_BOOTS: usize = 3 * (TRIAL_ATTEMPTS as usize + 2);
/// Resets one boot may take (a core that must reset, stage0 counting).
const RESETS: usize = 2 * (bootcore::MAX_COUNT as usize + 1);

static ONE_CORE: [(u32, u32); 1] = [CORE_SLOTS[0]];

/// The boot core layout explored.
#[derive(Clone, Copy, Debug)]
struct Cores(&'static [(u32, u32)]);

impl Cores {
    fn stage0(self) -> Stage0<'static> {
        Stage0 {
            log: &LOG,
            slots: if self.0.len() == 1 {
                &CORE_PLACES[..1]
            } else {
                &CORE_PLACES
            },
            max: TRIAL_ATTEMPTS,
            mcu_id: MCU_ID,
        }
    }
    /// The store of a node whose app runs from `running` (`slot::NONE`:
    /// none runs).
    fn store(self, f: &Cut, running: u8) -> Option<KindSStore<Cut>> {
        KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS, running)
            .ok()
            .map(|s| s.with_cores(self.0, true))
    }
}

const APPS: Apps<'static> = Apps {
    log: &LOG,
    slots: SLOTS.slots,
    target_tag: target_tag(TAG),
    entry: dagon_mcu_common::vector::entry,
};

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
        max_payload: max_payload::I2C as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    }
}

type NodeAgent = Agent<KindSStore<Cut>, Chain, Verifier>;

/// The image of `kind_` for `slot` with minor version `minor`.
fn image(kind_: u8, cores: Cores, slot: u8, minor: u8) -> Vec<u8> {
    let base = match kind_ {
        image_kind::BOOTCORE => cores.0[usize::from(slot)].0,
        _ => SLOTS.slots[usize::from(slot)].header,
    };
    signed(kind_, slot, base, minor, BODY)
}

/// The digests of every broken image the search can write.
fn broken(cores: Cores) -> HashSet<[u8; 16]> {
    let mut set = HashSet::new();
    for k in [image_kind::APP, image_kind::BOOTCORE] {
        for slot in 0..2u8 {
            if k == image_kind::BOOTCORE && usize::from(slot) >= cores.0.len() {
                continue;
            }
            set.insert(head(&image(k, cores, slot, BROKEN), slot).digest16);
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
/// then the power goes (`keep`: a reset, which keeps retained memory).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fault {
    at: usize,
    half: bool,
    keep: bool,
}

#[derive(Debug)]
struct Broke(String);

fn agent(cores: Cores, n: &Node) -> Option<NodeAgent> {
    let store = cores.store(&n.f, n.running.slot())?;
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
fn idle(n: &Node, t: u8, cores: Cores) -> u8 {
    if t == target::BOOTCORE && cores.0.len() == 1 {
        return 0;
    }
    let st = cores
        .store(&n.f, n.running.slot())
        .and_then(|mut s| s.target_status(t));
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

/// One boot: stage0, then the boot core it starts (its app decision, then
/// its settle), then the app; a core that must reset resets (stage0
/// counts the start). Stops where the power goes.
fn boot(cores: Cores, n: &mut Node) -> Result<(), Broke> {
    let v = verifier();
    for _ in 0..RESETS {
        if !n.f.powered() {
            return Ok(());
        }
        let started = bootcore::start(
            &mut n.f.clone(),
            &cores.stage0(),
            &APPS,
            n.cell,
            &v,
            &mut bootcore::NoDiag,
        );
        let start = match started {
            Ok(Boot::Core(s)) => s,
            Ok(Boot::App { slot, .. }) => {
                let good =
                    minor_of(&mut n.f, SLOTS.slots[usize::from(slot)].header) != Some(BROKEN);
                n.running = if good {
                    Running::App { slot }
                } else {
                    Running::Silent
                };
                if good {
                    // The app's start (its agent makes the log's room).
                    let _ = agent(cores, n);
                }
                return Ok(());
            }
            Err(no) => return Err(Broke(format!("N1: stage0 starts nothing: {no:?}"))),
        };
        n.cell = start.cell;
        let core_good = minor_of(&mut n.f, cores.0[usize::from(start.slot)].0) != Some(BROKEN);
        let cfg = BootConfig {
            mcu_id: MCU_ID,
            target_tag: target_tag(TAG),
            max_attempts: TRIAL_ATTEMPTS,
        };
        let decision = match Log::load(&mut n.f, &LOG) {
            Ok(mut l) => decide(&mut n.f, &mut l, &SLOTS, &cfg, &v),
            Err(_) => Decision::NoImage,
        };
        if !n.f.powered() {
            return Ok(());
        }
        let healthy = core_good && matches!(decision, Decision::Boot { .. });
        let cell = Cell {
            value: n.cell,
            max: TRIAL_ATTEMPTS,
            holds: true,
        };
        let running = (start.slot, cores.0[usize::from(start.slot)].0);
        match bootcore::settle_log(&mut n.f, &LOG, running, cell, healthy) {
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
            Decision::Boot { slot, .. } => {
                if minor_of(&mut n.f, SLOTS.slots[usize::from(slot)].header) != Some(BROKEN) {
                    Running::App { slot }
                } else {
                    Running::Silent
                }
            }
            Decision::NoImage => Running::Silent,
        };
        if matches!(n.running, Running::App { .. }) {
            // The app's start (its agent makes the log's room).
            let _ = agent(cores, n);
        }
        return Ok(());
    }
    Err(Broke("a boot core resets for ever".into()))
}

/// Runs `op` on `n`; `None` when it cannot happen in this state.
fn step(cores: Cores, n: &mut Node, op: Op) -> Option<Result<(), Broke>> {
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
            let slot = idle(n, t, cores);
            let mut a = agent(cores, n)?;
            let k = if t == target::APP {
                image_kind::APP
            } else {
                image_kind::BOOTCORE
            };
            let minor = if good { 1 + slot } else { BROKEN };
            write(&mut a, t, slot, &image(k, cores, slot, minor));
        }
        Op::Confirm => {
            let Running::App { slot } = n.running else {
                return None;
            };
            let mut a = agent(cores, n)?;
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
            return Some(boot(cores, n));
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
fn ops_of(cores: Cores, n: &Node, op: Op) -> Option<usize> {
    let mut m = n.copy();
    let _ = step(cores, &mut m, op)?;
    let k = *m.f.ops.borrow();
    Some(k)
}

/// What `run` leaves: the node, the step's result, and what the cut
/// flash operation was.
type Ran = (Node, Result<(), Broke>, Option<Site>);

/// `op` from `n`, cut by `fault`.
fn run(cores: Cores, n: &Node, op: Op, fault: Option<Fault>) -> Option<Ran> {
    let mut m = n.copy();
    m.f.cut = fault.map(|x| (x.at, x.half));
    let r = step(cores, &mut m, op)?;
    let mut site = None;
    if let Some(x) = fault {
        m.running = Running::Down;
        if !x.keep {
            m.cell = CLEARED;
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
    match Log::load(&mut n.f.clone(), &LOG) {
        Ok(l) => (l.state, l.core),
        Err(_) => Default::default(),
    }
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
fn settles(cores: Cores, n: &Node) -> Result<(), Broke> {
    let mut m = n.copy();
    for _ in 0..SETTLE_BOOTS {
        boot(cores, &mut m)?;
        let (app, core) = log_state(&m);
        // With one boot core slot, a cut core write or a new core that
        // fails leaves no core to start: stage0 starts the confirmed app,
        // which writes the core again (the limitations page). Until then
        // a pending core stays, and so does a pending app no core has
        // tried; no app trial is open (stage0 starts no app under one).
        let settled = if cores.0.len() == 1 {
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
fn installed_node(cores: Cores) -> Node {
    let f = Cut::new();
    let core = image(image_kind::BOOTCORE, cores, 0, 0);
    let app = image(image_kind::APP, cores, 0, 0);
    let mut g = f.clone();
    dagon_port::Flash::write(&mut g, cores.0[0].0, &core).unwrap();
    dagon_port::Flash::write(&mut g, SLOTS.slots[0].header, &app).unwrap();
    dagon_port::Flash::write(
        &mut g,
        LOG.faces[0],
        &dagon_format::statelog::FaceHeader { generation: 1 }.to_bytes(),
    )
    .unwrap();
    let mut log = Log::load(&mut g, &LOG).unwrap();
    for k in [
        dagon_format::statelog::kind::PROVISION,
        dagon_format::statelog::kind::CONFIRMED,
    ] {
        log.append_core(&mut g, k, head(&core, 0), 0).unwrap();
        log.append(&mut g, k, head(&app, 0), 0).unwrap();
    }
    let mut n = Node {
        f: f.copy(),
        cell: CLEARED,
        running: Running::Down,
    };
    boot(cores, &mut n).unwrap();
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

/// A node as the queue keeps it: the flash run-length coded (mostly
/// erased), so a wide search fits in memory.
struct Packed {
    runs: Vec<(u8, u32)>,
    cell: [u32; 2],
    running: Running,
}

impl Packed {
    fn of(n: &Node) -> Packed {
        let mut runs: Vec<(u8, u32)> = Vec::new();
        for &b in n.f.mem.borrow().iter() {
            match runs.last_mut() {
                Some((v, k)) if *v == b => *k += 1,
                _ => runs.push((b, 1)),
            }
        }
        Packed {
            runs,
            cell: n.cell,
            running: n.running,
        }
    }

    fn node(&self) -> Node {
        let f = Cut::new();
        {
            let mut mem = f.mem.borrow_mut();
            mem.clear();
            for &(v, k) in &self.runs {
                mem.extend(std::iter::repeat_n(v, k as usize));
            }
        }
        Node {
            f,
            cell: self.cell,
            running: self.running,
        }
    }
}

/// The search: breadth first over distinct states from `start`; returns
/// the states seen and the flash operations it cut.
fn explore(cores: Cores, start: Node, bound: Bound) -> (usize, Coverage) {
    let broken = broken(cores);
    let mut covered = Coverage::default();
    let mut seen: HashSet<(u64, u8, u8, u8)> = HashSet::new();
    let at = |n: &Node, b: Bound| (n.key(), b.steps, b.writes, b.faults);
    let mut settled: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<(Packed, Bound, Path)> = VecDeque::new();
    seen.insert(at(&start, bound));
    queue.push_back((Packed::of(&start), bound, Vec::new()));
    while let Some((packed, left, path)) = queue.pop_front() {
        let n = packed.node();
        let fail = |e: Broke| -> ! {
            panic!("{} ({cores:?}) after {path:#?}", e.0);
        };
        holds(&n, &broken).unwrap_or_else(|e| fail(e));
        if settled.insert(n.key()) {
            settles(cores, &n).unwrap_or_else(|e| fail(e));
        }
        if left.steps == 0 {
            continue;
        }
        for op in OPS {
            let writes = matches!(op, Op::App { .. } | Op::Core { .. }) as u8;
            if writes > left.writes {
                continue;
            }
            let Some(count) = ops_of(cores, &n, op) else {
                continue;
            };
            let mut faults = vec![None];
            if left.faults > 0 {
                for at in 0..count {
                    for half in [false, true] {
                        for keep in [false, true] {
                            faults.push(Some(Fault { at, half, keep }));
                        }
                    }
                }
            }
            for fault in faults {
                let Some((m, r, site)) = run(cores, &n, op, fault) else {
                    continue;
                };
                if let Some(x) = fault {
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
                    panic!("{} ({cores:?}) after {p:#?}", e.0);
                }
                let next = Bound {
                    steps: left.steps - 1,
                    writes: left.writes - writes,
                    faults: left.faults - fault.is_some() as u8,
                };
                if seen.insert(at(&m, next)) {
                    queue.push_back((Packed::of(&m), next, p));
                }
            }
        }
    }
    (seen.len(), covered)
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

/// [`installed_node`] with its log face filled (CONFIRMED records of the
/// running app) until it takes just the trials' room: the next record
/// leaves it short, so the search compacts the log at each kind of step
/// and with each record a compaction can carry.
fn short_log_node(cores: Cores) -> Node {
    let n = installed_node(cores);
    let app = image(image_kind::APP, cores, 0, 0);
    let mut f = n.f.clone();
    let mut log = Log::load(&mut f, &LOG).unwrap();
    while log.room() > dagon_core::log::trial_room(TRIAL_ATTEMPTS) {
        log.append(
            &mut f,
            dagon_format::statelog::kind::CONFIRMED,
            head(&app, 0),
            0,
        )
        .unwrap();
    }
    n
}

/// Every flash operation a node of `cores` makes outside its first
/// install: each slot's erase, body and header; each record the agent,
/// the boot core and its settle append; with `compaction`, each step of
/// a compaction with each record it carries (a torn record ends the
/// face's room, so a compaction can follow from any start). With one boot
/// core slot no core is rolled back (no other core is left to start), so
/// no core ROLLBACK is written.
fn inventory(cores: Cores, compaction: bool) -> Vec<Site> {
    use dagon_format::statelog::kind::*;
    let two = cores.0.len() == 2;
    let mut v = Vec::new();
    for slot in 0..2 {
        v.extend(slot_sites(target::APP, slot));
    }
    for slot in 0..cores.0.len() as u8 {
        v.extend(slot_sites(target::BOOTCORE, slot));
    }
    for k in [SLOT_PENDING, BOOT_ATTEMPT, CONFIRMED, ROLLBACK] {
        v.push(record(target::APP, k));
    }
    for k in [SLOT_PENDING, CONFIRMED] {
        v.push(record(target::BOOTCORE, k));
    }
    let mut carried = vec![
        (target::APP, SLOT_PENDING),
        (target::APP, ROLLBACK),
        (target::APP, SNAPSHOT),
        (target::BOOTCORE, SLOT_PENDING),
        (target::BOOTCORE, SNAPSHOT),
    ];
    if two {
        v.push(record(target::BOOTCORE, ROLLBACK));
        carried.push((target::BOOTCORE, ROLLBACK));
    }
    if compaction {
        v.extend(compaction_sites(&carried));
    }
    v
}

/// Searches `cores` from `node`, checks it cut every flash operation of
/// the inventory every way (the compaction's only when `compaction`: the
/// start runs the log short) and none outside it, and writes the report
/// section `name`.
fn search(cores: Cores, node: Node, compaction: bool, name: &str, title: &str) {
    let b = bound();
    let (n, c) = explore(cores, node, b);
    eprintln!("{title}: {n} states, {} cuts", c.cuts());
    assert!(n > 100, "{n} states");
    c.check(
        &inventory(cores, compaction),
        &inventory(cores, true),
        &ALL_WAYS,
    );
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

const TWO: Cores = Cores(&CORE_SLOTS);
const ONE: Cores = Cores(&ONE_CORE);

#[test]
fn every_state_of_a_node_with_two_boot_core_slots_boots_a_good_app() {
    search(
        TWO,
        installed_node(TWO),
        false,
        "explore-two-core-slots",
        "アプリのエージェントによる更新（ブートローダのスロット 2 個、初回書込の直後から）",
    );
}

#[test]
fn every_state_of_a_node_with_one_boot_core_slot_boots_a_good_app() {
    search(
        ONE,
        installed_node(ONE),
        false,
        "explore-one-core-slot",
        "アプリのエージェントによる更新（ブートローダのスロット 1 個、初回書込の直後から）",
    );
}

#[test]
fn every_state_of_a_node_with_two_boot_core_slots_and_a_short_log_boots_a_good_app() {
    search(
        TWO,
        short_log_node(TWO),
        true,
        "explore-two-core-slots-short-log",
        "アプリのエージェントによる更新（ブートローダのスロット 2 個、ログの面の残量が試行の予約量だけの状態から）",
    );
}

#[test]
fn every_state_of_a_node_with_one_boot_core_slot_and_a_short_log_boots_a_good_app() {
    search(
        ONE,
        short_log_node(ONE),
        true,
        "explore-one-core-slot-short-log",
        "アプリのエージェントによる更新（ブートローダのスロット 1 個、ログの面の残量が試行の予約量だけの状態から）",
    );
}

/// Runs `path` from the installed node; every step happens.
fn walk(cores: Cores, path: &[(Op, Option<Fault>)]) -> Node {
    walk_from(cores, installed_node(cores), path)
}

fn walk_from(cores: Cores, mut n: Node, path: &[(Op, Option<Fault>)]) -> Node {
    for &(op, fault) in path {
        let (m, r, _) = run(cores, &n, op, fault).unwrap_or_else(|| panic!("{op:?} cannot happen"));
        r.unwrap_or_else(|e| panic!("{op:?}: {}", e.0));
        n = m;
    }
    n
}

const CUT_FIRST: Option<Fault> = Some(Fault {
    at: 0,
    half: true,
    keep: false,
});

/// Found by the search: with one boot core slot, a new core written while
/// an app was pending started that app's trial at each of its starts, and
/// once stage0 gave the core up the open app trial left nothing to start.
/// The agent now refuses the core while an app is pending.
#[test]
fn one_boot_core_slot_takes_no_core_while_an_app_is_pending() {
    let cores = Cores(&ONE_CORE);
    let n = walk(
        cores,
        &[
            (Op::App { good: true }, None),
            (Op::Core { good: false }, None),
        ],
    );
    assert!(log_state(&n).1.pending.is_none(), "the core is refused");
    settles(cores, &n).unwrap_or_else(|e| panic!("{}", e.0));
}

/// The converse, found in review: with the only core slot left empty by
/// a cut write, stage0 starts the confirmed app; an app set pending then
/// would hold back every core write, and no core would start its trial.
/// The agent sets no app pending until the core is confirmed again.
#[test]
fn one_boot_core_slot_takes_no_app_while_the_core_is_not_confirmed() {
    let cores = Cores(&ONE_CORE);
    let n = walk(
        cores,
        &[(Op::Core { good: true }, CUT_FIRST), (Op::Boot, None)],
    );
    assert_eq!(n.running, Running::App { slot: 0 });
    let n = walk_from(cores, n, &[(Op::App { good: true }, None)]);
    assert!(log_state(&n).0.pending.is_none(), "the app is refused");
    let n = walk_from(cores, n, &[(Op::Core { good: true }, None)]);
    assert!(log_state(&n).1.pending.is_some(), "the core is taken");
    settles(cores, &n).unwrap_or_else(|e| panic!("{}", e.0));
}

/// Found by the search: the confirmed core written again into its one
/// slot and set pending, its CONFIRMED record torn by a power loss: the
/// core reset at every start (the record never lands until the app makes
/// the log's room), so the app never started. It now runs the app.
#[test]
fn a_confirmed_core_written_again_whose_record_is_torn_runs_the_app() {
    let cores = Cores(&ONE_CORE);
    let n = walk(
        cores,
        &[
            (Op::Core { good: true }, None),
            (Op::Boot, None),
            (Op::Core { good: true }, None),
            (Op::Boot, CUT_FIRST),
        ],
    );
    settles(cores, &n).unwrap_or_else(|e| panic!("{}", e.0));
}

/// Found by the search: a broken app's second BOOT_ATTEMPT torn by a power loss; the boot core
/// cannot record the next attempt, so it starts the confirmed app, whose
/// agent must take the slot the boot core started for the running one,
/// not the log's pending image, or CONFIRM confirms the broken image that
/// never ran.
#[test]
fn a_confirmed_app_started_past_a_torn_attempt_is_not_on_trial() {
    let cores = Cores(&CORE_SLOTS);
    let n = walk(
        cores,
        &[
            (Op::App { good: false }, None),
            (Op::Boot, None),
            (Op::Boot, CUT_FIRST),
            (Op::Boot, None),
        ],
    );
    assert_eq!(n.running, Running::App { slot: 0 });
    let a = agent(cores, &n).unwrap();
    assert_eq!(a.images().active(), Some(0), "the app runs from slot 0");
    assert!(!a.images().on_trial(), "the confirmed app is not on trial");
}

/// Found by the search with three image writes: with one boot core slot,
/// a good core written over a pending broken one, its SLOT_PENDING lost
/// to a power cut, left the broken core's record pending for an image no
/// longer in the slot. Stage0 gave that record up and started the
/// confirmed core from the slot, which took itself for the pending one:
/// it reset for ever when its CONFIRMED was torn, and would have
/// confirmed the broken record when it landed. A core now settles a
/// pending record only when its own header is that image.
#[test]
fn a_core_over_a_pending_one_whose_record_is_lost_settles_as_the_confirmed_core() {
    let cores = ONE;
    let broken = broken(cores);
    for half in [false, true] {
        let n = walk(
            cores,
            &[
                (Op::Core { good: true }, None),
                (Op::Boot, None),
                (Op::Core { good: false }, None),
                (
                    Op::Core { good: true },
                    Some(Fault {
                        at: 6,
                        half,
                        keep: false,
                    }),
                ),
            ],
        );
        holds(&n, &broken).unwrap_or_else(|e| panic!("half {half}: {}", e.0));
        settles(cores, &n).unwrap_or_else(|e| panic!("half {half}: {}", e.0));
        let mut m = n.copy();
        boot(cores, &mut m).unwrap_or_else(|e| panic!("half {half}: {}", e.0));
        holds(&m, &broken).unwrap_or_else(|e| panic!("half {half}: {}", e.0));
    }
}
