//! Fuse and sandbox scenarios: runs of the boot core and the app whose
//! extensions return, overrun, fault, misuse capabilities or lock the core
//! up at every boot step; resets after any byte of every fuse store; power
//! loss; image and extension replacement; host clears; bytes of another
//! fuse layout.
//!
//! Properties: P14 overrun, P15 fault, P16 capability misuse, P17 fault
//! limit, P18 bounded boot time with a service window, P19 trusted
//! verifier, P20 owner, P22 false-trip-free and complete, P23 hand-off,
//! P24 stale and upgrade, P25 versioning (foreign layout bytes are never
//! acted on: caught by P22's soundness), P26 host clear.

use crate::boot::{
    self, call, Behaviour, Decision, LogState, Manifest, Outcome, Protected, Target,
    VerifierReplacement, EXT_FAULT_LIMIT, STEPS, UNWIND_US,
};
use crate::fuse::{
    self, fuse_id, CallSite, Cell, FuseCodec, FuseRec, FuseV1, FuseV2, Mark, LAYER_APP, LAYER_BOOT,
};
use crate::harness::{Violation, World};
use crate::mem::{Cut, Mem, Survives};
use crate::rng::Rng;
use std::collections::BTreeSet;

const BOOT: usize = 0;
const APP: usize = 1;
/// The boot step at which the decision is planned, then committed.
const PLANNED: usize = 3;
const COMMITTED: usize = 4;
/// The service window polls its transport this often.
const POLL_US: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub ext: u8,
    /// Boot step (boot calls) or call order (app calls).
    pub step: u8,
    pub b: Behaviour,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// One power-on-to-reset run. Calls not listed return in 10 us.
    /// `cut`: a reset after that many fuse stores; `power`: the run ends
    /// in power loss.
    Run {
        boot: Vec<Call>,
        app: Vec<Call>,
        cut: Option<u32>,
        power: bool,
    },
    /// A new build of one extension (new artifact hash).
    ReplaceExt { app: bool, index: u8 },
    /// A new boot core image, or a new app image pending in the other slot.
    ReplaceImage { app: bool },
    /// `dagon extension reset` for the n-th disabled extension.
    HostClear { pick: u8 },
    /// Bytes of another fuse layout naming an installed call site.
    Skew {
        cell: u8,
        slot: u8,
        tripped: bool,
        pick: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub survives: Survives,
    pub trips: usize,
    pub boot_exts: u8,
    pub app_exts: u8,
    pub manifest: Manifest,
    pub trial_ms: u32,
    /// Boot extension 0 serves a transport between the steps for this long.
    pub window_ms: u32,
    pub verifier: Option<VerifierReplacement>,
    pub image_ok: [bool; 2],
}

impl Config {
    fn cells(&self) -> [Cell; 2] {
        let n = FuseV1.size() * (1 + self.trips);
        [
            Cell {
                base: 4,
                trips: self.trips,
            },
            Cell {
                base: 8 + n,
                trips: self.trips,
            },
        ]
    }

    fn mem_len(&self) -> usize {
        let a = self.cells()[APP];
        a.bytes(&FuseV1).end + 8
    }
}

struct Images {
    boot_header: [u8; 32],
    app_header: [[u8; 32]; 2],
    boot_ext: Vec<[u8; 32]>,
    app_ext: Vec<[u8; 32]>,
}

struct Sim<'a> {
    cfg: &'a Config,
    mem: Mem,
    rng: Rng,
    img: Images,
    log: LogState,
    /// Call sites that locked up since their last host clear.
    tripworthy: BTreeSet<[u8; 32]>,
    /// Lockups not yet acted on by their layer's start: (cell, id).
    pending: Vec<(usize, [u8; 32])>,
    /// Disabled at each cell's last completed start, still owed.
    owed: [BTreeSet<[u8; 32]>; 2],
    /// Boot-core trips the host asked to clear, not yet carried out.
    clearing: BTreeSet<[u8; 32]>,
    /// Call sites whose records foreign bytes destroyed: a lost clear
    /// acknowledgement may cost one later trip (a loss, never a forgery).
    disturbed: BTreeSet<[u8; 32]>,
    /// The MCU package's watchdog-reset flag, kept until acknowledged.
    watchdog: bool,
    step: usize,
}

enum End {
    Reset,
    Lockup,
}

