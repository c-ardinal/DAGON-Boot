//! `sim run`: the four simulator scenarios, end to end through dagon-host
//! against the simulated hub (RP2040) and F401, with the public RFC 8032
//! test key. Needs no hardware.

use std::cell::RefCell;
use std::rc::Rc;

use dagon_format::bundle::{dest, BundleView};
use dagon_format::header::{image_kind, target_tag, HEADER_SIZE, SIGNED_LEN};
use dagon_format::statelog::reason;
use dagon_format::Version;
use dagon_host::{Clock, Error, Host, Outcome, RecoverAssets, RecoverPath, Target, Transport};
use dagon_proto::payload::SlotState;
use dagon_proto::Status;
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{block_on, demo_hub, CutPoint, SimHub, TrialApp, NODE_I2C_ADDR};
use ed25519_compact::{KeyPair, Seed};

const YAML: &str = include_str!("../../../topology/sim-pair.yaml");

/// Node `id`'s tag in the sim topology: images are signed for it.
fn topology_tag(id: &str) -> String {
    let t = dagon_topology::parse_str(YAML).expect("sim topology parses");
    t.node(id).expect("node in the sim topology").tag.clone()
}
/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const SEG: u8 = 1;
const SEG_AT: Target = Target::I2c(NODE_I2C_ADDR);
const OLD: Version = Version::new(1, 0, 0);
const NEW: Version = Version::new(1, 1, 0);
/// Long enough for the trial watchdog to use up every boot attempt.
const TRIAL_RUNOUT_MS: u64 = 40_000;
/// The simulated node's trial watchdog period.
const TRIAL_STEP_MS: u64 = 10_000;

type R = Result<String, String>;
/// ID, title, scenario.
type Scenario = (&'static str, &'static str, fn() -> R);

/// The simulated hub as dagon-host's transport and clock.
#[derive(Clone)]
pub(crate) struct Sim {
    hub: Rc<RefCell<SimHub>>,
    pending: Vec<u8>,
}

impl Sim {
    #[cfg(test)]
    pub(crate) fn new(hub: SimHub) -> Sim {
        Sim {
            hub: Rc::new(RefCell::new(hub)),
            pending: Vec::new(),
        }
    }
}

