//! The conformance run against the simulator: the F401 behind the hub
//! (RELAY on I2C) and an F401 on the PC's port directly (USB as UART). The
//! simulated node has no conformance session, so a conformance build is
//! modelled here: CONF_LIST, CONF_RUN and CONF_RESULT frames are answered by
//! dagon-conform's node session, a test running over the node's flash after
//! its CONF_RUN was answered; everything else goes to the node.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dagon_conform::node::{self, NodeSetup, Session, RESPONSE_MAX};
use dagon_conform::report::Verdict;
use dagon_format::header::{image_kind, SIGN_DOMAIN};
use dagon_format::Version;
use dagon_port::Flash;
use dagon_proto::payload::Relay;
use dagon_proto::{flags, frame_len, max_payload, Cmd, Frame};
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{demo_hub, f401_config, FrameCutter, Layout, SimFlash, SimNode, SimRelay};
use ed25519_compact::{KeyPair, Seed};

use super::*;

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const TRIAL_WDT_MS: u64 = 4_000;

/// The seg of the simulated pair (`topology/sim-pair.yaml`): its target
/// from the hub and the kind of its link.
fn seg() -> (Target, LinkKind) {
    let t = dagon_topology::parse_str(include_str!("../../../../topology/sim-pair.yaml")).unwrap();
    node_target(dagon_topology::defs::Registry::builtin(), &t, "seg").unwrap()
}

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn img(slot: u8, v: Version) -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::APP);
    let mcu = "stm32f401re";
    let spec = ImageSpec {
        image_kind: image_kind::APP,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::APP, slot).unwrap(),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: dagon_format::header::target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: caps,
    };
    let body: Vec<u8> = (0..3000u32).map(|i| (i * 7 + v.raw()) as u8).collect();
    sign_image(&spec, &body, &key()).unwrap()
}

fn images() -> ConformImages {
    let (good, bad) = (Version::new(1, 1, 0), Version::new(1, 2, 0));
    ConformImages {
        good: vec![img(0, good), img(1, good)],
        no_confirm: vec![img(0, bad), img(1, bad)],
    }
}