impl<'a> Sim<'a> {
    fn fail(&self, p: &'static str, d: impl Into<String>) -> Violation {
        Violation::new(p, self.step, d)
    }

    fn ids(&self, cell: usize, app_slot: u8) -> Vec<[u8; 32]> {
        let (layer, slot, header, exts) = match cell {
            BOOT => (LAYER_BOOT, 0, self.img.boot_header, &self.img.boot_ext),
            _ => (
                LAYER_APP,
                app_slot as u32,
                self.img.app_header[app_slot as usize],
                &self.img.app_ext,
            ),
        };
        exts.iter()
            .enumerate()
            .map(|(i, a)| {
                fuse_id(&CallSite {
                    layer,
                    slot,
                    header,
                    chain: i as u32,
                    artifact: *a,
                })
            })
            .collect()
    }

    fn running_app_slot(&self) -> u8 {
        self.log.pending.or(self.log.confirmed).unwrap_or(0)
    }

    /// A completed start of `cell`: P22, P24, P26.
    fn after_start(
        &mut self,
        cell: usize,
        s: &fuse::Start,
        installed: &[[u8; 32]],
    ) -> Result<(), Violation> {
        // A verdict destroyed by foreign bytes may let a later watchdog flag
        // classify an interrupted (never returned) call as a lockup.
        let sound = |x: &[u8; 32]| self.tripworthy.contains(x) || self.disturbed.contains(x);
        if let Some(x) = s.disabled.iter().find(|x| !sound(x)) {
            return Err(self.fail(
                "P22",
                format!("disabled without a lockup: {:02x?}", &x[..4]),
            ));
        }
        for &(c, id) in &self.pending {
            let owed = !self.disturbed.contains(&id);
            if owed
                && c == cell
                && installed.contains(&id)
                && s.can_arm
                && !s.disabled.contains(&id)
            {
                return Err(self.fail("P22", "lockup not acted on by its layer's start"));
            }
        }
        if s.can_arm {
            self.pending.retain(|&(c, _)| c != cell);
        }
        for id in &self.owed[cell] {
            let owed = installed.contains(id) && !self.disturbed.contains(id);
            if owed && s.can_arm && !s.disabled.contains(id) {
                return Err(self.fail("P26", "trip cleared without a host clear"));
            }
        }
        self.owed[cell] = s.disabled.iter().copied().collect();
        if cell == BOOT {
            let done: Vec<_> = self
                .clearing
                .iter()
                .filter(|id| !s.disabled.contains(id))
                .copied()
                .collect();
            for id in done {
                self.clearing.remove(&id);
                self.tripworthy.remove(&id);
            }
        }
        let c = self.cfg.cells()[cell];
        for i in 1..=c.trips {
            let at = c.base + FuseV1.size() * i;
            let r = FuseV1.decode(&self.mem.bytes()[at..at + FuseV1.size()]);
            if let Some(r) = r.filter(|r| r.mark == Mark::Tripped && !installed.contains(&r.id)) {
                return Err(self.fail("P24", format!("stale trip kept: {:02x?}", &r.id[..4])));
            }
        }
        Ok(())
    }

    fn start(
        &mut self,
        cell: usize,
        installed: &[[u8; 32]],
    ) -> Result<Result<fuse::Start, Cut>, Violation> {
        let cells = self.cfg.cells();
        self.mem.allow(cells[cell].bytes(&FuseV1));
        match fuse::start(
            &FuseV1,
            &mut self.mem,
            &cells,
            cell,
            self.watchdog,
            installed,
        ) {
            Ok(s) => {
                self.after_start(cell, &s, installed)?;
                if s.complete {
                    self.watchdog = false;
                }
                Ok(Ok(s))
            }
            Err(c) => Ok(Err(c)),
        }
    }

