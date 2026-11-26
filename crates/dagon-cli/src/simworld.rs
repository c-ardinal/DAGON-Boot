//! `sim --topology`: an update of a topology of one's own in the
//! simulator, each node modelled on the project's MCU definitions
//! (`dagon_sim::world`), and the faults `--fault NODE=KIND` injects into
//! one node of it. Every node starts confirmed on 1.0.0; a release 1.1.0
//! for all of them is signed with the public RFC 8032 test key and written
//! through dagon-host, as `dagon deploy` writes a real one.

use dagon_format::bundle::BundleView;
use dagon_format::header::{image_kind, target_tag, HEADER_SIZE, SIGNED_LEN};
use dagon_format::statelog::reason;
use dagon_format::Version;
use dagon_host::{Error, Host, NodeOutcome, Outcome, RecoverAssets, RecoverPath, Target};
use dagon_proto::Status;
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{block_on, CutPoint, SimNode, SimRelay, TrialApp};
use dagon_topology::defs::Registry;
use dagon_topology::schema::{Node, Topology};
use ed25519_compact::{KeyPair, Seed};

use crate::cli::SimArgs;
use crate::config::Config;
use crate::simfault::{Fault, Injection};
use crate::simrun::{Sim, SEED};

const OLD: Version = Version::new(1, 0, 0);
const NEW: Version = Version::new(1, 1, 0);
/// Bytes of a simulated app or boot core (less where its slot holds less).
const BODY_BYTES: u32 = 6000;
/// A stand-in stage0 for the ROM write: the simulator runs none of it.
const STAGE0: [u8; 0x100] = [0x5A; 0x100];

/// `dagon sim`: the built-in scenarios, or with `--topology`, `--fault` or
/// `--config` the update of a topology of one's own.
pub fn command(a: &SimArgs) -> Result<(), String> {
    if a.topology.is_none() && a.fault.is_empty() && a.config.is_none() {
        return crate::simrun::run(a.scenario.as_deref());
    }
    let cfg = Config::load(a.config.as_deref())?;
    let path = cfg.topology_path(a.topology.as_deref())?;
    let t = crate::config::read_topology(&cfg, &path)?;
    let reg = crate::defs::registry(&cfg)?;
    run(&reg, &t, &a.fault, &mut |line| println!("{line}"))
}

/// Simulates the update of `t`, then each of `faults` on a simulation of
/// its own; `say` takes each result line.
pub(crate) fn run(
    reg: &Registry,
    t: &Topology,
    faults: &[Injection],
    say: &mut dyn FnMut(String),
) -> Result<(), String> {
    let first = World::new(reg, t)?;
    for f in faults {
        let named = |e: String| format!("--fault {}={}: {e}", f.node, f.fault.name());
        if t.node(&f.node).is_none() {
            let ids: Vec<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
            return Err(named(format!(
                "the topology has no node `{}` (it has {})",
                f.node,
                ids.join(", ")
            )));
        }
        if f.fault == Fault::Erased {
            first.rom_reaches(&f.node).map_err(named)?;
        }
    }
    say(first.wiring());
    let mut failed = 0;
    let mut tell = |what: String, r: Result<String, String>| match r {
        Ok(d) => say(format!("{what} PASS: {d}")),
        Err(e) => {
            failed += 1;
            say(format!("{what} FAIL: {e}"));
        }
    };
    tell("update".into(), first.update());
    for f in faults {
        let w = World::new(reg, t)?;
        let r = match f.fault {
            Fault::PowerCut => w.power_cut(&f.node),
            Fault::NoConfirm => w.no_confirm(&f.node),
            Fault::Tamper => w.tamper(&f.node),
            Fault::Erased => w.erased(&f.node),
        };
        tell(format!("{} {}", f.node, f.fault.name()), r);
    }
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} of {} runs failed", faults.len() + 1)),
    }
}

/// One simulation: every node of the topology confirmed on 1.0.0 and the
/// release that takes them all to 1.1.0.
struct World<'a> {
    reg: &'a Registry,
    t: &'a Topology,
    key: KeyPair,
    sim: Sim,
    /// The compiled topology.
    bin: Vec<u8>,
    release: Vec<u8>,
    /// Each node's target, in topology order (its index there).
    targets: Vec<Target>,
    /// The node NRST and BOOT0 reach.
    wired: Option<String>,
}

fn dbg<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

