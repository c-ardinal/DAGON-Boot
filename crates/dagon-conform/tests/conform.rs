//! The conformance run against an in-memory node: the real agent, an
//! in-memory slot store and flash, and a simulated boot core (trial,
//! watchdog, rollback after 3 attempts).

use dagon_port::{SlotStore, StoreError};
use std::cell::RefCell;

use dagon_agent::{Agent, Config, Transition};
use dagon_conform::node::{self, NodeSetup, Session, RESPONSE_MAX};
use dagon_conform::pc::{Channel, Images, NoAnswer, Runner, SetupError};
use dagon_conform::report::{Progress, Report, Verdict};
use dagon_conform::CTS;
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::reason;
use dagon_format::{key_id, sha256, Version};
use dagon_port::timeout::{link_limits, NodeTimings};
use dagon_port::{Flash, FlashError, FlashRegion, LinkKind};
use dagon_proto::handler::Ceiling;
use dagon_proto::payload::*;
use dagon_proto::{plant, split_status, Cmd, Frame, Status, MAX_FRAME};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const TAG: [u8; 16] = target_tag("seg-f401-7seg");
const CAP: usize = 8192;
const V_OLD: Version = Version::new(1, 0, 0);
const V_GOOD: Version = Version::new(1, 1, 0);
const V_NOCONF: Version = Version::new(1, 2, 0);
const TRIAL_WDT_MS: u64 = 4_000;
const MAX_ATTEMPTS: u8 = 3;
const BOOT_MS: u64 = 800;
const SCRATCH: u32 = 0x0800_8000;
const REGIONS: [FlashRegion; 2] = [
    FlashRegion {
        start: 0x0800_0000,
        len: 0x8000,
        erase_size: 0x4000,
        write_size: 4,
        erased_value: 0xFF,
    },
    FlashRegion {
        start: 0x0800_8000,
        len: 0x8000,
        erase_size: 0x8000,
        write_size: 4,
        erased_value: 0xFF,
    },
];

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn verifier() -> Ed25519Compact {
    Ed25519Compact::new(*keypair().pk)
}

fn sign(msg: &[u8]) -> [u8; 64] {
    *keypair().sk.sign([SIGN_DOMAIN, msg].concat(), None)
}

fn image(slot: u8, version: Version, len: usize) -> Vec<u8> {
    let body: Vec<u8> = (0..len as u32)
        .map(|i| (i * 13 + version.raw()) as u8)
        .collect();
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: McuId::of_name("stm32f401re"),
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: 0x0802_0200,
        payload_size: body.len() as u32,
        version,
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(&body),
        key_id: key_id(&keypair().pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    h.signature = sign(&h.to_bytes()[..SIGNED_LEN]);
    [&h.to_bytes()[..], &body].concat()
}

// ---- node flash ---------------------------------------------------------

struct MemFlash {
    /// Reads, erases and writes asked of it.
    calls: usize,
    regions: Vec<FlashRegion>,
    mem: Vec<u8>,
    erase_works: bool,
}

impl MemFlash {
    fn new() -> Self {
        MemFlash {
            calls: 0,
            regions: REGIONS.to_vec(),
            mem: vec![0xFF; 0x10000],
            erase_works: true,
        }
    }
    fn range(&self, addr: u32, len: usize) -> Result<std::ops::Range<usize>, FlashError> {
        let off = addr
            .checked_sub(REGIONS[0].start)
            .ok_or(FlashError::Range)? as usize;
        if off + len > self.mem.len() {
            return Err(FlashError::Range);
        }
        Ok(off..off + len)
    }
}

impl Flash for MemFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &self.regions
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.calls += 1;
        let r = self.range(addr, buf.len())?;
        buf.copy_from_slice(&self.mem[r]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.calls += 1;
        let reg = REGIONS
            .iter()
            .find(|r| r.start <= addr && addr < r.start + r.len)
            .ok_or(FlashError::Range)?;
        if !(addr - reg.start).is_multiple_of(reg.erase_size) || !len.is_multiple_of(reg.erase_size)
        {
            return Err(FlashError::Align);
        }
        let r = self.range(addr, len as usize)?;
        if self.erase_works {
            self.mem[r].fill(0xFF);
        }
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.calls += 1;
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let r = self.range(addr, data.len())?;
        if self.mem[r.clone()].iter().any(|&b| b != 0xFF) {
            return Err(FlashError::Hw);
        }
        self.mem[r].copy_from_slice(data);
        Ok(())
    }
}

// ---- slot store ---------------------------------------------------------

