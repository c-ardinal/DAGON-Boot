//! dagon-host against the simulated hub (hub + F401 on I2C), over the USB
//! byte stream, on the simulator's clock.

use std::cell::RefCell;
use std::rc::Rc;

use dagon_format::header::{image_kind, target_tag};
use dagon_format::Version;
use dagon_host::{Clock, ConfirmOutcome, Error, Event, Host, Outcome, Target, Transport};
use dagon_proto::payload::{phase, SlotState};
use dagon_proto::{flags, Cmd};
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{block_on, demo_hub, SimHub, TrialApp, NODE_I2C_ADDR};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

const YAML: &str = "dagon: 1
name: sim-pair
nodes:
  - id: hub
    mcu: rp2040
    board: waveshare-rp2040-touch-lcd-1.28
    role: coordinator
    port: self-boot
    flash_map: rp2040-4m
    max_attempts: 3
    listen_window_ms: 0
    tag: hub-rp2040
  - id: seg
    mcu: stm32f401re
    board: nucleo-f401re
    role: node
    port: self-boot
    flash_map: f401-default
    max_attempts: 3
    listen_window_ms: 50
    tag: seg-f401-7seg
buses:
  - id: i2c-main
    kind: i2c
    controller: i2c0
    sda: 16
    scl: 17
    speed_hz: 400000
    pullups: external
links:
  - from: hub
    to: seg
    kind: i2c
    bus: i2c-main
    addr: 0x42
    node_pins: { sda: PB4, scl: PA8 }
    control:
      kind: full
      nrst: 18
      boot: 28
      rom_entry: stm32-i2c-rom
recovery:
  hub: [local]
  seg: [local, redistribute, rom-forced]
update:
  order: [seg, hub]
";

const SEG: u8 = 1;
const SEG_AT: Target = Target::I2c(NODE_I2C_ADDR);

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn img(mcu: &str, tag: &str, slot: u8, v: Version, fill: u8) -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::APP);
    let spec = ImageSpec {
        image_kind: image_kind::APP,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::APP, slot).unwrap(),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    };
    sign_image(&spec, &[fill; 3000], &key()).unwrap()
}

fn seg_img(slot: u8, v: Version) -> Vec<u8> {
    img("stm32f401re", "seg-f401-7seg", slot, v, 0x5A + slot)
}

fn hub_img(slot: u8, v: Version) -> Vec<u8> {
    img("rp2040", "hub-rp2040", slot, v, 0x11 + slot)
}

fn topology(yaml: &str) -> Vec<u8> {
    dagon_topology::compile_str(yaml).unwrap()
}

/// Release 1.1.0 for hub and seg, both slots.
fn release() -> Vec<u8> {
    let t = dagon_topology::parse_str(YAML).unwrap();
    let v = Version::new(1, 1, 0);
    let images = [seg_img(0, v), seg_img(1, v), hub_img(0, v), hub_img(1, v)];
    dagon_sign::release::build(&t, &topology(YAML), &images, "r1.1", &key())
        .unwrap()
        .bundle
}

/// Hub and seg both running 1.0.0 from slot A, confirmed.
fn sim() -> Sim {
    let mut hub = demo_hub(*key().pk, 7);
    let v = Version::new(1, 0, 0);
    hub.node.provision(&hub_img(0, v), 0).unwrap();
    hub.i2c_node(NODE_I2C_ADDR)
        .unwrap()
        .provision(&seg_img(0, v), 0)
        .unwrap();
    Sim {
        hub: Rc::new(RefCell::new(hub)),
        pending: Vec::new(),
        reopens: Rc::default(),
        late_reboot: 0,
        held: None,
    }
}

#[derive(Clone)]
struct Sim {
    hub: Rc<RefCell<SimHub>>,
    pending: Vec<u8>,
    reopens: Rc<RefCell<u32>>,
    /// The hub's old firmware keeps answering for this many reopens after
    /// acknowledging REBOOT.
    late_reboot: u32,
    held: Option<Vec<u8>>,
}

