//! dagon-host against the simulated hub (hub + F401 on I2C), over the USB
//! byte stream, on the simulator's clock.

use std::cell::RefCell;
use std::rc::Rc;

use dagon_conform::timeout::REQUEST_MS;
use dagon_format::header::{image_kind, target_tag};
use dagon_format::Version;
use dagon_host::{
    rom, Clock, ConfirmOutcome, Error, Event, Host, NodeOutcome, Outcome, RecoverAssets,
    RecoverPath, Target, Transport,
};
use dagon_port::Flash;
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

const YAML: &str = include_str!("../../../topology/sim-pair.yaml");

/// The simulated hub's `target_tag`, as text.
fn hub_tag() -> String {
    let tag = dagon_sim::hub_config([0; 32]).target_tag;
    let end = tag.iter().position(|&b| b == 0).unwrap_or(tag.len());
    String::from_utf8_lossy(&tag[..end]).into_owned()
}

/// The sim topology with the hub tag the simulator actually accepts.
fn yaml() -> String {
    YAML.replace("tag: hub-rp2040-lcd", &format!("tag: {}", hub_tag()))
}

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
    img("rp2040", &hub_tag(), slot, v, 0x11 + slot)
}

fn topology(yaml: &str) -> Vec<u8> {
    dagon_topology::compile_str(yaml).unwrap()
}

/// Release 1.1.0 for hub and seg, both slots.
fn release() -> Vec<u8> {
    let t = dagon_topology::parse_str(&yaml()).unwrap();
    let v = Version::new(1, 1, 0);
    let images = [seg_img(0, v), seg_img(1, v), hub_img(0, v), hub_img(1, v)];
    dagon_sign::release::build(&t, &topology(&yaml()), &images, "r1.1", &key())
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
        faults: Rc::default(),
    }
}

/// Injected link faults, keyed by the request's command (the inner one for
/// RELAY).
#[derive(Default)]
struct Faults {
    /// (command, n): the next n requests never reach the hub.
    drop_req: Vec<(u8, u32)>,
    /// (command, k): the response to every k-th such request is lost after
    /// the hub (and node) handled it.
    drop_resp_every: Option<(u8, usize)>,
    /// The response to the next request with this command is lost.
    drop_resp_first: Option<u8>,
    /// The node holds NRST low during the next RELAY (it NACKs), then is
    /// released.
    nack_once: bool,
    /// Every request written: (command, RETRY flag set).
    seen: Vec<(u8, bool)>,
}

/// Command and RETRY flag of a request frame (inner frame for RELAY).
fn effective(b: &[u8]) -> (u8, bool) {
    const INNER: usize = 8 + 4;
    let at = if b[5] == Cmd::Relay as u8 { INNER } else { 0 };
    (b[at + 5], b[at + 3] & flags::RETRY != 0)
}