struct Mem {
    /// The APP slots the node has (A, then B).
    slots: usize,
    status: StatusResp,
    bodies: [Vec<u8>; 2],
    headers: [[u8; HEADER_SIZE]; 2],
    /// Last CONFIRM: (slot, previously confirmed slot).
    last_confirm: Option<(u8, u8)>,
}

impl Mem {
    /// Running confirmed 1.0.0 from A; B empty.
    fn new() -> Self {
        let img = image(0, V_OLD, 600);
        let mut hdr = [0u8; HEADER_SIZE];
        hdr.copy_from_slice(&img[..HEADER_SIZE]);
        let mut a = vec![0xFF; CAP];
        a[..600].copy_from_slice(&img[HEADER_SIZE..]);
        let mut status = StatusResp {
            active: 0,
            confirmed: 0,
            ..Default::default()
        };
        status.slots[0] = SlotInfo {
            state: SlotState::Confirmed,
            version: V_OLD,
            digest16: header_digest16(&hdr),
        };
        Mem {
            slots: 2,
            status,
            bodies: [a, vec![0xFF; CAP]],
            headers: [hdr, [0xFF; HEADER_SIZE]],
            last_confirm: None,
        }
    }
}

impl SlotStore for Mem {
    type Transition = Transition;

    fn status(&mut self) -> StatusResp {
        self.status
    }
    fn capacity(&self, target: u8, slot: u8) -> u32 {
        if target == target::APP && usize::from(slot) < self.slots {
            CAP as u32
        } else {
            0
        }
    }
    fn erase(&mut self, _t: u8, slot: u8, len: u32, _: &Transition) -> Result<(), StoreError> {
        self.bodies[slot as usize][..len as usize].fill(0xFF);
        self.headers[slot as usize] = [0xFF; HEADER_SIZE];
        self.status.slots[slot as usize] = SlotInfo::default();
        Ok(())
    }
    fn write(
        &mut self,
        _t: u8,
        slot: u8,
        off: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.bodies[slot as usize][off as usize..][..data.len()].copy_from_slice(data);
        Ok(())
    }
    fn read(&mut self, _t: u8, slot: u8, off: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let src = self.bodies[slot as usize].get(off as usize..off as usize + buf.len());
        buf.copy_from_slice(src.ok_or(StoreError::Range)?);
        Ok(())
    }
    fn write_header(
        &mut self,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.headers[slot as usize] = *h;
        self.status.slots[slot as usize] = SlotInfo {
            state: SlotState::Written,
            version: ImageHeader::parse(h).unwrap().version,
            digest16: header_digest16(h),
        };
        Ok(())
    }
    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        *out = self.headers[slot as usize];
        Ok(())
    }
    fn set_pending(
        &mut self,
        slot: u8,
        _h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.status.slots[slot as usize].state = SlotState::Pending;
        Ok(())
    }
    fn confirm(
        &mut self,
        slot: u8,
        _h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let old = self.status.confirmed;
        self.last_confirm = Some((slot, old));
        if old != slot && (old as usize) < 2 {
            self.status.slots[old as usize].state = SlotState::Written;
        }
        self.status.slots[slot as usize].state = SlotState::Confirmed;
        self.status.confirmed = slot;
        Ok(())
    }
}

// ---- simulated node behind the hub -------------------------------------

#[derive(Default, Clone, Copy)]
struct Faults {
    dead_after_reboot: bool,
    no_rollback: bool,
    wrong_reason: bool,
    erase_does_nothing: bool,
    /// The agent answers OK to XFER_BEGIN for the running slot.
    begin_active_accepted: bool,
    /// The agent answers OK where it should reject an incompatible header.
    compat_ignored: bool,
    /// CONFIRM only lives in RAM: the next boot sees the slot on trial again.
    confirm_not_persisted: bool,
    /// HELLO announces this `max_payload`.
    hello_max_payload: Option<u16>,
    /// The node has app slot A only.
    one_slot: bool,
    /// A node-side test keeps the node from answering for this long.
    test_takes_ms: u64,
    /// The node resets while a node-side test runs: its results are gone.
    resets_in_a_test: bool,
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

/// The app layer's chain of a fresh start (no extensions).
fn chain() -> Chain {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 4000)),
    )
}

struct FakeNode {
    cfg: Config,
    /// The slot store, shared with the bench's boot core.
    mem: &'static RefCell<Mem>,
    agent: Agent<&'static RefCell<Mem>, Chain, Ed25519Compact>,
    flash: MemFlash,
    session: Session,
    max_payload: usize,
    faults: Faults,
    now: u64,
    up_at: u64,
    trial_since: u64,
    attempts: u8,
    seq: u8,
    reboots: u32,
}