    /// Runs one layer's calls; `Ok(Err(_))` ends the run.
    #[allow(clippy::too_many_arguments)]
    fn calls(
        &mut self,
        cell: usize,
        ids: &[[u8; 32]],
        s: &fuse::Start,
        plan: &[(usize, Behaviour)],
        faults: &mut [u8],
        time: &mut u32,
    ) -> Result<Result<(), End>, Violation> {
        let c = self.cfg.cells()[cell];
        let layer = [LAYER_BOOT, LAYER_APP][cell];
        for &(ext, b) in plan {
            let id = ids[ext];
            let off = !s.can_arm
                || s.disabled.contains(&id)
                || s.held.contains(&id)
                || faults[ext] >= EXT_FAULT_LIMIT;
            if off {
                continue;
            }
            if fuse::arm(&FuseV1, &mut self.mem, c, layer, id).is_err() {
                return Ok(Err(End::Reset));
            }
            let mut p = Protected::default();
            let (out, us) = call(&self.cfg.manifest, b, &mut p);
            if p != Protected::default() {
                return Err(self.fail("P15", "a call changed protected state"));
            }
            self.check_outcome(b, out)?;
            if out == Outcome::LockedUp {
                self.tripworthy.insert(id);
                self.pending.push((cell, id));
                self.watchdog = true;
                return Ok(Err(End::Lockup));
            }
            *time += us;
            if !matches!(out, Outcome::Ran { .. } | Outcome::Refused) {
                faults[ext] += 1;
            }
            if fuse::disarm(&FuseV1, &mut self.mem, c).is_err() {
                return Ok(Err(End::Reset));
            }
        }
        Ok(Ok(()))
    }

    /// P14, P15, P16: the outcome the contract assigns to each behaviour.
    fn check_outcome(&self, b: Behaviour, out: Outcome) -> Result<(), Violation> {
        let m = &self.cfg.manifest;
        let (p, want) = match b {
            Behaviour::Return { us } if us <= m.budget_us => ("P14", Outcome::Ran { us }),
            Behaviour::Return { .. } | Behaviour::Overrun => ("P14", Outcome::Overran),
            Behaviour::Access(Target::OwnRam) => ("P15", Outcome::Ran { us: 1 }),
            Behaviour::Access(Target::Window(w)) if m.windows.contains(&w) => {
                ("P16", Outcome::Ran { us: 1 })
            }
            Behaviour::Access(t) => ("P15", Outcome::Faulted(t)),
            Behaviour::BusWrite { device, register } if m.devices.contains(&(device, register)) => {
                ("P16", Outcome::Ran { us: 5 })
            }
            Behaviour::BusWrite { .. } => ("P16", Outcome::Refused),
            Behaviour::Lockup => ("P22", Outcome::LockedUp),
        };
        if out != want {
            return Err(self.fail(p, format!("{b:?} gave {out:?}")));
        }
        Ok(())
    }

    fn boot_plan(&self, boot: &[Call], step: usize) -> Vec<(usize, Behaviour)> {
        (0..self.cfg.boot_exts as usize)
            .map(|e| {
                let b = boot
                    .iter()
                    .find(|c| c.ext as usize == e && c.step as usize == step)
                    .map_or(Behaviour::Return { us: 10 }, |c| c.b);
                (e, b)
            })
            .collect()
    }

    /// Boot core run: fuse start, extensions at every step, the decision.
    fn boot_core(&mut self, boot: &[Call]) -> Result<Result<Decision, End>, Violation> {
        let app_cell = self.cfg.cells()[APP].bytes(&FuseV1);
        let app_before = self.mem.bytes()[app_cell.clone()].to_vec();
        let ids = self.ids(BOOT, 0);
        let s = match self.start(BOOT, &ids)? {
            Ok(s) => s,
            Err(_) => return Ok(Err(End::Reset)),
        };
        let log_before = self.log;
        let mut faults = vec![0u8; ids.len()];
        let mut time = 0u32;
        let mut decision = None;
        for step in 0..STEPS {
            let plan = self.boot_plan(boot, step);
            if let Err(e) = self.calls(BOOT, &ids, &s, &plan, &mut faults, &mut time)? {
                return Ok(Err(e));
            }
            if step == 1 {
                if let Err(e) = self.window(&ids, &s, &mut faults, &mut time)? {
                    return Ok(Err(e));
                }
            }
            if step == PLANNED {
                let ok = |slot: u8| log_before.image_ok[slot as usize];
                decision = Some(
                    match self.cfg.verifier.filter(VerifierReplacement::accepted) {
                        Some(v) => boot::plan_with(&log_before, &|_| v.verdict),
                        None => boot::plan_with(&log_before, &ok),
                    },
                );
            }
            if step == COMMITTED {
                self.log = boot::commit(&log_before, decision.unwrap());
            }
        }
        let d = decision.unwrap();
        if d != boot::expected(&log_before, self.cfg.verifier) {
            let p = if self.cfg.verifier.is_some() {
                "P19"
            } else {
                "P14"
            };
            return Err(self.fail(p, format!("decision {d:?}")));
        }
        if self.log != boot::commit(&log_before, d) {
            return Err(self.fail("P14", "state log changed outside commit"));
        }
        let bound = self.cfg.trial_ms * 1000 / 2;
        if time > bound {
            return Err(self.fail("P18", format!("boot took {time} us, bound {bound}")));
        }
        if self.mem.bytes()[app_cell] != app_before[..] {
            return Err(self.fail("P23", "boot core changed the app's fuse cell"));
        }
        Ok(Ok(d))
    }