#[derive(Clone)]
struct Sim {
    hub: Rc<RefCell<SimHub>>,
    pending: Vec<u8>,
    reopens: Rc<RefCell<u32>>,
    faults: Rc<RefCell<Faults>>,
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
        let (cmd, retry) = effective(bytes);
        let mut f = self.faults.borrow_mut();
        f.seen.push((cmd, retry));
        if let Some(d) = f.drop_req.iter_mut().find(|d| d.0 == cmd && d.1 > 0) {
            d.1 -= 1;
            return Ok(());
        }
        let mut hub = self.hub.borrow_mut();
        let nack = bytes[5] == Cmd::Relay as u8 && std::mem::take(&mut f.nack_once);
        if nack {
            hub.bus.set_reset(true);
        }
        hub.usb_write(bytes);
        if nack {
            hub.bus.set_reset(false);
        }
        let n = f.seen.iter().filter(|s| s.0 == cmd).count();
        let every = f
            .drop_resp_every
            .is_some_and(|(c, k)| c == cmd && n.is_multiple_of(k));
        let first = f.drop_resp_first == Some(cmd);
        if first {
            f.drop_resp_first = None;
        }
        if every || first {
            hub.usb_read();
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
        h.apply(&release(), &topology(&yaml()), &key().pk, None, &mut |e| {
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
    let other = topology(&yaml().replace("addr: 0x42", "addr: 0x43"));
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
    let r = block_on(h.apply(&b, &topology(&yaml()), &key().pk, None, &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))));
}

#[test]
fn without_host_verify_the_node_rejects_tampered_images() {
    use dagon_format::bundle::{dest, BundleView};
    use dagon_format::header::{HEADER_SIZE, SIGNED_LEN};
    use dagon_proto::Status;
    let cases = [
        (0x18, Status::Signature, "E_SIGNATURE"),
        (HEADER_SIZE + 100, Status::Digest, "E_DIGEST"),
        (SIGNED_LEN + 5, Status::Signature, "E_SIGNATURE"),
    ];
    for (at, status, text) in cases {
        let s = sim();
        let mut b = release();
        let seg_items: Vec<usize> = {
            let v = BundleView::parse(&b).unwrap();
            (0..v.item_count())
                .map(|i| v.item(i))
                .filter(|it| it.dest == dest::SUB_STORE)
                .map(|it| it.offset as usize)
                .collect()
        };
        for off in seg_items {
            b[off + at] ^= 1;
        }
        let topo = topology(&yaml());
        let mut h = host(&s);
        assert!(block_on(h.apply(&b, &topo, &key().pk, None, &mut |_| {})).is_err());
        let out = block_on(h.apply_unverified(&b, &topo, None, &mut |_| {})).unwrap();
        match &out[0].outcome {
            Outcome::Failed(Error::Rejected { status: st, .. }) => assert_eq!(*st, status),
            o => panic!("flip at {at:#x}: {o:?}"),
        }
        assert!(matches!(out[1].outcome, Outcome::NotStarted));
        let screen = s.hub.borrow().screen.clone().unwrap();
        assert_eq!(
            (screen.phase, screen.text.as_slice()),
            (phase::REJECTED, text.as_bytes())
        );
        let st = block_on(h.status(SEG_AT)).unwrap();
        assert_eq!(
            (st.hello.active_version, st.status.active),
            (Version::new(1, 0, 0), 0)
        );
    }
}

#[test]
fn partial_apply_skips_the_hub() {
    let s = sim();
    let mut h = host(&s);
    let out = block_on(h.apply(
        &release(),
        &topology(&yaml()),
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
    let out =
        block_on(h.apply(&release(), &topology(&yaml()), &key().pk, None, &mut |_| {})).unwrap();
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
    let out =
        block_on(h.apply(&release(), &topology(&yaml()), &key().pk, None, &mut |_| {})).unwrap();
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

fn bootcore() -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::BOOTCORE);
    let mcu = "stm32f401re";
    let spec = ImageSpec {
        image_kind: image_kind::BOOTCORE,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::BOOTCORE, 0).unwrap(),
        version: Version::new(1, 0, 0),
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: caps,
    };
    sign_image(&spec, &[0xB0; 2000], &key()).unwrap()
}

#[test]
fn rom_plan_matches_the_provision_binary() {
    let (boot, app) = (bootcore(), seg_img(0, Version::new(1, 1, 0)));
    let log = rom::initial_log(app[..512].try_into().unwrap()).unwrap();
    let want = dagon_sign::provision::f401_flash(
        (
            &image::slot("stm32f401re", image_kind::BOOTCORE, 0).unwrap(),
            &boot,
        ),
        (
            &image::slot("stm32f401re", image_kind::APP, 0).unwrap(),
            &app,
        ),
    );
    let mut got = vec![0xFF; want.len()];
    for r in rom::f401_regions(&boot, &app, &log) {
        let off = (r.addr - rom::F401_BOOTCORE) as usize;
        got[off..off + r.data.len()].copy_from_slice(r.data);
    }
    assert!(got == want, "ROM regions and provision binary differ");
}

/// Erases the whole F401 and resets it: nothing answers on I2C any more.
fn wipe_seg(s: &Sim) {
    let mut hub = s.hub.borrow_mut();
    let now = hub.now_ms();
    let node = hub.i2c_node(NODE_I2C_ADDR).unwrap();
    let mut i = 0;
    while let Some((a, l)) = node.flash().sector_addr(i) {
        Flash::erase(node.flash(), a, l).unwrap();
        i += 1;
    }
    node.reset(now);
}

fn recover_seg(s: &Sim) -> (RecoverPath, Version) {
    let (rel, topo, boot) = (release(), topology(&yaml()), bootcore());
    let assets = RecoverAssets {
        release: &rel,
        topology: &topo,
        pubkey: &key().pk,
        bootcore: Some(&boot),
    };
    block_on(host(s).recover(SEG, &assets, &mut |_| {})).unwrap()
}

#[test]
fn erased_f401_comes_back_through_the_rom_loader() {
    // A V1.1 command list (17 codes) as on the F401.
    let v11 = [
        0x00, 0x01, 0x02, 0x11, 0x21, 0x31, 0x32, 0x44, 0x45, 0x63, 0x64, 0x73, 0x74, 0x82, 0x83,
        0x92, 0x93,
    ];
    // (no_stretch, 17-code list, expected over-reads). Strict reads fail an
    // over-read: only the 19-byte Get guess against a shorter list may
    // over-read, and the driver then re-reads Get exactly.
    for (no_stretch, full_list, over) in [(true, true, 0), (true, false, 1), (false, false, 1)] {
        let s = sim();
        {
            let mut hub = s.hub.borrow_mut();
            let rom = &mut hub.i2c_node(NODE_I2C_ADDR).unwrap().rom;
            rom.strict_reads = true;
            if full_list {
                rom.commands = v11.to_vec();
            }
            if !no_stretch {
                rom.commands.retain(|&c| c != 0x32 && c != 0x45);
            }
        }
        wipe_seg(&s);
        assert!(
            block_on(host(&s).hello(SEG_AT)).is_err(),
            "silent after erase"
        );

        let (path, v) = recover_seg(&s);
        let RecoverPath::Rom(stats) = path else {
            panic!("expected the ROM path, got {path:?}");
        };
        assert_eq!((stats.no_stretch, stats.attempts), (no_stretch, 1));
        assert_eq!(v, Version::new(1, 1, 0));
        let st = block_on(host(&s).status(SEG_AT)).unwrap();
        assert_eq!((st.hello.active_version, st.hello.trial), (v, false));
        assert_eq!((st.status.active, st.status.confirmed), (0, 0));
        let mut hub = s.hub.borrow_mut();
        assert_eq!(hub.screen.as_ref().unwrap().phase, phase::CONFIRM);
        let rom = &hub.i2c_node(NODE_I2C_ADDR).unwrap().rom;
        assert_eq!((rom.timeouts, rom.over_reads), (0, over));
    }
}

#[test]
fn rom_session_gives_up_after_three_attempts() {
    let s = sim();
    {
        let mut hub = s.hub.borrow_mut();
        // 128 KiB sector 5 then stays BUSY for 12.8 s.
        hub.i2c_node(NODE_I2C_ADDR).unwrap().rom.erase_ms_per_kib = 100;
    }
    wipe_seg(&s);
    let (rel, topo, boot) = (release(), topology(&yaml()), bootcore());
    let assets = RecoverAssets {
        release: &rel,
        topology: &topo,
        pubkey: &key().pk,
        bootcore: Some(&boot),
    };
    let r = block_on(host(&s).recover(SEG, &assets, &mut |_| {}));
    assert!(
        matches!(r, Err(Error::Rom(dagon_host::RomError::Busy))),
        "{r:?}"
    );
    let mut hub = s.hub.borrow_mut();
    assert_eq!(hub.screen.as_ref().unwrap().phase, phase::FAILED);
    assert!(
        !hub.i2c_node(NODE_I2C_ADDR).unwrap().boot0(),
        "BOOT0 left low"
    );
}

#[test]
fn answering_node_is_recovered_by_a_normal_transfer() {
    let s = sim();
    let (path, v) = recover_seg(&s);
    assert_eq!((path, v), (RecoverPath::Retransfer, Version::new(1, 1, 0)));
    let st = block_on(host(&s).status(SEG_AT)).unwrap();
    assert_eq!((st.hello.active_version, st.status.active), (v, 1));
}

#[test]
fn rom_path_needs_a_valid_boot_core() {
    let s = sim();
    wipe_seg(&s);
    let (rel, topo) = (release(), topology(&yaml()));
    let mut boot = bootcore();
    boot[600] ^= 1;
    let assets = RecoverAssets {
        release: &rel,
        topology: &topo,
        pubkey: &key().pk,
        bootcore: Some(&boot),
    };
    let r = block_on(host(&s).recover(SEG, &assets, &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))));
}