/// The image of `kind` for slot `slot` of node `n`, when its MCU has it.
fn sign(
    reg: &Registry,
    key: &KeyPair,
    n: &Node,
    kind: u8,
    slot: u8,
    v: Version,
) -> Result<Option<Vec<u8>>, String> {
    let Some(at) = image::slot_with(reg, &n.mcu, kind, slot) else {
        return Ok(None);
    };
    let def = reg
        .mcu(&n.mcu)
        .ok_or_else(|| format!("no MCU definition {}", n.mcu))?;
    let (flags, caps) = image::default_bits(kind);
    let spec = ImageSpec {
        image_kind: kind,
        port_kind: image::port_kind_of(&n.port)
            .ok_or_else(|| format!("node {}: no port kind {}", n.id, n.port))?,
        mcu_id: def.mcu_id(),
        slot: at,
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag(&n.tag),
        proto_version: 1,
        capabilities: caps,
    };
    let len = BODY_BYTES.min(at.max_payload) as usize;
    let body: Vec<u8> = (0..len)
        .map(|i| (i * 7 + usize::from(slot) + usize::from(v.minor())) as u8)
        .collect();
    sign_image(&spec, &body, key)
        .map(Some)
        .map_err(|e| format!("node {}: {e}", n.id))
}

/// The apps of node `n` at `v`, one for each slot an image is built for.
fn apps(reg: &Registry, key: &KeyPair, n: &Node, v: Version) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    for slot in 0..2 {
        out.extend(sign(reg, key, n, image_kind::APP, slot, v)?);
    }
    if out.is_empty() {
        return Err(format!(
            "node {}: MCU definition {} has no app slot",
            n.id, n.mcu
        ));
    }
    Ok(out)
}

fn node_at(relay: &mut SimRelay, to: Target) -> Option<&mut SimNode> {
    match to {
        Target::Local => Some(&mut relay.node),
        Target::Linked { link, addr, .. } => relay.node_behind_mut(link, addr),
        // The simulated relaying node has no relaying node behind it.
        Target::Deep(_) => None,
    }
}