    /// The service window: boot extension 0 polls a transport until the
    /// window closes; a disabled or faulting transport leaves it idle.
    fn window(
        &mut self,
        ids: &[[u8; 32]],
        s: &fuse::Start,
        faults: &mut [u8],
        time: &mut u32,
    ) -> Result<Result<(), End>, Violation> {
        if ids.is_empty() || self.cfg.window_ms == 0 {
            return Ok(Ok(()));
        }
        let end = *time + self.cfg.window_ms * 1000;
        while *time < end {
            let before = *time;
            let plan = [(0, Behaviour::Return { us: 50 })];
            if let Err(e) = self.calls(BOOT, ids, s, &plan, faults, time)? {
                return Ok(Err(e));
            }
            *time = if *time == before {
                end
            } else {
                (before + POLL_US).max(*time)
            };
        }
        Ok(Ok(()))
    }

    fn app(&mut self, slot: u8, app: &[Call]) -> Result<Result<(), End>, Violation> {
        let ids = self.ids(APP, slot);
        let s = match self.start(APP, &ids)? {
            Ok(s) => s,
            Err(_) => return Ok(Err(End::Reset)),
        };
        let plan: Vec<(usize, Behaviour)> = app
            .iter()
            .filter(|c| (c.ext as usize) < ids.len())
            .map(|c| (c.ext as usize, c.b))
            .collect();
        let mut faults = vec![0u8; ids.len()];
        let mut time = 0;
        if let Err(e) = self.calls(APP, &ids, &s, &plan, &mut faults, &mut time)? {
            return Ok(Err(e));
        }
        self.log = boot::confirm(&self.log);
        Ok(Ok(()))
    }

    fn run(
        &mut self,
        boot: &[Call],
        app: &[Call],
        cut: Option<u32>,
        power: bool,
    ) -> Result<(), Violation> {
        self.mem.cut_after(cut.map(u64::from));
        if let Ok(d) = self.boot_core(boot)? {
            let slot = match d {
                Decision::Trial(s) | Decision::Boot(s) => Some(s),
                Decision::Rollback { to, .. } => to,
                Decision::NoImage => None,
            };
            if let Some(slot) = slot {
                self.app(slot, app)?.ok();
            }
        }
        self.mem.cut_after(None);
        self.mem.allow(0..0);
        if power {
            // Power-on clears the watchdog flag: a lockup not yet classified
            // is lost, never forged.
            self.mem.power_loss(self.cfg.survives);
            self.watchdog = false;
            self.pending.clear();
            if self.cfg.survives != Survives::Power {
                self.owed = Default::default();
            }
        }
        Ok(())
    }

    fn host_clear(&mut self, pick: u8) -> Result<(), Violation> {
        let all: Vec<[u8; 32]> = self.owed[BOOT]
            .iter()
            .chain(&self.owed[APP])
            .copied()
            .collect();
        if all.is_empty() {
            return Ok(());
        }
        let id = all[pick as usize % all.len()];
        let cells = self.cfg.cells();
        self.mem.allow(cells[APP].bytes(&FuseV1));
        let done = fuse::host_clear(&FuseV1, &mut self.mem, &cells, APP, LAYER_APP, id)
            .expect("no cut outside runs");
        self.mem.allow(0..0);
        if done && self.owed[APP].remove(&id) {
            self.tripworthy.remove(&id);
        } else if done {
            // Carried out by the boot core's next start.
            self.owed[BOOT].remove(&id);
            self.clearing.insert(id);
        }
        if done {
            self.pending.retain(|&(_, x)| x != id);
        }
        Ok(())
    }