const HELLO: u8 = Cmd::Hello as u8;
const XFER_BEGIN: u8 = Cmd::XferBegin as u8;

/// A host whose session with the hub and the seg node is already open, and
/// the fault log cleared.
fn opened(s: &Sim) -> Host<Sim, Sim> {
    let mut h = host(s);
    block_on(h.hello(Target::Hub)).unwrap();
    block_on(h.hello(SEG_AT)).unwrap();
    s.faults.borrow_mut().seen.clear();
    h
}

fn count(s: &Sim, cmd: Cmd) -> usize {
    s.faults
        .borrow()
        .seen
        .iter()
        .filter(|x| x.0 == cmd as u8)
        .count()
}

fn xfer_begin(slot: u8, hdr_len: u16) -> Vec<u8> {
    let mut b = [0u8; 8];
    let n = dagon_proto::payload::XferBegin {
        target: dagon_proto::payload::target::APP,
        slot,
        total_len: hdr_len as u32 + 100,
        hdr_len,
    }
    .encode(&mut b)
    .unwrap();
    b[..n].to_vec()
}

#[test]
fn a_new_session_never_gets_the_previous_sessions_stored_reply() {
    let s = sim();
    // Session A: its first request to the node, refused (bad header length).
    let mut a = host(&s);
    let r = block_on(a.call(SEG_AT, Cmd::XferBegin, &xfer_begin(1, 0), REQUEST_MS));
    assert!(matches!(r, Err(Error::Rejected { .. })), "{r:?}");
    drop(a);
    // Session B restarts its seq count. Its first XFER_BEGIN, valid this time,
    // must run on the node, not get A's stored refusal back.
    let mut b = host(&s);
    let r = block_on(b.call(SEG_AT, Cmd::XferBegin, &xfer_begin(1, 512), REQUEST_MS));
    assert!(r.is_ok(), "{r:?}");
    let seen = s.faults.borrow().seen.clone();
    let opening = seen.iter().position(|x| x.0 == XFER_BEGIN).unwrap();
    assert!(
        seen[..opening].iter().any(|x| x.0 == HELLO),
        "opening HELLO first"
    );
}