impl Transport for Sim {
    type Error = ();
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let is_reboot = bytes.len() > 5 && bytes[3] == 0 && bytes[5] == Cmd::Reboot as u8;
        if is_reboot && self.late_reboot > 0 {
            let mut ack = vec![0u8; dagon_proto::frame_len(1)];
            dagon_proto::finish(&mut ack, flags::RESP, bytes[4], bytes[5], 1).unwrap();
            self.pending.extend(ack);
            self.held = Some(bytes.to_vec());
            return Ok(());
        }
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
        *self.reopens.borrow_mut() += 1;
        if self.held.is_some() {
            self.late_reboot -= 1;
            if self.late_reboot == 0 {
                let mut hub = self.hub.borrow_mut();
                hub.usb_write(&self.held.take().unwrap());
                hub.usb_read();
            }
        }
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

fn host(s: &Sim) -> Host<Sim, Sim> {
    Host::new(s.clone(), s.clone())
}

#[test]
fn status_and_discover() {
    let s = sim();
    let mut h = host(&s);
    let hub = block_on(h.status(Target::Hub)).unwrap();
    assert_eq!(hub.hello.active_version, Version::new(1, 0, 0));
    let seg = block_on(h.status(SEG_AT)).unwrap();
    assert_eq!((seg.status.active, seg.status.confirmed), (0, 0));

    let d = block_on(h.discover()).unwrap();
    assert_eq!(d.nodes.len(), 1);
    assert_eq!(d.nodes[0].0, SEG_AT);
    assert_eq!(d.nodes[0].1.active_version, Version::new(1, 0, 0));
}

#[test]
fn full_apply_then_rollback() {
    let s = sim();
    let mut h = host(&s);
    let mut events = Vec::new();
    let out = block_on(
        h.apply(&release(), &topology(YAML), &key().pk, None, &mut |e| {
            events.push(e)
        }),
    )
    .unwrap();
    let v = Version::new(1, 1, 0);
    let order: Vec<_> = out.iter().map(|o| o.node).collect();
    assert_eq!(order, [SEG, 0], "children first, hub last");
    assert!(out
        .iter()
        .all(|o| matches!(o.outcome, Outcome::Done(x) if x == v)));
    assert!(*s.reopens.borrow() >= 1, "port reopened after hub REBOOT");

    let confirmed = |e: &&Event| e.phase == phase::CONFIRM;
    assert_eq!(events.iter().filter(confirmed).count(), 2);
    assert!(events
        .iter()
        .any(|e| e.phase == phase::TRANSFER && e.percent == 100));
    let screen = s.hub.borrow().screen.clone().unwrap();
    assert_eq!((screen.node, screen.phase), (0, phase::CONFIRM));

    for to in [SEG_AT, Target::Hub] {
        let st = block_on(h.status(to)).unwrap();
        assert_eq!((st.hello.active_version, st.hello.trial), (v, false));
        assert_eq!((st.status.active, st.status.confirmed), (1, 1));
    }
    // Still confirmed after the trial watchdog would have fired.
    s.hub.borrow_mut().advance_ms(30_000);
    assert_eq!(block_on(h.status(SEG_AT)).unwrap().status.active, 1);

    let back = block_on(h.rollback(SEG_AT, SEG, &mut |_| {})).unwrap();
    assert_eq!(back, Version::new(1, 0, 0));
    let st = block_on(h.status(SEG_AT)).unwrap();
    assert_eq!((st.status.active, st.status.confirmed), (0, 0));
    assert_eq!(st.status.slots[0].state, SlotState::Confirmed);
    assert!(!st.hello.trial);
}

#[test]
fn topology_mismatch_refuses_before_sending() {
    let s = sim();
    let mut h = host(&s);
    let other = topology(&YAML.replace("addr: 0x42", "addr: 0x43"));
    let t0 = s.hub.borrow().now_ms();
    let r = block_on(h.apply(&release(), &other, &key().pk, None, &mut |_| {}));
    assert!(matches!(r, Err(Error::TopologyMismatch { .. })));
    assert_eq!(s.hub.borrow().now_ms(), t0, "no request reached the hub");
    assert!(s.hub.borrow().screen.is_none());
}

#[test]
fn tampered_release_refused() {
    let s = sim();
    let mut h = host(&s);
    let mut b = release();
    let n = b.len();
    b[n - 10] ^= 1;
    let r = block_on(h.apply(&b, &topology(YAML), &key().pk, None, &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))));
}

#[test]
fn partial_apply_skips_the_hub() {
    let s = sim();
    let mut h = host(&s);
    let out = block_on(h.apply(
        &release(),
        &topology(YAML),
        &key().pk,
        Some(&[SEG]),
        &mut |_| {},
    ))
    .unwrap();
    assert_eq!(out.len(), 1);
    let hub = block_on(h.hello(Target::Hub)).unwrap();
    assert_eq!(hub.active_version, Version::new(1, 0, 0));
}

#[test]
fn node_that_falls_back_stops_the_run_and_confirm_rule() {
    let s = sim();
    s.hub
        .borrow_mut()
        .i2c_node(NODE_I2C_ADDR)
        .unwrap()
        .trial_app = TrialApp::NeverConfirms;
    let mut h = host(&s);
    let out = block_on(h.apply(&release(), &topology(YAML), &key().pk, None, &mut |_| {})).unwrap();
    assert!(matches!(
        out[0].outcome,
        Outcome::Failed(Error::Rejected { .. })
    ));
    assert!(
        matches!(out[1].outcome, Outcome::NotStarted),
        "hub untouched"
    );
    let screen = s.hub.borrow().screen.clone().unwrap();
    assert_eq!((screen.node, screen.phase), (SEG, phase::FAILED));
    assert_eq!(screen.text, b"E_STATE");

    // On trial with the version this session applied: CONFIRM is sent (and
    // refused by the node's self-check). A fresh session must ask first.
    let r = block_on(h.confirm(SEG_AT, false));
    assert!(matches!(r, Err(Error::Rejected { .. })));
    let mut fresh = host(&s);
    let r = block_on(fresh.confirm(SEG_AT, false)).unwrap();
    assert_eq!(r, ConfirmOutcome::NeedsChoice(Version::new(1, 1, 0)));

    // The trial watchdog takes it back to A; B is Reverted, not a target.
    for _ in 0..4 {
        s.hub.borrow_mut().advance_ms(10_000);
    }
    let st = block_on(h.status(SEG_AT)).unwrap().status;
    assert_eq!((st.active, st.slots[1].state), (0, SlotState::Reverted));
    let r = block_on(h.rollback(SEG_AT, SEG, &mut |_| {}));
    assert!(matches!(r, Err(Error::NoPreviousVersion)));
}

#[test]
fn old_hub_firmware_answering_after_reboot_is_waited_out() {
    let mut s = sim();
    s.late_reboot = 3;
    let mut h = host(&s);
    let out = block_on(h.apply(&release(), &topology(YAML), &key().pk, None, &mut |_| {})).unwrap();
    let v = Version::new(1, 1, 0);
    assert!(
        out.iter()
            .all(|o| matches!(o.outcome, Outcome::Done(x) if x == v)),
        "{out:?}"
    );
    assert!(*s.reopens.borrow() >= 3);
    let hub = block_on(h.status(Target::Hub)).unwrap();
    assert_eq!((hub.status.active, hub.status.confirmed), (1, 1));
}