    fn skew(&mut self, cell: u8, slot: u8, tripped: bool, pick: u8) {
        let cell = cell as usize % 2;
        let ids = self.ids(cell, self.running_app_slot());
        let Some(&id) = ids.get(pick as usize % ids.len().max(1)) else {
            return;
        };
        let c = self.cfg.cells()[cell];
        let mut b = vec![0u8; FuseV2.size()];
        let r = FuseRec {
            mark: if tripped { Mark::Tripped } else { Mark::Armed },
            layer: 0,
            id,
        };
        for (o, v) in FuseV2.stores(&r) {
            b[o] = v;
        }
        let room = c.bytes(&FuseV1);
        let at = (room.start + slot as usize * FuseV1.size()).min(room.end - b.len());
        let before = self.records();
        self.mem.poke(at, &b);
        // Foreign bytes may destroy fuse records (a loss, never a forgery):
        // the oracle stops expecting what no longer reads back.
        let after = self.records();
        let lost: BTreeSet<[u8; 32]> = before.difference(&after).map(|r| r.1).collect();
        self.pending.retain(|(_, id)| !lost.contains(id));
        self.disturbed.extend(lost.iter().copied());
        for o in &mut self.owed {
            o.retain(|id| !lost.contains(id));
        }
    }

    /// Every layout-1 record in both cells: (mark, id).
    fn records(&self) -> BTreeSet<(u8, [u8; 32])> {
        let mut out = BTreeSet::new();
        for c in self.cfg.cells() {
            for i in 0..=c.trips {
                let a = c.base + i * FuseV1.size();
                if let Some(r) = FuseV1.decode(&self.mem.bytes()[a..a + FuseV1.size()]) {
                    out.insert((r.mark as u8, r.id));
                }
            }
        }
        out
    }
}

pub struct FuseWorld;

fn behaviour(rng: &mut Rng) -> Behaviour {
    let targets = [
        Target::FlashController,
        Target::StateLog,
        Target::Watchdog,
        Target::DagonRam,
        Target::BootRecord,
        Target::Fuse,
        Target::OtherExtension,
        Target::OwnRam,
        Target::Window(rng.below(3) as u8),
    ];
    match rng.below(10) {
        0..=2 => Behaviour::Return {
            us: rng.below(140) as u32,
        },
        3 => Behaviour::Overrun,
        4..=6 => Behaviour::Access(targets[rng.below(targets.len() as u64) as usize]),
        7 => Behaviour::BusWrite {
            device: 0x20 + rng.below(2) as u8,
            register: rng.below(3) as u8,
        },
        _ => Behaviour::Lockup,
    }
}

impl World for FuseWorld {
    type Config = Config;
    type Op = Op;

    fn generate(rng: &mut Rng) -> (Config, Vec<Op>) {
        let verifier = rng.chance(0.3).then(|| VerifierReplacement {
            declared: rng.chance(0.7),
            hash_matches: rng.chance(0.7),
            image_signed: rng.chance(0.7),
            verdict: rng.chance(0.5),
        });
        let cfg = Config {
            survives: [
                Survives::ResetRandom,
                Survives::ResetCleared,
                Survives::Power,
            ][rng.below(3) as usize],
            trips: 1 + rng.below(4) as usize,
            boot_exts: rng.below(4) as u8,
            app_exts: rng.below(6) as u8,
            manifest: Manifest {
                budget_us: 100,
                windows: vec![0, 1],
                devices: vec![(0x20, 1)],
            },
            trial_ms: 2000,
            window_ms: [0, 0, 100, 400][rng.below(4) as usize],
            verifier,
            image_ok: [rng.chance(0.9), rng.chance(0.8)],
        };
        let n = 1 + rng.below(30);
        let mut ops = Vec::new();
        for _ in 0..n {
            ops.push(match rng.below(20) {
                0 => Op::ReplaceExt {
                    app: rng.chance(0.5),
                    index: rng.below(6) as u8,
                },
                1 => Op::ReplaceImage {
                    app: rng.chance(0.5),
                },
                2 | 3 => Op::HostClear { pick: rng.byte() },
                4 => Op::Skew {
                    cell: rng.below(2) as u8,
                    slot: rng.below(5) as u8,
                    tripped: rng.chance(0.5),
                    pick: rng.byte(),
                },
                _ => {
                    let mut calls = |k: u64, steps: u64| -> Vec<Call> {
                        (0..rng.below(k))
                            .map(|_| Call {
                                ext: rng.below(6) as u8,
                                step: rng.below(steps) as u8,
                                b: behaviour(rng),
                            })
                            .collect()
                    };
                    let boot = calls(4, STEPS as u64);
                    let app = calls(8, 8);
                    let cut = rng.chance(0.3).then(|| rng.below(400) as u32);
                    Op::Run {
                        boot,
                        app,
                        cut,
                        power: rng.chance(0.1),
                    }
                }
            });
        }
        (cfg, ops)
    }