#[test]
fn a_session_opens_each_target_once() {
    let s = sim();
    let mut h = host(&s);
    for _ in 0..3 {
        block_on(h.status(SEG_AT)).unwrap();
    }
    // 1 opening HELLO + 3 × (HELLO + STATUS).
    assert_eq!((count(&s, Cmd::Hello), count(&s, Cmd::Status)), (4, 3));
}

#[test]
fn lost_requests_are_resent_after_300_ms_and_backoff() {
    let s = sim();
    let mut h = opened(&s);
    s.faults.borrow_mut().drop_req.push((HELLO, 2));
    let t0 = s.hub.borrow().now_ms();
    let v = block_on(h.hello(SEG_AT)).unwrap().active_version;
    assert_eq!(v, Version::new(1, 0, 0));
    let seen = s.faults.borrow().seen.clone();
    assert_eq!(seen, [(HELLO, false), (HELLO, true), (HELLO, true)]);
    // Each lost attempt waits its full answer time: the node's 300 ms plus
    // 300 ms for the hub's RELAY reply. Then 50 and 100 ms backoff.
    let waited = s.hub.borrow().now_ms() - t0;
    let floor = 2 * (REQUEST_MS as u64 * 2) + 50 + 100;
    assert!((floor..floor + 100).contains(&waited), "{waited} ms");
}

#[test]
fn resending_stops_after_three_retries() {
    let s = sim();
    let mut h = opened(&s);
    s.faults.borrow_mut().drop_req.push((HELLO, 10));
    let r = block_on(h.hello(SEG_AT));
    assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
    assert_eq!(count(&s, Cmd::Hello), 4);
}

#[test]
fn a_transient_nack_is_resent_and_then_answered() {
    let s = sim();
    let mut h = opened(&s);
    // NRST held low during the next RELAY: the node NACKs, the hub answers
    // E_TIMEOUT; the node is released (and boots) before the resend.
    s.faults.borrow_mut().nack_once = true;
    let v = block_on(h.hello(SEG_AT)).unwrap().active_version;
    assert_eq!(v, Version::new(1, 0, 0));
    let seen = s.faults.borrow().seen.clone();
    assert_eq!(seen, [(HELLO, false), (HELLO, true)]);
}

#[test]
fn an_absent_node_is_tried_four_times() {
    let s = sim();
    let mut h = opened(&s);
    let r = block_on(h.hello(Target::I2c(0x43)));
    assert!(matches!(r, Err(Error::Relay(dagon_proto::Status::Timeout))));
    let seen = s.faults.borrow().seen.clone();
    assert_eq!(seen.len(), 4, "opening HELLO sent and resent 3 times");
    assert!(seen[1..].iter().all(|&(c, retry)| c == HELLO && retry));
}