impl FakeNode {
    fn new(link: LinkKind, faults: Faults) -> Self {
        let max_payload =
            link_limits(link).map_or(dagon_proto::max_payload::UART, |l| l.max_payload);
        let cfg = Config {
            mcu_id: McuId::of_name("stm32f401re"),
            port_kind: port_kind::SELF_BOOT,
            target_tag: TAG,
            role: role::NODE,
            mode: mode::APP,
            max_payload: max_payload as u16,
            trial_attempts: MAX_ATTEMPTS,
        };
        let mut flash = MemFlash::new();
        flash.erase_works = !faults.erase_does_nothing;
        let mut m = Mem::new();
        if faults.one_slot {
            m.slots = 1;
        }
        let mem: &'static RefCell<Mem> = Box::leak(Box::new(RefCell::new(m)));
        FakeNode {
            agent: Agent::new(cfg.clone(), verifier(), mem, chain()),
            mem,
            cfg,
            flash,
            session: Session::new(),
            max_payload,
            faults,
            now: 0,
            up_at: 0,
            trial_since: 0,
            attempts: 0,
            seq: 0,
            reboots: 0,
        }
    }

    /// Boot core decision on every reset.
    fn boot(&mut self) {
        self.reboots += 1;
        let mut mem = self.mem.borrow_mut();
        let lost = mem.last_confirm.take();
        let st = &mut mem.status;
        if let (true, Some((slot, old))) = (self.faults.confirm_not_persisted, lost) {
            st.slots[slot as usize].state = SlotState::Trying;
            st.confirmed = old;
            if (old as usize) < 2 {
                st.slots[old as usize].state = SlotState::Confirmed;
            }
        }
        if let Some(p) = st.slots[..2]
            .iter()
            .position(|s| s.state == SlotState::Pending)
        {
            st.slots[p].state = SlotState::Trying;
            st.active = p as u8;
            self.attempts = 1;
        } else if st.slots[st.active as usize].state == SlotState::Trying {
            self.attempts += 1;
            if self.attempts > MAX_ATTEMPTS {
                st.slots[st.active as usize].state = SlotState::Reverted;
                st.active = st.confirmed;
                st.last_reason = if self.faults.wrong_reason {
                    reason::NONE
                } else {
                    reason::TRIAL_EXHAUSTED
                };
                self.attempts = 0;
            }
        }
        self.trial_since = self.now;
        self.up_at = if self.faults.dead_after_reboot {
            u64::MAX
        } else {
            self.now + BOOT_MS
        };
        drop(mem);
        self.agent = Agent::new(self.cfg.clone(), verifier(), self.mem, chain());
    }

    /// Rewrites the agent's answer for the injected faults.
    fn misbehave(&mut self, cmd: Cmd, req: &[u8], body: &mut Vec<u8>) {
        let f = self.faults;
        let active = self.mem.borrow().status.active;
        match (cmd, body[0]) {
            (Cmd::XferBegin, s)
                if f.begin_active_accepted
                    && s == Status::State as u8
                    && XferBegin::parse(req).unwrap().slot == active =>
            {
                *body = vec![Status::Ok as u8];
            }
            (Cmd::XferHdr, s) if f.compat_ignored && s == Status::Compat as u8 => {
                let c = XferChunk::parse(req).unwrap();
                let next = c.offset + c.data.len() as u32;
                *body = [&[Status::Ok as u8][..], &next.to_le_bytes()].concat();
            }
            (Cmd::Hello, 0) if f.hello_max_payload.is_some() => {
                // After the status: the core's proto_ver u32, then max_payload.
                let mp = f.hello_max_payload.unwrap_or_default();
                body[5..7].copy_from_slice(&mp.to_le_bytes());
            }
            _ => {}
        }
    }

    /// Trial watchdog of an app that never confirms.
    fn tick(&mut self) {
        loop {
            let st = self.mem.borrow().status;
            let a = st.slots[st.active as usize];
            let fires = a.state == SlotState::Trying
                && a.version == V_NOCONF
                && !self.faults.no_rollback
                && self.now >= self.trial_since + TRIAL_WDT_MS;
            if !fires {
                return;
            }
            self.now = self.trial_since + TRIAL_WDT_MS;
            self.boot();
        }
    }
}