impl<'a> World<'a> {
    fn new(reg: &'a Registry, t: &'a Topology) -> Result<World<'a>, String> {
        let key = KeyPair::from_seed(Seed::new(SEED));
        let bin = dagon_topology::compile_with(reg, t).map_err(|e| e.to_string())?;
        let (mut relay, wired) = dagon_sim::world::build(reg, t, *key.pk, 1)?;
        let targets = crate::device::targets(reg, t)?
            .into_iter()
            .map(|(id, to)| to.map_err(|e| format!("node {id} is not reachable: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut images = Vec::new();
        for (n, to) in t.nodes.iter().zip(&targets) {
            let old = apps(reg, &key, n, OLD)?.swap_remove(0);
            let node = node_at(&mut relay, *to)
                .ok_or_else(|| format!("node {}: the simulated wiring has no such node", n.id))?;
            let put = |r: Result<(), _>| r.map_err(|e| format!("node {}: {e:?}", n.id));
            put(node.provision_core(|m| *key.sk.sign(m, None)))?;
            if node.store().layout.cores.is_none() && node.store().layout.factory.is_some() {
                put(node.store().provision_factory(&old))?;
            }
            put(node.provision(&old, 0))?;
            images.extend(apps(reg, &key, n, NEW)?);
        }
        let release = dagon_sign::release::build(reg, t, &bin, &images, "sim-1.1", &key)
            .map_err(|e| e.to_string())?
            .bundle;
        Ok(World {
            reg,
            t,
            key,
            sim: Sim::new(relay),
            bin,
            release,
            targets,
            wired,
        })
    }

    /// The nodes and how the PC reaches each.
    fn wiring(&self) -> String {
        let nodes: Vec<String> = self
            .t
            .nodes
            .iter()
            .zip(&self.targets)
            .map(|(n, to)| match to {
                Target::Local => format!("{} ({}, connected to the PC)", n.id, n.mcu),
                Target::Linked { link, addr, .. } => {
                    format!("{} ({}, link {link} address {addr:#04x})", n.id, n.mcu)
                }
                Target::Deep(route) => {
                    let hops: Vec<String> = route
                        .hops()
                        .iter()
                        .map(|h| format!("link {} address {:#04x}", h.link, h.addr))
                        .collect();
                    format!("{} ({}, {})", n.id, n.mcu, hops.join(", then "))
                }
            })
            .collect();
        format!("topology {}: {}", self.t.name, nodes.join(", "))
    }

    fn host(&self) -> Result<Host<Sim, Sim>, String> {
        let mut h = Host::new(self.sim.clone(), self.sim.clone());
        h.timings = dagon_host::node_timings(self.reg, self.t).map_err(dbg)?;
        h.local = crate::device::local(self.reg, self.t);
        Ok(h)
    }

    /// The index and target of node `id`.
    fn at(&self, id: &str) -> Result<(u8, Target), String> {
        let i = self
            .t
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| format!("no node {id}"))?;
        Ok((i as u8, self.targets[i]))
    }

    /// Changes node `id` of the simulated wiring.
    fn with_node<R>(&self, id: &str, f: impl FnOnce(&mut SimNode, u64) -> R) -> Result<R, String> {
        let (_, to) = self.at(id)?;
        let mut relay = self.sim.relay();
        let now = relay.now_ms();
        let node = node_at(&mut relay, to).ok_or_else(|| format!("no simulated node {id}"))?;
        Ok(f(node, now))
    }

    fn apply(&self, bundle: &[u8]) -> Result<Vec<NodeOutcome<()>>, Error<()>> {
        let mut h = self.host().map_err(|_| Error::BadResponse)?;
        block_on(h.apply(bundle, &self.bin, &self.key.pk, None, &mut |_| {}))
    }

    /// Fails unless node `id` runs `v`, confirmed.
    fn expect(&self, id: &str, v: Version) -> Result<(), String> {
        let (_, to) = self.at(id)?;
        let h = block_on(self.host()?.hello(to)).map_err(|e| format!("{id}: {e:?}"))?;
        if (h.active_version, h.trial) != (v, false) {
            return Err(format!(
                "{id} runs {} (trial {}), expected {v} confirmed",
                h.active_version, h.trial
            ));
        }
        Ok(())
    }

    fn expect_all(&self, v: Version) -> Result<(), String> {
        self.t.nodes.iter().try_for_each(|n| self.expect(&n.id, v))
    }

    /// Fails unless the write stopped at node `id`: its outcome a failure,
    /// or the whole write one when the PC is connected to that node.
    fn stopped_at(
        &self,
        id: &str,
        r: Result<Vec<NodeOutcome<()>>, Error<()>>,
    ) -> Result<(), String> {
        let (i, to) = self.at(id)?;
        match r {
            Err(_) if to == Target::Local => Ok(()),
            Err(e) => Err(format!("the write failed before {id}: {e:?}")),
            Ok(out) => match out.iter().find(|o| o.node == i).map(|o| &o.outcome) {
                Some(Outcome::Failed(_)) => Ok(()),
                o => Err(format!("the write did not stop at {id}: {o:?}")),
            },
        }
    }

    /// Writes the release and fails unless every node ends on 1.1.0.
    fn write_all(&self) -> Result<(), String> {
        let out = self.apply(&self.release).map_err(dbg)?;
        for o in &out {
            if !matches!(o.outcome, Outcome::Done(v) if v == NEW) {
                let id = &self.t.nodes[usize::from(o.node)].id;
                return Err(format!("{id}: {:?}", o.outcome));
            }
        }
        self.expect_all(NEW)
    }

    fn update(&self) -> Result<String, String> {
        self.write_all()?;
        let ids: Vec<&str> = self.t.nodes.iter().map(|n| n.id.as_str()).collect();
        Ok(format!("{} on {NEW}, from {OLD}", ids.join(", ")))
    }

    fn power_cut(&self, id: &str) -> Result<String, String> {
        self.with_node(id, |n, _| {
            let mut f = n.flash();
            let at = f.ops().len() + 4;
            f.arm_cut(CutPoint::During(at));
        })?;
        self.stopped_at(id, self.apply(&self.release))?;
        self.with_node(id, |n, now| n.power_on(now))?;
        self.expect(id, OLD)?;
        self.write_all()?;
        Ok(format!(
            "the cut kept {OLD}; written again, every node on {NEW}"
        ))
    }

    fn no_confirm(&self, id: &str) -> Result<String, String> {
        let (attempts, step) = self.with_node(id, |n, _| {
            n.trial_app = TrialApp::NeverConfirms;
            (n.store().layout.trial_attempts(), n.trial_wdt_ms)
        })?;
        self.stopped_at(id, self.apply(&self.release))?;
        // One watchdog reset for each boot attempt.
        for _ in 0..=attempts {
            self.sim.relay().advance_ms(step);
        }
        let (_, to) = self.at(id)?;
        let st = block_on(self.host()?.status(to)).map_err(dbg)?.status;
        if st.last_reason != reason::TRIAL_EXHAUSTED {
            return Err(format!("{id} did not use up its trial: {st:?}"));
        }
        self.expect(id, OLD)?;
        Ok(format!("back on {OLD}, TRIAL_EXHAUSTED"))
    }

    /// Where each image of node `id` starts in the release.
    fn images_of(&self, id: &str) -> Result<Vec<usize>, String> {
        let tag = target_tag(&self.t.node(id).ok_or("no such node")?.tag);
        let b = BundleView::parse(&self.release).map_err(dbg)?;
        Ok((0..b.item_count())
            .map(|i| b.item(i))
            .filter(|it| {
                let (at, len) = (it.offset as usize, it.length as usize);
                self.release
                    .get(at..at + len)
                    .and_then(|img| image::verify_image(img, &self.key.pk).ok())
                    .is_some_and(|h| h.target_tag == tag)
            })
            .map(|it| it.offset as usize)
            .collect())
    }

    fn tamper(&self, id: &str) -> Result<String, String> {
        let (i, _) = self.at(id)?;
        let images = self.images_of(id)?;
        for (what, at, node_says) in [
            ("header", 0x20, Status::Signature),
            ("payload", HEADER_SIZE + 100, Status::Digest),
            ("signature", SIGNED_LEN + 5, Status::Signature),
        ] {
            let mut bad = self.release.clone();
            images.iter().for_each(|&off| bad[off + at] ^= 0x01);
            match self.apply(&bad) {
                Err(Error::Format(_)) => {}
                r => return Err(format!("the PC took a flipped {what}: {r:?}")),
            }
            // Without the PC's check the node itself refuses it.
            let out =
                block_on(
                    self.host()?
                        .apply_unverified(&bad, &self.bin, Some(&[i]), &mut |_| {}),
                )
                .map_err(|e| format!("{what}, no check by the PC: {e:?}"))?;
            match out.iter().find(|o| o.node == i).map(|o| &o.outcome) {
                Some(Outcome::Failed(Error::Rejected { status, .. })) if *status == node_says => {}
                o => return Err(format!("{what}: {id} did not answer {node_says:?}: {o:?}")),
            }
        }
        self.expect_all(OLD)?;
        Ok(format!(
            "3 flipped images refused by the PC, and by {id} itself with no check by the PC"
        ))
    }

    /// Fails unless the simulated NRST and BOOT0 reach node `id`, so its
    /// ROM loader can be entered.
    fn rom_reaches(&self, id: &str) -> Result<(), String> {
        match &self.wired {
            Some(w) if w == id => Ok(()),
            Some(w) => Err(format!(
                "the simulated reset and boot lines reach {w}, not {id}"
            )),
            None => Err(
                "the simulator models an I2C ROM loader on a link whose other end drives the \
                 node's reset and boot pins; the topology has no such node"
                    .into(),
            ),
        }
    }

    fn erased(&self, id: &str) -> Result<String, String> {
        self.rom_reaches(id)?;
        let (i, _) = self.at(id)?;
        self.with_node(id, |n, now| {
            let r = n.flash().erase_all();
            n.reset(now);
            r
        })?
        .map_err(dbg)?;
        let n = self.t.node(id).ok_or("no such node")?;
        let boot = sign(self.reg, &self.key, n, image_kind::BOOTCORE, 0, OLD)?
            .ok_or_else(|| format!("MCU definition {} has no boot core slot", n.mcu))?;
        let roms = dagon_host::rom::rom_maps(self.reg);
        let mut h = self.host()?;
        let way = block_on(h.write_path(i, &self.bin, &roms)).map_err(dbg)?;
        if way != dagon_host::WritePath::Rom {
            return Err(format!("expected the ROM path, got {way:?}"));
        }
        let assets = RecoverAssets {
            release: &self.release,
            topology: &self.bin,
            pubkey: &self.key.pk,
            bootcore: Some(&boot),
            stage0: Some(&STAGE0),
            roms: &roms,
            rom_confirmed: true,
        };
        let (path, v) = block_on(h.recover(i, &assets, &mut |_| {})).map_err(dbg)?;
        let RecoverPath::Rom(stats) = path else {
            return Err(format!("expected the ROM path, got {path:?}"));
        };
        self.expect(id, v)?;
        Ok(format!(
            "rewritten through the ROM loader to {v}: {} frames, {} ms simulated",
            stats.frames, stats.elapsed_ms
        ))
    }
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests;