    fn run(cfg: &Config, ops: &[Op], seed: u64) -> Result<(), Violation> {
        let mut rng = Rng::new(seed ^ 0xF05E);
        let h = |rng: &mut Rng| {
            let mut a = [0u8; 32];
            a.iter_mut().for_each(|b| *b = rng.byte());
            a
        };
        let calls_bound =
            cfg.boot_exts as u32 * STEPS as u32 * (cfg.manifest.budget_us + UNWIND_US);
        if calls_bound > cfg.trial_ms * 1000 / 4 || cfg.window_ms > cfg.trial_ms / 4 {
            return Err(Violation::new("P18", 0, "definition over its time budget"));
        }
        let mem = match cfg.survives {
            Survives::ResetRandom => Mem::random(cfg.mem_len(), seed),
            _ => Mem::new(cfg.mem_len(), 0, seed),
        };
        let img = Images {
            boot_header: h(&mut rng),
            app_header: [h(&mut rng), h(&mut rng)],
            boot_ext: (0..cfg.boot_exts).map(|_| h(&mut rng)).collect(),
            app_ext: (0..cfg.app_exts).map(|_| h(&mut rng)).collect(),
        };
        let mut sim = Sim {
            cfg,
            mem,
            rng,
            img,
            log: LogState {
                pending: None,
                confirmed: Some(0),
                attempts: 0,
                max_attempts: 3,
                image_ok: cfg.image_ok,
            },
            tripworthy: BTreeSet::new(),
            pending: Vec::new(),
            owed: Default::default(),
            clearing: BTreeSet::new(),
            disturbed: BTreeSet::new(),
            watchdog: false,
            step: 0,
        };
        for (i, op) in ops.iter().enumerate() {
            sim.step = i;
            match op {
                Op::Run {
                    boot,
                    app,
                    cut,
                    power,
                } => sim.run(boot, app, *cut, *power)?,
                Op::ReplaceExt { app, index } => {
                    let new = h(&mut sim.rng);
                    let v = if *app {
                        &mut sim.img.app_ext
                    } else {
                        &mut sim.img.boot_ext
                    };
                    if !v.is_empty() {
                        let k = *index as usize % v.len();
                        v[k] = new;
                    }
                }
                Op::ReplaceImage { app: false } => sim.img.boot_header = h(&mut sim.rng),
                Op::ReplaceImage { app: true } => {
                    let other = 1 - sim.running_app_slot();
                    sim.img.app_header[other as usize] = h(&mut sim.rng);
                    sim.log.pending = Some(other);
                    sim.log.attempts = 0;
                }
                Op::HostClear { pick } => sim.host_clear(*pick)?,
                Op::Skew {
                    cell,
                    slot,
                    tripped,
                    pick,
                } => sim.skew(*cell, *slot, *tripped, *pick),
            }
            if let Some(a) = sim.mem.stray().first() {
                return Err(sim.fail("P20", format!("store outside the layer's cell at {a}")));
            }
        }
        Ok(())
    }

    fn simpler(op: &Op) -> Vec<Op> {
        match op {
            Op::Run {
                boot,
                app,
                cut,
                power,
            } => {
                let mk = |boot: &[Call], app: &[Call], cut, power| Op::Run {
                    boot: boot.to_vec(),
                    app: app.to_vec(),
                    cut,
                    power,
                };
                let mut v = vec![
                    mk(boot, app, None, *power),
                    mk(boot, app, *cut, false),
                    mk(&[], app, *cut, *power),
                    mk(boot, &[], *cut, *power),
                ];
                for i in 0..boot.len() {
                    let mut b = boot.clone();
                    b.remove(i);
                    v.push(mk(&b, app, *cut, *power));
                }
                for i in 0..app.len() {
                    let mut a = app.clone();
                    a.remove(i);
                    v.push(mk(boot, &a, *cut, *power));
                }
                v.retain(|o| o != op);
                v
            }
            _ => Vec::new(),
        }
    }
}