impl Channel for &mut FakeNode {
    fn request(
        &mut self,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
        resp: &mut [u8],
    ) -> Result<usize, NoAnswer> {
        self.now += 3;
        self.tick();
        if self.now < self.up_at {
            self.now += timeout_ms as u64;
            return Err(NoAnswer);
        }
        self.seq = self.seq.wrapping_add(1) & 0x7F;
        let mut req = vec![0u8; MAX_FRAME];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut req)
        .unwrap();
        let mut out = [0u8; RESPONSE_MAX];
        let setup = NodeSetup {
            declared: &REGIONS,
            scratch: SCRATCH,
        };
        let answered = self
            .session
            .handle_frame(&req[..n], self.max_payload, &mut out);
        // The test runs after its CONF_RUN was answered; the node answers
        // nothing meanwhile.
        if self.session.pending() {
            self.session
                .run_pending(&mut self.flash, &setup, &mut || {});
            self.up_at = self.now + self.faults.test_takes_ms;
            if self.faults.resets_in_a_test {
                self.session = Session::new();
            }
        }
        let (frame, reboot) = match answered {
            Some(len) => (out[..len].to_vec(), false),
            None => {
                let r = self.agent.handle(&req[..n]).expect("agent reply");
                (r.frame.to_vec(), r.reboot)
            }
        };
        let (f, _) = Frame::decode(&frame, MAX_FRAME).unwrap();
        assert_eq!(f.seq, self.seq);
        let mut body = f.payload.to_vec();
        self.misbehave(cmd, payload, &mut body);
        resp[..body.len()].copy_from_slice(&body);
        if reboot {
            self.boot();
        }
        Ok(body.len())
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
    fn sleep_ms(&mut self, ms: u32) {
        self.now += ms as u64;
        self.tick();
    }
}

struct Imgs {
    good: &'static [&'static [u8]],
    noconf: &'static [&'static [u8]],
}

/// `images` kept for the whole test run.
fn kept(images: Vec<Vec<u8>>) -> &'static [&'static [u8]] {
    let refs: Vec<&'static [u8]> = images
        .into_iter()
        .map(|i| &*Box::leak(i.into_boxed_slice()))
        .collect();
    Box::leak(refs.into_boxed_slice())
}

impl Imgs {
    fn new() -> Self {
        Self::slots(2)
    }
    /// Images for a node with `n` app slots.
    fn slots(n: u8) -> Self {
        Imgs {
            good: kept((0..n).map(|s| image(s, V_GOOD, 3000)).collect()),
            noconf: kept((0..n).map(|s| image(s, V_NOCONF, 2000)).collect()),
        }
    }
    fn images(&self) -> Images<'static> {
        Images {
            good: self.good,
            no_confirm: self.noconf,
        }
    }
}

