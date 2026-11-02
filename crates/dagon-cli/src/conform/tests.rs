//! The conformance run against the simulator: the F401 behind the hub
//! (RELAY on I2C) and an F401 on the PC's port directly (USB as UART). The
//! simulated node has no CONF_RUN handler, so a conformance build is
//! modelled here: CONF_RUN frames are answered by dagon-conform's node
//! handler over the node's flash; everything else goes to the node.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dagon_conform::node::{self, NodeSetup, RESPONSE_MAX};
use dagon_conform::report::Verdict;
use dagon_format::header::{image_kind, SIGN_DOMAIN};
use dagon_format::Version;
use dagon_port::Flash;
use dagon_proto::payload::Relay;
use dagon_proto::{flags, frame_len, max_payload, Cmd, Frame};
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{demo_hub, f401_config, FrameCutter, Layout, SimFlash, SimHub, SimNode};
use ed25519_compact::{KeyPair, Seed};

use super::*;

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const TRIAL_WDT_MS: u64 = 4_000;

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

fn images() -> [Vec<u8>; 4] {
    let (good, bad) = (Version::new(1, 1, 0), Version::new(1, 2, 0));
    [img(0, good), img(1, good), img(0, bad), img(1, bad)]
}

/// CONF_RUN answered by the node-side handler, as a conformance build does.
fn conf_run(node: &mut SimNode, frame: &[u8], max: usize) -> Option<Vec<u8>> {
    if !node.responsive() {
        return None;
    }
    let active = node.status().active.min(1) as usize;
    let setup_regions = node.flash().geometry().to_vec();
    let setup = NodeSetup {
        declared: &setup_regions,
        scratch: Layout::STM32F401XE.slots[1 - active].0,
    };
    let mut out = vec![0u8; RESPONSE_MAX];
    let n = node::handle_frame(frame, max, node.flash(), &setup, &mut out, &mut || {})?;
    out.truncate(n);
    Some(out)
}

fn is_conf_run(frame: &[u8], max: usize) -> bool {
    Frame::decode(frame, max).is_ok_and(|(f, _)| f.cmd == Cmd::ConfRun as u8)
}

/// The hub's USB stream, with RELAYed CONF_RUN served by the handler.
#[derive(Clone)]
struct HubSim {
    hub: Rc<RefCell<SimHub>>,
    cut: Rc<RefCell<FrameCutter>>,
    pending: Vec<u8>,
}

impl HubSim {
    fn relay_conf_run(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        let (f, _) = Frame::decode(frame, max_payload::USB).ok()?;
        if f.cmd != Cmd::Relay as u8 {
            return None;
        }
        let r = Relay::parse(f.payload).ok()?;
        if !is_conf_run(r.inner, max_payload::I2C) {
            return None;
        }
        assert!(
            r.timeout_ms as u32 >= dagon_conform::timeout::CONF_RUN_MS,
            "CONF_RUN relayed with {} ms",
            r.timeout_ms
        );
        let mut hub = self.hub.borrow_mut();
        hub.advance_ms(dagon_sim::hub::PC_ROUND_TRIP_MS);
        let inner = conf_run(hub.i2c_node(r.addr)?, r.inner, max_payload::I2C);
        let (status, body) = match &inner {
            Some(b) => (dagon_proto::Status::Ok, b.as_slice()),
            None => (dagon_proto::Status::Timeout, &[][..]),
        };
        let mut out = vec![0u8; frame_len(1 + body.len())];
        out[8] = status as u8;
        out[9..9 + body.len()].copy_from_slice(body);
        dagon_proto::finish(&mut out, flags::RESP, f.seq, f.cmd, 1 + body.len()).ok()?;
        Some(out)
    }
}

impl Transport for HubSim {
    type Error = ();
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let frames = self.cut.borrow_mut().push(bytes, max_payload::USB);
        for f in frames {
            match self.relay_conf_run(&f) {
                Some(r) => self.pending.extend(r),
                None => self.hub.borrow_mut().usb_write(&f),
            }
        }
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

impl Clock for HubSim {
    fn now_ms(&self) -> u64 {
        self.hub.borrow().now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.hub.borrow_mut().advance_ms(ms as u64);
    }
}

/// An F401 on the PC's port, framed as on UART.
#[derive(Clone)]
struct DirectSim {
    node: Rc<RefCell<SimNode>>,
    now: Rc<Cell<u64>>,
    cut: Rc<RefCell<FrameCutter>>,
    pending: Vec<u8>,
}

impl Transport for DirectSim {
    type Error = ();
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let frames = self.cut.borrow_mut().push(bytes, max_payload::UART);
        let now = self.now.get();
        for f in frames {
            let mut node = self.node.borrow_mut();
            let r = if is_conf_run(&f, max_payload::UART) {
                conf_run(&mut node, &f, max_payload::UART)
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

impl Clock for DirectSim {
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
    let node = hub.i2c_node(dagon_sim::NODE_I2C_ADDR).unwrap();
    node.provision(&old_image(), 0).unwrap();
    node.trial_wdt_ms = TRIAL_WDT_MS;
    let sim = HubSim {
        hub: Rc::new(RefCell::new(hub)),
        cut: Rc::default(),
        pending: Vec::new(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let to = Target::I2c(dagon_sim::NODE_I2C_ADDR);
    let report = run(&mut host, to, link_of(to), &images(), &mut sign);
    assert_all_pass(&report);
    assert_eq!(report.link, LinkKind::I2cTarget);

    let j = json(&report, "seg");
    assert!(j.starts_with("{\n  \"schema\": \"dagon-conform/1\""));
    assert!(j.contains("\"node\": \"seg\"") && j.contains("\"passed\": 12"));
    let h = html(&report, "seg<x>");
    assert!(h.contains("12 of 12 passed") && h.contains("seg&lt;x&gt;"));
    assert!(!h.contains("<script") && !h.contains("http"));
}

#[test]
fn twelve_of_twelve_on_usb_as_uart() {
    let mut cfg = f401_config(*key().pk);
    cfg.max_payload = max_payload::UART as u16;
    let mut node = SimNode::new(cfg, SimFlash::stm32f401xe(5), Layout::STM32F401XE);
    node.provision(&old_image(), 0).unwrap();
    node.trial_wdt_ms = TRIAL_WDT_MS;
    let sim = DirectSim {
        node: Rc::new(RefCell::new(node)),
        now: Rc::default(),
        cut: Rc::default(),
        pending: Vec::new(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let report = run(
        &mut host,
        Target::Hub,
        link_of(Target::Hub),
        &images(),
        &mut sign,
    );
    assert_all_pass(&report);
    assert!(json(&report, "seg").contains("\"link\": \"uart\""));
}

#[test]
fn unreachable_node_gives_a_full_not_run_file() {
    let hub = demo_hub(*key().pk, 4);
    let sim = HubSim {
        hub: Rc::new(RefCell::new(hub)),
        cut: Rc::default(),
        pending: Vec::new(),
    };
    let mut host = Host::new(sim.clone(), sim);
    let to = Target::I2c(dagon_sim::NODE_I2C_ADDR);
    let report = run(&mut host, to, link_of(to), &images(), &mut sign);
    assert_eq!(report.passed(), 0);
    let j = json(&report, "seg");
    assert_eq!(j.matches("\"not_run\"").count(), 12);
    assert!(j.contains("\"mcu_id\": null"));
}