#[test]
fn subctl_and_i2c_raw_are_never_resent() {
    let s = sim();
    let mut h = opened(&s);
    s.faults
        .borrow_mut()
        .drop_req
        .extend([(Cmd::SubCtl as u8, 1), (Cmd::I2cRaw as u8, 1)]);
    let r = block_on(h.call(Target::Hub, Cmd::SubCtl, &[0, 0], REQUEST_MS));
    assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
    let raw = [1u8, 0, 1, 0, 0x00];
    let r = block_on(h.call(Target::Hub, Cmd::I2cRaw, &raw, REQUEST_MS));
    assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
    assert_eq!((count(&s, Cmd::SubCtl), count(&s, Cmd::I2cRaw)), (1, 1));
}

/// Flash writes into seg's slot B (0x08040000..0x08060000).
fn slot_b_writes(s: &Sim) -> Vec<(u32, u32)> {
    let mut hub = s.hub.borrow_mut();
    let f = hub.i2c_node(NODE_I2C_ADDR).unwrap().flash();
    f.ops()
        .iter()
        .filter(|o| {
            o.kind == dagon_sim::OpKind::Write && (0x0804_0000..0x0806_0000).contains(&o.addr)
        })
        .map(|o| (o.addr, o.len))
        .collect()
}

fn apply_seg(s: &Sim) -> Vec<NodeOutcome<()>> {
    block_on(host(s).apply(
        &release(),
        &topology(&yaml()),
        &key().pk,
        Some(&[SEG]),
        &mut |_| {},
    ))
    .unwrap()
}

#[test]
fn lost_xfer_data_replies_do_not_write_twice() {
    let clean = sim();
    assert!(matches!(apply_seg(&clean)[0].outcome, Outcome::Done(_)));
    let want = slot_b_writes(&clean);

    let s = sim();
    // Every third XFER_DATA reply is lost after the node wrote the chunk.
    s.faults.borrow_mut().drop_resp_every = Some((Cmd::XferData as u8, 3));
    let out = apply_seg(&s);
    assert!(matches!(out[0].outcome, Outcome::Done(_)), "{out:?}");
    let f = s.faults.borrow();
    assert!(f.seen.iter().any(|&(c, r)| c == Cmd::XferData as u8 && r));
    drop(f);
    assert_eq!(
        slot_b_writes(&s),
        want,
        "same flash writes as without faults"
    );
}

#[test]
fn a_lost_confirm_reply_is_resent() {
    let s = sim();
    s.faults.borrow_mut().drop_resp_first = Some(Cmd::Confirm as u8);
    let out = apply_seg(&s);
    assert!(matches!(out[0].outcome, Outcome::Done(_)), "{out:?}");
    let f = s.faults.borrow();
    let confirms: Vec<bool> = f
        .seen
        .iter()
        .filter(|x| x.0 == Cmd::Confirm as u8)
        .map(|x| x.1)
        .collect();
    assert_eq!(confirms, [false, true]);
    drop(f);
    let st = block_on(host(&s).status(SEG_AT)).unwrap().status;
    assert_eq!((st.active, st.confirmed), (1, 1));
}

#[test]
fn a_lost_reboot_reply_is_not_resent() {
    let s = sim();
    s.faults.borrow_mut().drop_resp_first = Some(Cmd::Reboot as u8);
    let out = apply_seg(&s);
    assert!(matches!(out[0].outcome, Outcome::Done(_)), "{out:?}");
    assert_eq!(count(&s, Cmd::Reboot), 1);
}

#[test]
fn f401_confirm_lands_inside_a_4_s_trial() {
    let s = sim();
    s.hub
        .borrow_mut()
        .i2c_node(NODE_I2C_ADDR)
        .unwrap()
        .trial_wdt_ms = 4_000;
    let out = block_on(host(&s).apply(
        &release(),
        &topology(&yaml()),
        &key().pk,
        Some(&[SEG]),
        &mut |_| {},
    ))
    .unwrap();
    assert!(matches!(out[0].outcome, Outcome::Done(_)), "{out:?}");
    s.hub.borrow_mut().advance_ms(20_000);
    let st = block_on(host(&s).status(SEG_AT)).unwrap().status;
    assert_eq!(
        (st.active, st.confirmed),
        (1, 1),
        "confirmed, not rolled back"
    );
}