fn run(node: &mut FakeNode, link: LinkKind) -> Report {
    let imgs = Imgs::new();
    let mut signer = sign;
    Runner::new(
        node,
        link,
        imgs.images(),
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .unwrap()
    .run_all()
}

fn verdict(r: &Report, id: u8) -> Verdict {
    r.results.iter().find(|x| x.id == id).unwrap().verdict
}

fn assert_all_pass(r: &Report) {
    let failed: Vec<_> = r
        .results
        .iter()
        .filter(|x| x.verdict != Verdict::Pass)
        .collect();
    assert!(failed.is_empty(), "{:#?}", failed);
    assert_eq!(r.passed(), 12);
}

// ---- tests ----------------------------------------------------------------

#[test]
fn twelve_of_twelve_over_uart_and_again_from_the_new_slot() {
    let mut n = FakeNode::new(LinkKind::Uart, Faults::default());
    let r = run(&mut n, LinkKind::Uart);
    assert_all_pass(&r);
    let st = n.mem.borrow().status;
    assert_eq!((st.active, st.confirmed), (1, 1));
    assert!(n.reboots >= 4 + MAX_ATTEMPTS as u32);
    // A second iteration starts from slot B and must pass too.
    let r2 = run(&mut n, LinkKind::Uart);
    assert_all_pass(&r2);
    assert_eq!(n.mem.borrow().status.active, 0);
}

#[test]
fn twelve_of_twelve_over_i2c_with_fragmented_header() {
    let mut n = FakeNode::new(LinkKind::I2c, Faults::default());
    let r = run(&mut n, LinkKind::I2c);
    assert_all_pass(&r);
    let ct09 = r.results.iter().find(|x| x.id == 9).unwrap();
    assert!(ct09.measured > 0);
}

#[test]
fn erase_that_does_nothing_fails_ct02() {
    let faults = Faults {
        erase_does_nothing: true,
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let r = run(&mut n, LinkKind::Uart);
    assert_eq!(verdict(&r, 2), Verdict::Fail);
    assert_eq!(verdict(&r, 1), Verdict::Pass);
}

/// A node with one app slot: the tests that write a slot other than the
/// running one are not run (there is none); the others pass.
#[test]
fn a_one_slot_node_runs_every_test_that_needs_no_second_slot() {
    let faults = Faults {
        one_slot: true,
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let imgs = Imgs::slots(1);
    let mut signer = sign;
    let r = Runner::new(
        &mut n,
        LinkKind::Uart,
        imgs.images(),
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .unwrap()
    .run_all();
    for id in [9, 11, 12, 18, 20] {
        assert_eq!(verdict(&r, id), Verdict::NotRun, "CT-{id}");
    }
    for id in [1, 2, 3, 4, 7, 8, 10] {
        assert_eq!(verdict(&r, id), Verdict::Pass, "CT-{id}");
    }
    let st = n.mem.borrow().status;
    assert_eq!((st.active, st.confirmed), (0, 0));
}

/// Images for more slots than the wire has, or a different number of good
/// and no-confirm images, are refused at setup.
#[test]
fn setup_takes_one_image_per_slot_of_each_kind() {
    let mut signer = sign;
    let mut n = FakeNode::new(LinkKind::Uart, Faults::default());
    let two = Imgs::new();
    let one = Imgs::slots(1);
    for (good, no_confirm) in [(two.good, one.noconf), (&[][..], &[][..])] {
        let e = Runner::new(
            &mut n,
            LinkKind::Uart,
            Images { good, no_confirm },
            &mut signer,
            NodeTimings::FALLBACK,
        )
        .err();
        assert_eq!(e, Some(SetupError::Slots));
    }
}

#[test]
fn missing_rollback_fails_ct12_and_ct18() {
    let faults = Faults {
        no_rollback: true,
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let r = run(&mut n, LinkKind::Uart);
    assert_eq!(verdict(&r, 11), Verdict::Pass);
    assert_eq!(verdict(&r, 12), Verdict::Fail);
    assert_eq!(verdict(&r, 18), Verdict::Fail);
}

#[test]
fn wrong_rollback_reason_fails_ct12() {
    let faults = Faults {
        wrong_reason: true,
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let r = run(&mut n, LinkKind::Uart);
    let ct12 = r.results.iter().find(|x| x.id == 12).unwrap();
    assert_eq!(ct12.verdict, Verdict::Fail);
    assert!(std::str::from_utf8(ct12.detail.as_bytes())
        .unwrap()
        .contains("last_reason"));
}

#[test]
fn node_that_never_returns_fails_reboot_tests() {
    let faults = Faults {
        dead_after_reboot: true,
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let r = run(&mut n, LinkKind::Uart);
    for id in [10, 11, 12, 18] {
        assert_eq!(verdict(&r, id), Verdict::Fail, "CT-{id}");
    }
    let ct10 = r.results.iter().find(|x| x.id == 10).unwrap();
    assert!(ct10.duration_ms >= 30_000);
}

#[test]
fn setup_rejects_bad_inputs() {
    let imgs = Imgs::new();
    let mut signer = sign;
    let mut n = FakeNode::new(LinkKind::Uart, Faults::default());
    let mut same = imgs.images();
    same.no_confirm = same.good;
    let e = Runner::new(
        &mut n,
        LinkKind::Uart,
        same,
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .err();
    assert_eq!(e, Some(SetupError::SameVersion));
    let mut swapped = imgs.images();
    let other_way = [imgs.good[1], imgs.good[0]];
    swapped.good = &other_way;
    let e = Runner::new(
        &mut n,
        LinkKind::Uart,
        swapped,
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .err();
    assert_eq!(e, Some(SetupError::BadImage(0)));
    let mut dead = FakeNode::new(LinkKind::Uart, Faults::default());
    dead.up_at = u64::MAX;
    let e = Runner::new(
        &mut dead,
        LinkKind::Uart,
        imgs.images(),
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .err();
    assert_eq!(e, Some(SetupError::Unreachable));
}

#[test]
fn node_side_geometry_checks() {
    let mut f = MemFlash::new();
    let ok = NodeSetup {
        declared: &REGIONS,
        scratch: SCRATCH,
    };
    let req = |id| ConfRunReq {
        test_id: id,
        param: 0,
    };
    assert!(node::run(&mut f, &ok, req(1), &mut || {}).unwrap().pass);
    let other = [REGIONS[0]];
    let bad = NodeSetup {
        declared: &other,
        ..ok
    };
    assert!(!node::run(&mut f, &bad, req(1), &mut || {}).unwrap().pass);
    let mut overlap = MemFlash::new();
    overlap.regions[1].start = 0x0800_4000;
    let decl = overlap.regions.clone();
    let o = node::run(
        &mut overlap,
        &NodeSetup {
            declared: &decl,
            ..ok
        },
        req(1),
        &mut || {},
    )
    .unwrap();
    assert!(!o.pass);
    assert_eq!(o.detail.as_bytes(), b"regions 0 and 1 overlap");
    let unaligned = NodeSetup {
        scratch: SCRATCH + 4,
        ..ok
    };
    assert!(
        !node::run(&mut f, &unaligned, req(2), &mut || {})
            .unwrap()
            .pass
    );
    assert!(node::run(&mut f, &ok, req(3), &mut || {}).unwrap().pass);
    assert_eq!(node::run(&mut f, &ok, req(4), &mut || {}), None);
}

/// A conformance build serves CONF_LIST, CONF_RUN and CONF_RESULT in front
/// of the agent and nothing else, and one of them with an unknown must
/// field, a field sent twice or a bad length is refused with nothing taken.
#[test]
fn a_conformance_request_is_read_before_anything_is_taken() {
    let mut s = Session::new();
    let ask = |s: &mut Session, cmd: u8, payload: &[u8], ceiling: Ceiling| {
        let mut req = [0u8; 600];
        let mut out = [0u8; RESPONSE_MAX];
        req[8..8 + payload.len()].copy_from_slice(payload);
        let n = dagon_proto::finish(&mut req, 0, 1, cmd, payload.len()).unwrap();
        let len = s.handle_frame(&req[..n], ceiling, &mut out)?;
        let (fr, _) = Frame::decode(&out[..len], 256).unwrap();
        Some(split_status(fr.payload).unwrap().0)
    };
    let wide = Ceiling::from(512usize);
    let ours = [Cmd::ConfList, Cmd::ConfRun, Cmd::ConfResult];
    let cmds = ours.iter().fold(CmdSet::EMPTY, |c, n| c.with(*n as u8));
    assert_eq!(node::CMDS, cmds);
    let mut served = CmdSet::EMPTY;
    plant::each_probe(|n, p| match ask(&mut s, n, p, wide) {
        Some(Status::Field) => served = served.with(n),
        other => assert_eq!(other, None, "{n:#x} is left to the agent"),
    });
    assert_eq!(served, node::CMDS, "what answers is what it says");
    let commands = plant::each(&node::CMDS, |n, plant, p| {
        let st = ask(&mut s, n, p, wide);
        assert_eq!(st, Some(plant.status()), "{plant:?}");
        assert!(!s.pending(), "{plant:?}");
    });
    assert_eq!(commands, 3);
    // A requester that takes the smallest payload a link offers cannot
    // take CONF_RESULT's answer: refused.
    let floor = Ceiling {
        link: 512,
        peer: Some(dagon_proto::max_payload::SMALLEST as u16),
    };
    assert_eq!(
        ask(&mut s, Cmd::ConfResult as u8, &[2], floor),
        Some(Status::Arg)
    );
    // A test that does not run in the node is not taken.
    assert_eq!(
        ask(&mut s, Cmd::ConfRun as u8, &[12], wide),
        Some(Status::Unsupported)
    );
    assert_eq!(
        ask(&mut s, Cmd::ConfResult as u8, &[12], wide),
        Some(Status::Unsupported)
    );
    // Another command is the agent's.
    assert_eq!(ask(&mut s, Cmd::Status as u8, &[], wide), None);
    assert!(!s.pending());
}

/// CONF_RUN answers before the test runs; the test runs in `run_pending`;
/// CONF_RESULT tells its state before and after; a second CONF_RUN is
/// refused until the first has run; CONF_LIST forgets every result.
#[test]
fn a_test_is_taken_then_run_then_its_result_is_read() {
    let mut f = MemFlash::new();
    let setup = NodeSetup {
        declared: &REGIONS,
        scratch: SCRATCH,
    };
    let mut s = Session::new();
    let mut out = [0u8; RESPONSE_MAX];
    let mut ask = |s: &mut Session, cmd: Cmd, payload: &[u8]| {
        let mut req = [0u8; 64];
        req[8..8 + payload.len()].copy_from_slice(payload);
        let n = dagon_proto::finish(&mut req, 0, 1, cmd as u8, payload.len()).unwrap();
        let len = s.handle_frame(&req[..n], 512usize, &mut out).unwrap();
        let (fr, _) = Frame::decode(&out[..len], 512).unwrap();
        let (st, body) = split_status(fr.payload).unwrap();
        (st, body.to_vec())
    };
    let result = |body: &[u8]| {
        let r = ConfResultResp::parse(body).unwrap();
        (r.state, r.run_id, r.verdict, r.measured)
    };

    let (st, body) = ask(&mut s, Cmd::ConfList, &[]);
    assert_eq!(st, Status::Ok);
    assert_eq!(ConfListResp::parse(&body).unwrap().tests, node::TESTS);
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[2]);
    assert_eq!(result(&body), (conf_state::NONE, 0, verdict::PENDING, 0));

    let (st, body) = ask(&mut s, Cmd::ConfRun, &[2]);
    assert_eq!(st, Status::Ok);
    let taken = ConfRunResp::parse(&body).unwrap();
    assert_eq!((taken.verdict, taken.run_id), (verdict::PENDING, 1));
    assert_eq!(f.calls, 0, "the test has not run when CONF_RUN answers");
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[2]);
    assert_eq!(result(&body), (conf_state::RUNNING, 1, verdict::PENDING, 0));
    assert_eq!(ask(&mut s, Cmd::ConfRun, &[3]).0, Status::State);

    let mut feeds = 0;
    s.run_pending(&mut f, &setup, &mut || feeds += 1);
    assert!(f.calls > 0 && feeds > 0 && !s.pending());
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[2]);
    assert_eq!(result(&body), (conf_state::DONE, 1, verdict::PASS, 0x8000));
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[3]);
    assert_eq!(result(&body).0, conf_state::NONE);

    // A failing test keeps its detail; the run's number goes on.
    let bad = NodeSetup {
        scratch: SCRATCH + 4,
        ..setup
    };
    let (_, body) = ask(&mut s, Cmd::ConfRun, &[3]);
    assert_eq!(ConfRunResp::parse(&body).unwrap().run_id, 2);
    s.run_pending(&mut f, &bad, &mut || {});
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[3]);
    let r = ConfResultResp::parse(&body).unwrap();
    assert_eq!((r.state, r.verdict), (conf_state::DONE, verdict::FAIL));
    assert!(!r.detail.is_empty());

    ask(&mut s, Cmd::ConfList, &[]);
    let (_, body) = ask(&mut s, Cmd::ConfResult, &[2]);
    assert_eq!(result(&body), (conf_state::NONE, 0, verdict::PENDING, 0));
}

/// A node-side test that keeps the node from answering passes while it is
/// done within its deadline, and fails once it is not.
#[test]
fn a_node_side_test_has_until_its_deadline() {
    let slow = |ms| {
        faulty(Faults {
            test_takes_ms: ms,
            ..Default::default()
        })
    };
    // CT-01's deadline over UART is 11 s (no work, the link, the margin).
    let limits = link_limits(LinkKind::Uart).unwrap();
    let timings = NodeTimings::<dagon_port::timeout::EraseLayout>::FALLBACK;
    assert_eq!(timings.conf_deadline_s(1, limits), Some(11));
    assert_all_pass(&slow(9_000));
    let r = slow(12_000);
    for id in [1, 2, 3, 7, 8] {
        let ct = r.results.iter().find(|c| c.id == id).unwrap();
        // The tests with work to do have longer deadlines than 12 s.
        let late = matches!(id, 1 | 8);
        assert_eq!(ct.verdict == Verdict::Fail, late, "CT-{id:02}");
        if late {
            assert_eq!(ct.detail.as_bytes(), b"no result in 11 s");
        }
    }
}

#[test]
fn a_node_that_resets_in_a_test_fails_it() {
    let r = faulty(Faults {
        resets_in_a_test: true,
        ..Default::default()
    });
    let ct = r.results.iter().find(|c| c.id == 2).unwrap();
    assert_eq!(ct.verdict, Verdict::Fail);
    assert!(ct.detail.as_bytes().ends_with(b"lost (the node reset)"));
}

#[test]
fn result_file_shape() {
    let mut r = Report::unreachable(LinkKind::I2c);
    r.results[0].verdict = Verdict::Pass;
    r.results[0].measured = 2;
    r.results[1].verdict = Verdict::Fail;
    r.results[1].detail = dagon_conform::Detail::from_bytes(b"a\"b\\c\x01");
    let mut s = String::new();
    let p = Progress {
        iteration: Some(3),
        elapsed_s: None,
    };
    r.write_json(&mut s, "seg", p).unwrap();
    assert_eq!(
        dagon_conform::report::SCHEMA,
        format!("dagon-conform/{}", dagon_conform::report::SCHEMA_VERSION)
    );
    assert!(s.starts_with(
        "{\n  \"schema\": \"dagon-conform/1\",\n  \"node\": \"seg\",\n  \"mcu_id\": null,"
    ));
    assert!(s.contains("\"link\": \"i2c\",\n  \"iteration\": 3,\n  \"elapsed_s\": null,"));
    assert!(s.contains("\"passed\": 1,\n  \"total\": 12,"));
    assert_eq!(s.matches("{\"id\": \"CT-").count(), 12);
    assert!(s.contains(
        "{\"id\": \"CT-01\", \"test_id\": 1, \"name\": \"flash geometry\", \"side\": \"node\", \
         \"verdict\": \"pass\", \"measured\": 2, \"unit\": \"regions\", \"duration_ms\": 0, \
         \"detail\": \"\"}"
    ));
    assert!(s.contains("\"detail\": \"a\\\"b\\\\c\\u0001\"}"));
    assert!(s.contains("\"id\": \"CT-20\""));
    assert!(s.ends_with("}\n  ]\n}\n"));
    let order: Vec<u8> = CTS.iter().map(|c| c.id).collect();
    assert_eq!(order, [1, 2, 3, 7, 8, 4, 20, 9, 10, 11, 12, 18]);
}

fn faulty(faults: Faults) -> Report {
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    run(&mut n, LinkKind::Uart)
}

#[test]
fn accepting_xfer_to_the_running_slot_fails_ct04() {
    let r = faulty(Faults {
        begin_active_accepted: true,
        ..Default::default()
    });
    assert_eq!(verdict(&r, 4), Verdict::Fail);
    assert_eq!(verdict(&r, 9), Verdict::Pass);
}

#[test]
fn ignoring_mcu_id_and_target_tag_fails_ct20() {
    let r = faulty(Faults {
        compat_ignored: true,
        ..Default::default()
    });
    assert_eq!(verdict(&r, 20), Verdict::Fail);
    assert_eq!(verdict(&r, 4), Verdict::Pass);
}

#[test]
fn confirm_lost_on_reboot_fails_ct11_and_ct18() {
    let r = faulty(Faults {
        confirm_not_persisted: true,
        ..Default::default()
    });
    assert_eq!(verdict(&r, 10), Verdict::Pass);
    assert_eq!(verdict(&r, 11), Verdict::Fail);
    assert_eq!(verdict(&r, 18), Verdict::Fail);
}

/// Every link kind of the list is either judged with its own limits or
/// refused at setup; conform never guesses limits for a kind.
#[test]
fn every_link_kind_is_judged_or_refused() {
    let imgs = Imgs::new();
    let mut signer = sign;
    for &kind in LinkKind::ALL {
        let mut n = FakeNode::new(kind, Faults::default());
        let r = Runner::new(
            &mut n,
            kind,
            imgs.images(),
            &mut signer,
            NodeTimings::FALLBACK,
        );
        match link_limits(kind) {
            Some(l) => {
                assert!(l.max_payload > 0 && l.floor_bps > 0, "{kind:?}");
                assert!(r.is_ok(), "{kind:?} has limits but was refused");
            }
            None => assert_eq!(r.err(), Some(SetupError::UnsupportedLink(kind))),
        }
    }
    let ble = SetupError::UnsupportedLink(LinkKind::Ble).to_string();
    assert!(ble.contains("ble"), "{ble}");
}

/// The result file names the link as the 構成 does.
#[test]
fn result_file_names_the_link_kind_as_the_topology_does() {
    let mut s = String::new();
    Report::unreachable(LinkKind::UsbCdc)
        .write_json(&mut s, "hub", Progress::default())
        .unwrap();
    assert!(s.contains("\"link\": \"usb-cdc\","), "{s}");
}

#[test]
fn tiny_hello_max_payload_is_rejected_at_setup() {
    let imgs = Imgs::new();
    let mut signer = sign;
    let faults = Faults {
        hello_max_payload: Some(4),
        ..Default::default()
    };
    let mut n = FakeNode::new(LinkKind::Uart, faults);
    let e = Runner::new(
        &mut n,
        LinkKind::Uart,
        imgs.images(),
        &mut signer,
        NodeTimings::FALLBACK,
    )
    .err();
    assert_eq!(e, Some(SetupError::BadHello));
}

#[test]
fn node_tests_feed_the_watchdog_around_erases() {
    let mut f = MemFlash::new();
    let setup = NodeSetup {
        declared: &REGIONS,
        scratch: SCRATCH,
    };
    for (id, min) in [(2u8, 4 + 0x8000 / 256), (3, 4 + 1)] {
        let mut feeds = 0u32;
        let req = ConfRunReq {
            test_id: id,
            param: 0,
        };
        let o = node::run(&mut f, &setup, req, &mut || feeds += 1).unwrap();
        assert!(o.pass);
        assert!(feeds >= min, "CT-0{id}: {feeds} feeds");
    }
}