/// The images folder holds slot a's images, and slot b's for a node with
/// slot B; slot a's are required.
#[test]
// Sets up its own scratch folder directly.
#[allow(clippy::disallowed_methods)]
fn the_images_folder_holds_one_pair_per_app_slot() {
    let dir = std::env::temp_dir().join(format!("dagon-conform-images-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(conform_images(&dir).is_err(), "slot a is required");
    for f in ["good-a.img", "no-confirm-a.img"] {
        std::fs::write(dir.join(f), f).unwrap();
    }
    let one = conform_images(&dir).unwrap();
    assert_eq!((one.good.len(), one.no_confirm.len()), (1, 1));
    std::fs::write(dir.join("good-b.img"), "b").unwrap();
    assert!(conform_images(&dir).is_err(), "a slot needs both images");
    std::fs::write(dir.join("no-confirm-b.img"), "b").unwrap();
    let two = conform_images(&dir).unwrap();
    assert_eq!((two.good.len(), two.no_confirm.len()), (2, 2));
    assert_eq!(two.good[1], b"b");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A conformance request answered by the node-side session, as a
/// conformance build does: the test CONF_RUN took runs after the answer, on
/// the MCU definition's scratch, apart from every image.
fn conformance(
    session: &RefCell<Session>,
    node: &mut SimNode,
    frame: &[u8],
    max: usize,
) -> Option<Vec<u8>> {
    if !node.responsive() {
        return None;
    }
    let scratch = dagon_topology::defs::Registry::builtin()
        .mcu("stm32f401re")
        .and_then(|m| m.conformance)
        .expect("stm32f401re has a conformance scratch")
        .scratch;
    let setup_regions = node.flash().geometry().to_vec();
    let setup = NodeSetup {
        declared: &setup_regions,
        scratch: scratch.at,
    };
    let mut out = vec![0u8; RESPONSE_MAX];
    let mut session = session.borrow_mut();
    let n = session.handle_frame(frame, max, &mut out)?;
    session.run_pending(&mut *node.flash(), &setup, &mut || {});
    out.truncate(n);
    Some(out)
}

fn is_conformance(frame: &[u8], max: usize) -> bool {
    Frame::decode(frame, max).is_ok_and(|(f, _)| node::CMDS.has(f.cmd))
}

/// The relaying node's USB stream, with RELAYed conformance requests served
/// by the session.
#[derive(Clone)]
struct RelaySim {
    relay: Rc<RefCell<SimRelay>>,
    cut: Rc<RefCell<FrameCutter>>,
    pending: Vec<u8>,
    session: Rc<RefCell<Session>>,
}

impl RelaySim {
    fn relay_conformance(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        let (f, _) = Frame::decode(frame, max_payload::USB).ok()?;
        if f.cmd != Cmd::Relay as u8 {
            return None;
        }
        let r = Relay::parse(f.payload).ok()?;
        if !is_conformance(r.inner, max_payload::I2C) {
            return None;
        }
        let mut relay = self.relay.borrow_mut();
        relay.advance_ms(dagon_sim::relay::PC_ROUND_TRIP_MS);
        let port = relay.links().iter().find(|l| l.id == r.link)?.port;
        let node = relay.bus.i2c_node(port, r.addr)?;
        let inner = conformance(&self.session, node, r.inner, max_payload::I2C);
        // OK with the node's frame as bulk, or the relay's E_TIMEOUT alone.
        let mut out = vec![0u8; frame_len(2 + inner.as_ref().map_or(0, Vec::len))];
        let len = match &inner {
            Some(b) => {
                out[8] = dagon_proto::Status::Ok as u8;
                1 + dagon_proto::payload::encode_bulk_resp(b, &mut out[9..]).ok()?
            }
            None => {
                out[8] = dagon_proto::Status::Timeout as u8;
                1
            }
        };
        let n = dagon_proto::finish(&mut out, flags::RESP, f.seq, f.cmd, len).ok()?;
        out.truncate(n);
        Some(out)
    }
}

impl Transport for RelaySim {
    type Error = ();
    fn max_payload(&self) -> usize {
        dagon_proto::max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let frames = self.cut.borrow_mut().push(bytes, max_payload::USB);
        for f in frames {
            match self.relay_conformance(&f) {
                Some(r) => self.pending.extend(r),
                None => self.relay.borrow_mut().usb_write(&f),
            }
        }
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.pending.extend(self.relay.borrow_mut().usb_read());
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for RelaySim {
    fn now_ms(&self) -> u64 {
        self.relay.borrow().now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.relay.borrow_mut().advance_ms(ms as u64);
    }
}

/// An F401 on the PC's port, framed as on UART.
#[derive(Clone)]
struct DirectSim {
    node: Rc<RefCell<SimNode>>,
    now: Rc<Cell<u64>>,
    cut: Rc<RefCell<FrameCutter>>,
    pending: Vec<u8>,
    session: Rc<RefCell<Session>>,
}

impl Transport for DirectSim {
    type Error = ();
    fn max_payload(&self) -> usize {
        dagon_proto::max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let frames = self.cut.borrow_mut().push(bytes, max_payload::UART);
        let now = self.now.get();
        for f in frames {
            let mut node = self.node.borrow_mut();
            let r = if is_conformance(&f, max_payload::UART) {
                conformance(&self.session, &mut node, &f, max_payload::UART)
            } else {
                node.handle_frame(&f, now)
            };
            self.pending.extend(r.unwrap_or_default());
        }
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for DirectSim {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        let t = self.now.get() + ms as u64;
        self.now.set(t);
        self.node.borrow_mut().tick(t);
    }
}

fn old_image() -> Vec<u8> {
    img(0, Version::new(1, 0, 0))
}

/// The F401's own limits, from its built-in definition.
fn f401() -> dagon_host::HostTimings {
    let m = dagon_topology::defs::Registry::builtin()
        .mcu("stm32f401re")
        .unwrap();
    let reg = dagon_topology::defs::Registry::builtin();
    dagon_host::mcu_timings(m, reg.default_board("stm32f401re")).unwrap()
}

fn sign(m: &[u8]) -> [u8; 64] {
    *key().sk.sign([SIGN_DOMAIN, m].concat(), None)
}

fn assert_all_pass(report: &Report) {
    let failed: Vec<_> = report
        .results
        .iter()
        .filter(|r| r.verdict != Verdict::Pass)
        .map(|r| (Label(r.id).to_string(), r.detail))
        .collect();
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn twelve_of_twelve_through_the_hub() {
    let mut hub = demo_hub(*key().pk, 3);
    let node = hub
        .i2c_node(dagon_sim::NODE_I2C_PORT, dagon_sim::NODE_I2C_ADDR)
        .unwrap();
    node.provision_core(|m| *key().sk.sign(m, None)).unwrap();
    node.provision(&old_image(), 0).unwrap();
    node.trial_wdt_ms = TRIAL_WDT_MS;
    let sim = RelaySim {
        relay: Rc::new(RefCell::new(hub)),
        cut: Rc::default(),
        pending: Vec::new(),
        session: Rc::default(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let (to, link) = seg();
    let report = run(&mut host, to, link, &images(), &mut sign, f401().borrowed());
    assert_all_pass(&report);
    assert_eq!(report.link, LinkKind::I2c);

    let j = json(&report, "seg");
    assert!(j.starts_with("{\n  \"schema\": \"dagon-conform/1\""));
    assert!(j.contains("\"node\": \"seg\"") && j.contains("\"passed\": 12"));
    let h = html(&report, "seg<x>");
    assert!(h.contains("12 of 12 passed") && h.contains("seg&lt;x&gt;"));
    // The link is named as the 構成 names it, in both files.
    assert!(
        j.contains("\"link\": \"i2c\"") && h.contains("Link i2c,"),
        "{h}"
    );
    assert!(!h.contains("<script") && !h.contains("http"));
}

#[test]
fn twelve_of_twelve_on_usb_as_uart() {
    let mut cfg = f401_config();
    cfg.max_payload = max_payload::UART as u16;
    let mut node = SimNode::new(
        cfg,
        *key().pk,
        SimFlash::stm32f401xe(5),
        Layout::of("stm32f401re"),
    );
    node.provision_core(|m| *key().sk.sign(m, None)).unwrap();
    node.provision(&old_image(), 0).unwrap();
    node.trial_wdt_ms = TRIAL_WDT_MS;
    let sim = DirectSim {
        node: Rc::new(RefCell::new(node)),
        now: Rc::default(),
        cut: Rc::default(),
        pending: Vec::new(),
        session: Rc::default(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let report = run(
        &mut host,
        Target::Local,
        DIRECT,
        &images(),
        &mut sign,
        f401().borrowed(),
    );
    assert_all_pass(&report);
    assert!(json(&report, "seg").contains("\"link\": \"uart\""));
}

#[test]
fn unreachable_node_gives_a_full_not_run_file() {
    let hub = demo_hub(*key().pk, 4);
    let sim = RelaySim {
        relay: Rc::new(RefCell::new(hub)),
        cut: Rc::default(),
        pending: Vec::new(),
        session: Rc::default(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let (to, link) = seg();
    let report = run(&mut host, to, link, &images(), &mut sign, f401().borrowed());
    assert_eq!(report.passed(), 0);
    let j = json(&report, "seg");
    assert_eq!(j.matches("\"not_run\"").count(), 12);
    assert!(j.contains("\"mcu_id\": null"));
}