impl Transport for Sim {
    type Error = ();
    fn max_payload(&self) -> usize {
        dagon_proto::max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.hub.borrow_mut().usb_write(bytes);
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.pending.extend(self.hub.borrow_mut().usb_read());
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl Clock for Sim {
    fn now_ms(&self) -> u64 {
        self.hub.borrow().now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.hub.borrow_mut().advance_ms(ms as u64);
    }
}

struct World {
    key: KeyPair,
    sim: Sim,
    topology: Vec<u8>,
    release: Vec<u8>,
}

fn sign(
    key: &KeyPair,
    mcu: &str,
    kind: u8,
    tag: &str,
    slot: u8,
    v: Version,
    len: usize,
) -> Vec<u8> {
    let (flags, caps) = image::default_bits(kind);
    let spec = ImageSpec {
        image_kind: kind,
        port_kind: image::port_kind_of("self-boot").expect("known"),
        mcu_id: image::mcu_id_of(mcu).expect("known"),
        slot: image::slot(mcu, kind, slot).expect("known"),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    };
    let body: Vec<u8> = (0..len).map(|i| (i * 7 + slot as usize) as u8).collect();
    sign_image(&spec, &body, key).expect("valid spec")
}

fn seg_img(key: &KeyPair, slot: u8, v: Version) -> Vec<u8> {
    sign(
        key,
        "stm32f401re",
        image_kind::APP,
        &topology_tag("seg"),
        slot,
        v,
        6000,
    )
}

fn hub_img(key: &KeyPair, slot: u8, v: Version) -> Vec<u8> {
    sign(
        key,
        "rp2040",
        image_kind::APP,
        &topology_tag("hub"),
        slot,
        v,
        8000,
    )
}

/// Hub and F401 on 1.0.0 (slot A, confirmed), release 1.1.0 for both.
fn world() -> Result<World, String> {
    let key = KeyPair::from_seed(Seed::new(SEED));
    let t = dagon_topology::parse_str(YAML).map_err(|e| e.to_string())?;
    let topology = dagon_topology::compile(&t).map_err(|e| e.to_string())?;
    let images = [
        seg_img(&key, 0, NEW),
        seg_img(&key, 1, NEW),
        hub_img(&key, 0, NEW),
        hub_img(&key, 1, NEW),
    ];
    let release = dagon_sign::release::build(
        dagon_topology::defs::Registry::builtin(),
        &t,
        &topology,
        &images,
        "sim-1.1",
        &key,
    )
    .map_err(|e| e.to_string())?
    .bundle;
    let mut hub = demo_hub(*key.pk, 1);
    hub.node
        .provision(&hub_img(&key, 0, OLD), 0)
        .map_err(|e| format!("{e:?}"))?;
    seg(&mut hub)
        .provision(&seg_img(&key, 0, OLD), 0)
        .map_err(|e| format!("{e:?}"))?;
    let sim = Sim {
        hub: Rc::new(RefCell::new(hub)),
        pending: Vec::new(),
    };
    Ok(World {
        key,
        sim,
        topology,
        release,
    })
}

fn seg(hub: &mut SimHub) -> &mut dagon_sim::SimNode {
    hub.i2c_node(NODE_I2C_ADDR).expect("demo hub has the F401")
}

impl World {
    fn host(&self) -> Host<Sim, Sim> {
        Host::new(self.sim.clone(), self.sim.clone())
    }

    fn apply(&self, bundle: &[u8]) -> Result<Vec<Outcome<()>>, Error<()>> {
        let r =
            block_on(
                self.host()
                    .apply(bundle, &self.topology, &self.key.pk, None, &mut |_| {}),
            )?;
        Ok(r.into_iter().map(|o| o.outcome).collect())
    }

    /// Fails unless `to` runs `v`, confirmed.
    fn expect(&self, to: Target, v: Version) -> Result<(), String> {
        let h = block_on(self.host().hello(to)).map_err(|e| format!("{to:?}: {e:?}"))?;
        if (h.active_version, h.trial) != (v, false) {
            return Err(format!(
                "{to:?} runs {} (trial {}), expected {v} confirmed",
                h.active_version, h.trial
            ));
        }
        Ok(())
    }
}

fn all_done(out: &[Outcome<()>]) -> bool {
    out.iter()
        .all(|o| matches!(o, Outcome::Done(v) if *v == NEW))
}

/// Update, with one power cut mid-transfer on the F401, then again.
fn s1() -> R {
    let w = world()?;
    {
        let mut hub = w.sim.hub.borrow_mut();
        let mut f = seg(&mut hub).flash();
        let at = f.ops().len() + 4;
        f.arm_cut(CutPoint::During(at));
    }
    let out = w.apply(&w.release).map_err(|e| format!("{e:?}"))?;
    if !matches!(out.first(), Some(Outcome::Failed(_))) {
        return Err(format!("power cut not noticed: {out:?}"));
    }
    {
        let mut hub = w.sim.hub.borrow_mut();
        let now = hub.now_ms();
        seg(&mut hub).power_on(now);
    }
    w.expect(SEG_AT, OLD)?;
    let out = w.apply(&w.release).map_err(|e| format!("{e:?}"))?;
    if !all_done(&out) {
        return Err(format!("second apply: {out:?}"));
    }
    w.expect(SEG_AT, NEW)?;
    w.expect(Target::Hub, NEW)?;
    Ok("cut kept 1.0.0; resent, hub and seg on 1.1.0".into())
}

/// A new F401 app that never confirms rolls back.
fn s2() -> R {
    let w = world()?;
    seg(&mut w.sim.hub.borrow_mut()).trial_app = TrialApp::NeverConfirms;
    let out = w.apply(&w.release).map_err(|e| format!("{e:?}"))?;
    if !matches!(out.first(), Some(Outcome::Failed(_))) {
        return Err(format!("apply did not stop: {out:?}"));
    }
    // One watchdog reset per boot attempt: advance a trial period at a time.
    for _ in 0..TRIAL_RUNOUT_MS / TRIAL_STEP_MS {
        w.sim.hub.borrow_mut().advance_ms(TRIAL_STEP_MS);
    }
    let st = block_on(w.host().status(SEG_AT))
        .map_err(|e| format!("{e:?}"))?
        .status;
    let ok = st.active == 0
        && st.slots[1].state == SlotState::Reverted
        && st.last_reason == reason::TRIAL_EXHAUSTED;
    if !ok {
        return Err(format!("not rolled back: {st:?}"));
    }
    w.expect(SEG_AT, OLD)?;
    Ok("back on 1.0.0, TRIAL_EXHAUSTED".into())
}

/// One bit flipped in an image header, its payload, its signature.
fn s3() -> R {
    let w = world()?;
    let b = BundleView::parse(&w.release).map_err(|e| format!("{e:?}"))?;
    let seg_items: Vec<usize> = (0..b.item_count())
        .map(|i| b.item(i))
        .filter(|it| it.dest == dest::SUB_STORE)
        .map(|it| it.offset as usize)
        .collect();
    for (what, at, node_says) in [
        ("header", 0x20, Status::Signature),
        ("payload", HEADER_SIZE + 100, Status::Digest),
        ("signature", SIGNED_LEN + 5, Status::Signature),
    ] {
        let mut bad = w.release.clone();
        seg_items.iter().for_each(|&off| bad[off + at] ^= 0x01);
        match w.apply(&bad) {
            Err(Error::Format(_)) => {}
            r => return Err(format!("{what} flip accepted: {r:?}")),
        }
        // Without the PC-side check the node itself must refuse it.
        let out = block_on(
            w.host()
                .apply_unverified(&bad, &w.topology, None, &mut |_| {}),
        )
        .map_err(|e| format!("{what}, no host verify: {e:?}"))?;
        match out.first().map(|o| &o.outcome) {
            Some(Outcome::Failed(Error::Rejected { status, .. })) if *status == node_says => {}
            o => return Err(format!("{what}: node did not answer {node_says:?}: {o:?}")),
        }
    }
    w.expect(SEG_AT, OLD)?;
    w.expect(Target::Hub, OLD)?;
    Ok("3 tampered releases refused by the PC, and by the F401 (E_SIGNATURE, E_DIGEST) with no host verify".into())
}

/// The F401 fully erased, then rewritten through the ROM loader.
fn s4() -> R {
    let w = world()?;
    {
        let mut hub = w.sim.hub.borrow_mut();
        let now = hub.now_ms();
        let node = seg(&mut hub);
        node.flash().erase_all().map_err(|e| format!("{e:?}"))?;
        node.reset(now);
    }
    let boot = sign(
        &w.key,
        "stm32f401re",
        image_kind::BOOTCORE,
        &topology_tag("seg"),
        0,
        OLD,
        4000,
    );
    let assets = RecoverAssets {
        release: &w.release,
        topology: &w.topology,
        pubkey: &w.key.pk,
        bootcore: Some(&boot),
        stage0: Some(&[0x5A; 0x100]),
        roms: &dagon_host::rom::rom_maps(dagon_topology::defs::Registry::builtin()),
    };
    let (path, v) =
        block_on(w.host().recover(SEG, &assets, &mut |_| {})).map_err(|e| format!("{e:?}"))?;
    let RecoverPath::Rom(stats) = path else {
        return Err(format!("expected the ROM path, got {path:?}"));
    };
    w.expect(SEG_AT, v)?;
    Ok(format!(
        "ROM path: {} frames, max round trip {} ms, {} ms simulated",
        stats.frames, stats.max_rtt_ms, stats.elapsed_ms
    ))
}

pub fn run() -> Result<(), String> {
    let scenarios: [Scenario; 4] = [
        ("S1", "update with a power cut", s1),
        ("S2", "trial app never confirms", s2),
        ("S3", "tampered release", s3),
        ("S4", "erased F401 via ROM loader", s4),
    ];
    let mut failed = 0;
    for (id, name, f) in scenarios {
        match f() {
            Ok(d) => println!("{id} PASS {name}: {d}"),
            Err(e) => {
                failed += 1;
                println!("{id} FAIL {name}: {e}");
            }
        }
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} of 4 scenarios failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dagon status` and `dagon doctor` print these lines: every node's
    /// versions, and "unknown" for an agent that predates VERSION.
    #[test]
    fn version_lines_of_the_hub_and_its_nodes() {
        let w = world().unwrap();
        let nodes = [("seg".to_string(), Some(SEG_AT)), ("far".to_string(), None)];
        let lines = crate::device::version_lines(&mut w.host(), &nodes);
        let s0 = dagon_sim::store::STAGE0_VERSION;
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].starts_with(&format!("hub: versions: stage0 {s0}; boot core A:")),
            "{lines:?}"
        );
        assert!(lines[0].ends_with("; wire 1; mcu 0x0001"), "{lines:?}");
        assert!(
            lines[1].contains(&format!("app A:Confirmed {OLD} (running)")),
            "{lines:?}"
        );
        assert!(lines[1].ends_with("mcu 0x0002"), "{lines:?}");
        assert_eq!(lines[2], "far: not reachable through the hub");

        seg(&mut w.sim.hub.borrow_mut()).predates = vec![dagon_proto::Cmd::Version as u8];
        let lines = crate::device::version_lines(&mut w.host(), &nodes);
        assert_eq!(
            lines[1],
            "seg: versions unknown (its agent predates VERSION)"
        );
        assert!(
            lines[0].starts_with("hub: versions: stage0"),
            "the hub still answers"
        );
    }

    #[test]
    fn all_scenarios_pass() {
        super::run().unwrap();
    }

    #[test]
    fn rollback_command_returns_the_node_to_its_previous_version() {
        let w = world().unwrap();
        let out = w.apply(&w.release).unwrap();
        assert!(all_done(&out), "{out:?}");
        let t = dagon_topology::parse_str(YAML).unwrap();
        let mut events = Vec::new();
        let v = crate::device::rollback_node(
            &mut w.host(),
            dagon_topology::defs::Registry::builtin(),
            &t,
            "seg",
            &mut |e| events.push(e),
        )
        .unwrap();
        assert_eq!(v, OLD);
        w.expect(SEG_AT, OLD).unwrap();
        let st = block_on(w.host().status(SEG_AT)).unwrap().status;
        assert_eq!((st.active, st.confirmed), (0, 0));
        assert_eq!(st.slots[0].state, SlotState::Confirmed);
        w.expect(Target::Hub, NEW).unwrap();
        let unknown = crate::device::rollback_node(
            &mut w.host(),
            dagon_topology::defs::Registry::builtin(),
            &t,
            "nope",
            &mut |_| {},
        );
        assert!(unknown.is_err());
    }

    /// A package of the F401 app `app` and its boot core `core` (slots A
    /// and B, `len` bytes each).
    fn with_core(w: &World, app: Version, core: Version, len: usize) -> Vec<u8> {
        let t = dagon_topology::parse_str(YAML).unwrap();
        let tag = topology_tag("seg");
        let images = [
            seg_img(&w.key, 0, app),
            seg_img(&w.key, 1, app),
            sign(
                &w.key,
                "stm32f401re",
                image_kind::BOOTCORE,
                &tag,
                0,
                core,
                len,
            ),
            sign(
                &w.key,
                "stm32f401re",
                image_kind::BOOTCORE,
                &tag,
                1,
                core,
                len,
            ),
        ];
        dagon_sign::release::build(
            dagon_topology::defs::Registry::builtin(),
            &t,
            &w.topology,
            &images,
            "core",
            &w.key,
        )
        .unwrap()
        .bundle
    }

    /// The F401's running boot core: (version, digest16).
    fn running_core(w: &World) -> (Version, [u8; 16]) {
        let cs = block_on(w.host().boot_core_status(SEG_AT)).unwrap();
        let s = cs.slots[usize::from(cs.active)];
        (s.version, s.digest16)
    }

    #[test]
    fn a_boot_core_is_written_only_when_its_version_differs_from_the_running_one() {
        let w = world().unwrap();
        // A boot core of another version than the running one is written.
        let mut confirms = Vec::new();
        let out = block_on(w.host().apply(
            &with_core(&w, NEW, NEW, 4000),
            &w.topology,
            &w.key.pk,
            None,
            &mut |e| {
                if e.node == SEG && e.phase == dagon_proto::payload::phase::CONFIRM {
                    confirms.push(e.boot_core);
                }
            },
        ))
        .unwrap();
        assert!(
            matches!(out[0].outcome, Outcome::Done(v) if v == NEW),
            "{out:?}"
        );
        // The boot core's CONFIRM says it is the boot core's; the node's
        // write ends with its app's.
        assert_eq!(confirms, [true, false]);
        let (v, written) = running_core(&w);
        assert_eq!(v, NEW);
        // The same version, even with other bytes, is not written again.
        let app = Version::new(1, 2, 0);
        let out = w.apply(&with_core(&w, app, NEW, 4004)).unwrap();
        assert!(matches!(out[0], Outcome::Done(v) if v == app), "{out:?}");
        assert_eq!(
            running_core(&w),
            (NEW, written),
            "same version: not written"
        );
        // A raised version is written.
        let core = Version::new(1, 2, 0);
        let app = Version::new(1, 3, 0);
        let out = w.apply(&with_core(&w, app, core, 4004)).unwrap();
        assert!(matches!(out[0], Outcome::Done(v) if v == app), "{out:?}");
        assert_eq!(running_core(&w).0, core);
    }
}
