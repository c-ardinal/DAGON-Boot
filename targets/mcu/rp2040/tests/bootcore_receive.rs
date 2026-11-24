//! The RP2040 boot core's receiving, through its own receive entry
//! (`dagon_mcu_rp2040::boot_receive`: its links read as one by the pump,
//! served through the shared `dagon_agent::update_mode` by the port the
//! boot core polls), on every link kind the RP2040 definition lets the
//! boot core receive on (`[boot_receive] kinds`), each in the slot the
//! boot core opens it in: the listen window where the kind takes part, a
//! STAY answered by update mode, the 60 s exit, recovery mode, a write and
//! the I2C address the node's topology gives. The app's hand-over is the
//! UPDATE_STAY this package writes and the step consumes, refused when the
//! boot core receives on no link.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::pin::pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;

use dagon_agent::update_mode::{Exit, Stay, EXIT_IDLE_MS};
use dagon_agent::writer::BootCore;
use dagon_agent::Agent;
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::statelog::FaceHeader;
use dagon_mcu_rp2040::bl::{self, HandOverError};
use dagon_mcu_rp2040::boot_receive::{
    link_addr, pump, receive_step, serve, Entry, Links, Mailbox, PumpPort, Sleep, BOOT_LINKS,
};
use dagon_mcu_rp2040::layout::{APP_LOG_LAYOUT, MIN_FLASH_SIZE, SECTOR, XIP_BASE};
use dagon_port::{Flash, FlashRegion, Link, LinkError, LinkKind, Reconnect};
use dagon_proto::payload::{
    mode, reboot_mode, target, HelloReq, HelloResp, SetPending, SlotState, XferBegin, XferChunk,
};
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::{
    hub_config, listen as sim_listen, sample, Layout, Run, SimFlash, SimNode, SimStore,
};
use dagon_topology::defs::{McuDef, Registry};

/// How long one wait of the test link lasts with nothing due.
const TICK_MS: u64 = 5;
/// The boot core's longest sleep between feeds (its watchdog period / 4).
const FEED_EVERY_MS: u32 = 2_000;

fn rp2040() -> McuDef {
    Registry::builtin()
        .mcu("rp2040")
        .expect("built-in RP2040")
        .clone()
}

/// Every kind the RP2040's definition lets the boot core receive on, with
/// whether it takes part in the listen window and the hub's first STAY.
fn kinds() -> Vec<(LinkKind, bool, f64)> {
    let m = rp2040();
    let r = m.boot_receive.as_ref().expect("the boot core receives");
    let all: Vec<_> = r
        .kinds
        .iter()
        .map(|(&k, &takes_part)| {
            let port = LinkKind::from_name(k.name()).expect("a link kind");
            let first = sim_listen::first_stay_ms(k).expect("the simulator has the kind");
            (port, takes_part, first)
        })
        .collect();
    let mut names: Vec<_> = all.iter().map(|k| k.0.name()).collect();
    names.sort();
    assert_eq!(names, ["i2c", "uart", "usb-cdc"]);
    all
}

/// The listen window the boot core opens over the links it opened before
/// deciding (build.rs: the largest the definition gives their kinds).
fn window(kind: LinkKind) -> u16 {
    let m = rp2040();
    let r = m.boot_receive.as_ref().unwrap();
    let k = r.kinds.keys().find(|k| k.name() == kind.name()).unwrap();
    m.listen_window_ms(*k)
}

type TestLinks = Links<TestLink, TestLink, TestLink>;

/// The boot core's links with `link` in the slot of its kind.
fn links(link: TestLink) -> TestLinks {
    match link.kind {
        LinkKind::I2c => Links::new(None, Some(link), None),
        LinkKind::Uart => Links::new(None, None, Some(link)),
        _ => Links::new(Some(link), None, None),
    }
}

/// The RP2040's flash as its definition gives it (4 KiB erase, any number
/// of bytes per write) with an empty app log.
fn rp_flash() -> SimFlash {
    let mut flash = SimFlash::new(
        &[FlashRegion {
            start: XIP_BASE,
            len: MIN_FLASH_SIZE as u32,
            erase_size: SECTOR,
            write_size: 1,
            erased_value: 0xFF,
        }],
        3,
    );
    flash
        .write(
            APP_LOG_LAYOUT.faces[0],
            &FaceHeader { generation: 1 }.to_bytes(),
        )
        .unwrap();
    flash
}

/// A link of one kind on a simulated clock: each frame arrives at its
/// time; with nothing due a read waits one tick or until the next frame is
/// due (moving the clock) and gives up.
struct TestLink {
    kind: LinkKind,
    clock: Rc<Cell<u64>>,
    incoming: VecDeque<(u64, Vec<u8>)>,
    current: Vec<u8>,
    written: Rc<RefCell<Vec<Vec<u8>>>>,
    unanswered: Rc<Cell<usize>>,
}

impl TestLink {
    fn new(kind: LinkKind, clock: &Rc<Cell<u64>>) -> Self {
        TestLink {
            kind,
            clock: clock.clone(),
            incoming: VecDeque::new(),
            current: Vec::new(),
            written: Rc::default(),
            unanswered: Rc::default(),
        }
    }
}

/// Pending once: the link's wait for its next event.
async fn tick() {
    let mut once = false;
    std::future::poll_fn(|_| {
        if std::mem::replace(&mut once, true) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await
}

impl Link for TestLink {
    fn max_payload(&self) -> usize {
        match self.kind {
            LinkKind::I2c => max_payload::I2C,
            LinkKind::Uart => max_payload::UART,
            _ => max_payload::USB,
        }
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }
    fn kind(&self) -> LinkKind {
        self.kind
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        let now = self.clock.get();
        match self.incoming.front() {
            Some(&(at, _)) if at <= now => {
                self.current = self.incoming.pop_front().unwrap().1;
                Ok(&self.current)
            }
            next => {
                let due = next.map_or(u64::MAX, |&(at, _)| at);
                tick().await;
                // Links that share the clock never move it back.
                let to = due.min(now + TICK_MS);
                self.clock.set(self.clock.get().max(to));
                Err(LinkError::Timeout)
            }
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.written.borrow_mut().push(frame.to_vec());
        Ok(())
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.unanswered.set(self.unanswered.get() + 1);
        Ok(())
    }
}

/// The host's sleep: none (the test link's wait moves the clock).
struct NoSleep;

impl Sleep for NoSleep {
    fn arm(&mut self, _: u32) {}
    fn done(&mut self) -> bool {
        true
    }
    fn idle(&mut self) {}
}

fn request(seq: u8, cmd: Cmd, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; dagon_proto::frame_len(payload.len())];
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut b)
    .unwrap();
    b.truncate(n);
    b
}

fn hello(seq: u8) -> Vec<u8> {
    let mut p = [0u8; 32];
    let n = HelloReq {
        proto_ver: dagon_proto::PROTO_VERSION as u32,
        max_payload: max_payload::I2C as u16,
    }
    .encode(&mut p)
    .unwrap();
    request(seq, Cmd::Hello, &p[..n])
}

/// The status and body of a reply frame.
fn reply(f: &[u8]) -> (u8, Status, Vec<u8>) {
    let (f, _) = Frame::decode(f, max_payload::USB).unwrap();
    let (st, body) = split_status(f.payload).unwrap();
    let body = bulk_of(f.cmd, st, body);
    (f.cmd, st, body.to_vec())
}

/// Calls the boot core's verifier makes between the steps of a signature
/// check (the watchdog feed on the hardware).
static TICKS: AtomicUsize = AtomicUsize::new(0);

fn verify_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Verifier = Ed25519Small<fn()>;
type CoreAgent = Agent<SimStore, Chain, Verifier, BootCore, BOOT_LINKS>;

/// The boot core's session in `entry`'s mode over the hub's store, as the
/// boot core builds it: an app chain without app extensions, the verifier
/// that ticks through every check, the links' largest payload.
fn session(store: SimStore, entry: Entry, links: &TestLinks) -> CoreAgent {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    let chain = compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(8000, 8000)),
    );
    let cfg = dagon_agent::Config {
        mode: entry.mode(),
        max_payload: links.max_payload() as u16,
        ..hub_config()
    };
    Agent::with_links(
        cfg,
        Ed25519Small::with_tick(sample::PUBKEY, verify_tick as fn()),
        store,
        chain,
    )
}

fn hub() -> SimNode {
    let mut node = SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(7),
        Layout::of("rp2040"),
    );
    node.store().provision_core(sample::HUB_CORE_1_0_0).unwrap();
    node.provision(sample::HUB_1_0_0, 0).unwrap();
    assert_eq!(node.run(), Run::App(0));
    node
}

/// What the boot core did on one link: the step before the decision, and
/// update or recovery mode when it entered one.
struct Outcome {
    entry: Option<Entry>,
    exit: Option<Exit>,
    /// Time the step ended, and the time the boot core left.
    step_ms: u64,
    left_ms: u64,
    written: Vec<Vec<u8>>,
    unanswered: usize,
    feeds: usize,
}

/// The boot core's receiving as it runs: the step (`receive_step`) on the
/// link when the kind takes part in the window, then, for an entry (or
/// `recovery`), its mode (`serve`) on the same links, the window's STAY
/// answered first; `store` is the session's.
fn boot(link: TestLink, flash: &mut SimFlash, store: SimStore, recovery: bool) -> Outcome {
    let clock = link.clock.clone();
    let written = link.written.clone();
    let unanswered = link.unanswered.clone();
    let kind = link.kind;
    let mut feeds = 0;
    let mut feed = || feeds += 1;
    let mut now = {
        let c = clock.clone();
        move || c.get() * 1000
    };
    // USB is opened only once the boot core receives: the step runs on
    // the I2C and UART links alone.
    let (mut l, usb) = match kind {
        LinkKind::UsbCdc => (Links::new(None, None, None), Some(link)),
        _ => (links(link), None),
    };
    let w = if kind == LinkKind::UsbCdc {
        0
    } else {
        window(kind)
    };
    let found: Option<(Entry, Option<Stay>)> = {
        let mb = Mailbox::new();
        let p = pin!(pump(&mut l, &mb));
        let mut port = PumpPort::new(p, &mb, NoSleep);
        receive_step(
            flash,
            &mut port,
            true,
            &mut now,
            w,
            FEED_EVERY_MS,
            &mut feed,
        )
    };
    let step_ms = clock.get();
    let found = found.or(recovery.then_some((Entry::Recovery, None)));
    let (entry, exit) = match found {
        None => (None, None),
        Some((entry, stay)) => {
            l.usb = usb;
            let mut agent = session(store, entry, &l);
            let mb = if stay.is_some() {
                Mailbox::taken()
            } else {
                Mailbox::new()
            };
            let p = pin!(pump(&mut l, &mb));
            let mut port = PumpPort::new(p, &mb, NoSleep);
            let first = stay.as_ref();
            let exit = serve(
                &mut port,
                &mut agent,
                &mut now,
                FEED_EVERY_MS,
                &mut feed,
                entry,
                first,
            );
            (Some(entry), Some(exit))
        }
    };
    assert!(feeds > 0 || entry.is_none(), "the loop feeds the watchdog");
    let out = written.borrow().clone();
    Outcome {
        entry,
        exit,
        step_ms,
        left_ms: clock.get(),
        written: out,
        unanswered: unanswered.get(),
        feeds,
    }
}

/// The listen window on each kind: a STAY arriving when the hub's first
/// one does keeps the boot core (update mode answers it OK, then a HELLO
/// in mode 1) where the kind takes part; a request other than STAY in the
/// window gets no reply; with no STAY the window ends within 10 ms of its
/// length (the boot time it adds); USB, enumerated first, opens no window.
#[test]
fn the_listen_window_and_stay_on_each_rp2040_link_kind() {
    for (kind, takes_part, first) in kinds() {
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        let at = first.ceil() as u64;
        link.incoming.extend([
            (0, hello(1)),
            (at, request(2, Cmd::Stay, &[])),
            (at + 1, hello(3)),
            (
                at + 2,
                request(
                    4,
                    Cmd::Reboot,
                    &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
                ),
            ),
        ]);
        let o = boot(link, &mut rp_flash(), hub().store().clone(), false);
        if !takes_part {
            assert_eq!(kind, LinkKind::UsbCdc, "only USB takes no part");
            assert_eq!((o.entry, o.step_ms), (None, 0), "{kind:?}: no window");
            assert!(o.written.is_empty());
            continue;
        }
        assert_eq!(
            window(kind),
            rp2040().boot_receive.unwrap().listen_window_ms
        );
        assert_eq!(o.entry, Some(Entry::Stay), "{kind:?}: STAY in the window");
        assert_eq!(
            o.unanswered, 1,
            "{kind:?}: HELLO in the window is not answered"
        );
        assert_eq!(o.exit, Some(Exit::Reboot), "{kind:?}");
        let w = &o.written;
        assert_eq!(
            (reply(&w[0]).0, reply(&w[0]).1),
            (Cmd::Stay as u8, Status::Ok)
        );
        let (_, st, body) = reply(&w[1]);
        assert_eq!(st, Status::Ok);
        assert_eq!(HelloResp::parse(&body).unwrap().mode, mode::UPDATE);

        let clock = Rc::new(Cell::new(0));
        let mut late = TestLink::new(kind, &clock);
        let wms = u64::from(window(kind));
        late.incoming
            .push_back((wms + 1, request(5, Cmd::Stay, &[])));
        let o = boot(late, &mut rp_flash(), hub().store().clone(), false);
        assert_eq!((o.entry, o.written.len()), (None, 0), "{kind:?}: late");
        assert!(
            o.step_ms <= wms + 10,
            "{kind:?}: the window ended at {} ms",
            o.step_ms
        );
    }
}

/// The app's hand-over on this MCU's log, taken by the step on any link
/// kind: REBOOT mode 1 records UPDATE_STAY, the boot core's next step
/// consumes it and enters update mode before any window, the step after
/// does not. A boot core that receives on no link takes no hand-over, and
/// the app refuses it (E_UNSUPPORTED from its store) writing nothing.
#[test]
fn the_hand_over_is_taken_once_and_refused_without_a_link() {
    for (kind, _, _) in kinds() {
        let mut flash = rp_flash();
        bl::request_update_mode(&mut flash, true).unwrap();
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.push_back((
            1,
            request(
                1,
                Cmd::Reboot,
                &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
            ),
        ));
        let o = boot(link, &mut flash, hub().store().clone(), false);
        assert_eq!((o.entry, o.step_ms), (Some(Entry::HandOver), 0), "{kind:?}");
        assert_eq!(o.exit, Some(Exit::Reboot), "{kind:?}");
        assert!(!bl::handed_over(&mut flash), "{kind:?}: consumed for good");
    }
    let mut flash = rp_flash();
    assert_eq!(
        bl::request_update_mode(&mut flash, false),
        Err(HandOverError::Unsupported)
    );
    assert!(!bl::handed_over(&mut flash), "nothing written");
    bl::request_update_mode(&mut flash, true).unwrap();
    let mut none: TestLinks = Links::new(None, None, None);
    let mb = Mailbox::new();
    let p = pin!(pump(&mut none, &mb));
    let mut port = PumpPort::new(p, &mb, NoSleep);
    let entry = receive_step(
        &mut flash,
        &mut port,
        false,
        &mut || 0,
        50,
        2000,
        &mut || {},
    );
    assert!(
        entry.is_none(),
        "a boot core without a link takes no hand-over"
    );
}

/// Update mode on each kind: HELLO answers mode 1, STAY and REBOOT mode 1
/// keep the boot core there, and with no request for 60 s it leaves for
/// a normal restart, the watchdog fed meanwhile.
#[test]
fn update_mode_and_its_60_s_exit_on_each_rp2040_link_kind() {
    assert_eq!(EXIT_IDLE_MS, 60_000);
    for (kind, _, _) in kinds() {
        let mut flash = rp_flash();
        bl::request_update_mode(&mut flash, true).unwrap();
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.extend([
            (5, hello(1)),
            (10, request(2, Cmd::Stay, &[])),
            (
                30_000,
                request(
                    3,
                    Cmd::Reboot,
                    &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
                ),
            ),
        ]);
        let o = boot(link, &mut flash, hub().store().clone(), false);
        assert_eq!(o.exit, Some(Exit::Idle), "{kind:?}");
        let idle = u64::from(EXIT_IDLE_MS);
        assert!(
            (30_000 + idle..=30_000 + idle + TICK_MS).contains(&o.left_ms),
            "{kind:?}: left at {} ms",
            o.left_ms
        );
        assert!(
            o.feeds as u64 >= idle / u64::from(FEED_EVERY_MS),
            "{kind:?}: fed"
        );
        let w = &o.written;
        let (_, st, body) = reply(&w[0]);
        assert_eq!(st, Status::Ok);
        assert_eq!(HelloResp::parse(&body).unwrap().mode, mode::UPDATE);
        assert_eq!(reply(&w[1]).1, Status::Ok, "STAY");
        assert_eq!(reply(&w[2]).1, Status::Ok, "REBOOT mode 1 stays");
    }
}

/// Recovery mode on each kind: HELLO answers mode 2, and the boot core
/// does not leave by itself however long no request comes (it has
/// nothing to start); REBOOT ends it.
#[test]
fn recovery_mode_never_leaves_by_itself_on_each_rp2040_link_kind() {
    for (kind, _, _) in kinds() {
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        let late = 5 * u64::from(EXIT_IDLE_MS);
        // After the listen window, which takes no request but STAY.
        link.incoming.extend([
            (100, hello(1)),
            (
                late,
                request(
                    2,
                    Cmd::Reboot,
                    &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
                ),
            ),
        ]);
        let o = boot(link, &mut rp_flash(), hub().store().clone(), true);
        assert_eq!(
            (o.entry, o.exit),
            (Some(Entry::Recovery), Some(Exit::Reboot))
        );
        assert!(o.left_ms >= late, "{kind:?}: stayed until REBOOT");
        let (_, st, body) = reply(&o.written[0]);
        assert_eq!(st, Status::Ok);
        assert_eq!(HelloResp::parse(&body).unwrap().mode, mode::RECOVERY);
    }
}

/// The boot core's I2C target answers at the address the node's end gives
/// in the topology (its boot wiring's `LINK_ADDR`), not the board's
/// `[default_link] addr`; without one, at the board's; with neither it
/// opens no I2C link.
#[test]
fn the_i2c_target_answers_at_the_topology_address() {
    assert_eq!(link_addr(Some(0x45), Some(0x42)), Some(0x45));
    assert_eq!(link_addr(None, Some(0x42)), Some(0x42));
    assert_eq!(link_addr(None, None), None);
}

fn write_requests() -> Vec<Vec<u8>> {
    let img = sample::HUB_1_0_0;
    let mut out = Vec::new();
    let mut seq = 0u8;
    let mut push = |cmd, p: &[u8]| {
        seq = seq.wrapping_add(1);
        out.push(request(seq, cmd, p));
    };
    let mut p = [0u8; 300];
    let n = XferBegin {
        target: target::APP,
        slot: 0,
        total_len: img.len() as u32,
        hdr_len: 512,
    }
    .encode(&mut p)
    .unwrap();
    push(Cmd::XferBegin, &p[..n]);
    let (hdr, body) = img.split_at(512);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(200).enumerate() {
            let n = XferChunk {
                offset: (i * 200) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            push(cmd, &p[..n]);
        }
    }
    push(Cmd::XferEnd, &[]);
    let n = SetPending {
        target: target::APP,
        slot: 0,
    }
    .encode(&mut p)
    .unwrap();
    push(Cmd::SetPending, &p[..n]);
    push(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
    );
    out
}

/// A write through update mode and through recovery mode on each kind:
/// every request is answered OK on the link it came on, the verifier ticks
/// (feeds the watchdog) through its signature checks, REBOOT mode 0 leaves
/// after its reply was delivered, and the node's next start tries the
/// image written.
#[test]
fn a_write_through_update_and_recovery_mode_on_each_rp2040_link_kind() {
    for (kind, _, _) in kinds() {
        for recovery in [false, true] {
            let mut node = hub();
            let mut flash = rp_flash();
            if !recovery {
                bl::request_update_mode(&mut flash, true).unwrap();
            }
            let clock = Rc::new(Cell::new(0));
            let mut link = TestLink::new(kind, &clock);
            // After the listen window, which takes no request but STAY.
            link.incoming
                .extend(write_requests().into_iter().map(|r| (100, r)));
            let ticks = TICKS.load(Ordering::Relaxed);
            let o = boot(link, &mut flash, node.store().clone(), recovery);
            assert_eq!(o.exit, Some(Exit::Reboot), "{kind:?} {recovery}");
            for w in &o.written {
                let (cmd, st, _) = reply(w);
                assert_eq!(st, Status::Ok, "{kind:?} {recovery} cmd {cmd:#x}");
            }
            assert_eq!(o.written.len(), write_requests().len());
            assert!(
                TICKS.load(Ordering::Relaxed) > ticks,
                "the boot core's verifier feeds the watchdog while it checks"
            );
            node.reset(1);
            assert_eq!(node.run(), Run::App(0), "{kind:?} {recovery}");
            let st = node.status();
            assert_eq!(st.slots[0].state, SlotState::Trying, "{kind:?} {recovery}");
        }
    }
}

/// Update mode on the I2C and UART links at once, each fed `(ms,
/// request)`: the replies each link got, by link, once the boot core left.
fn update_on_two(i2c: Vec<(u64, Vec<u8>)>, uart: Vec<(u64, Vec<u8>)>) -> [Vec<Vec<u8>>; 2] {
    let clock = Rc::new(Cell::new(0));
    let mut a = TestLink::new(LinkKind::I2c, &clock);
    let mut b = TestLink::new(LinkKind::Uart, &clock);
    a.incoming.extend(i2c);
    b.incoming.extend(uart);
    let written = [a.written.clone(), b.written.clone()];
    let mut l: TestLinks = Links::new(None, Some(a), Some(b));
    let mut agent = session(hub().store().clone(), Entry::HandOver, &l);
    let mb = Mailbox::new();
    let p = pin!(pump(&mut l, &mb));
    let mut port = PumpPort::new(p, &mb, NoSleep);
    let mut now = move || clock.get() * 1000;
    let exit = serve(
        &mut port,
        &mut agent,
        &mut now,
        FEED_EVERY_MS,
        &mut || {},
        Entry::HandOver,
        None,
    );
    assert_eq!(exit, Exit::Idle);
    written.map(|w| w.borrow().clone())
}

fn hello_max(seq: u8, max: usize) -> Vec<u8> {
    let mut p = [0u8; 32];
    let n = HelloReq {
        proto_ver: dagon_proto::PROTO_VERSION as u32,
        max_payload: max as u16,
    }
    .encode(&mut p)
    .unwrap();
    request(seq, Cmd::Hello, &p[..n])
}

fn statuses(w: &[Vec<u8>]) -> Vec<Status> {
    w.iter().map(|f| reply(f).1).collect()
}

/// The boot core's links are each another requester: a request over one
/// is not a repeat of the other's with the same seq and command, and each
/// has the ceiling its own HELLO named.
#[test]
fn each_boot_core_link_has_its_own_requester() {
    let small = max_payload::SMALLEST;
    // UART's STATUS, then I2C's with the same seq: no repeat for I2C.
    let [i2c, uart] = update_on_two(
        vec![
            (10, hello_max(1, small)),
            (30, request(9, Cmd::Status, &[])),
        ],
        vec![
            (15, hello_max(3, max_payload::UART)),
            (20, request(9, Cmd::Status, &[])),
        ],
    );
    assert_eq!(statuses(&uart), [Status::Ok, Status::Ok]);
    assert_eq!(
        statuses(&i2c),
        [Status::Ok, Status::Arg],
        "I2C got UART's reply as a repeat"
    );
    // I2C's small HELLO, then UART's requests.
    let [i2c, uart] = update_on_two(
        vec![
            (10, hello_max(1, small)),
            (30, request(9, Cmd::Status, &[])),
        ],
        vec![
            (20, request(9, Cmd::Status, &[])),
            (40, request(2, Cmd::Status, &[])),
        ],
    );
    assert_eq!(
        statuses(&i2c),
        [Status::Ok, Status::Arg],
        "I2C's STATUS is its own request, over its own ceiling"
    );
    assert_eq!(
        statuses(&uart),
        [Status::Ok, Status::Ok],
        "I2C's HELLO does not lower UART's ceiling"
    );
}

/// A transfer's idle deadline on the boot core runs on the link it was
/// begun over: the other link's requests do not keep it.
#[test]
fn a_boot_core_transfer_times_out_on_its_own_link() {
    let idle = u64::from(dagon_agent::IDLE_TIMEOUT_MS);
    let w = write_requests();
    let (begin, hdr) = (w[0].clone(), w[1].clone());
    let i2c = (1..=4)
        .map(|i| (100 + i * idle / 3, request(i as u8, Cmd::Status, &[])))
        .collect();
    let [i2c, uart] = update_on_two(i2c, vec![(100, begin), (200 + 4 * idle / 3, hdr)]);
    assert_eq!(statuses(&i2c), [Status::Ok; 4]);
    assert_eq!(
        statuses(&uart),
        [Status::Ok, Status::State],
        "I2C's requests kept UART's transfer"
    );
}

/// The request payload `enc` writes.
#[allow(dead_code)]
fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The bulk of an OK RELAY or I2C_RAW response; any other body as it is.
#[allow(dead_code)]
fn bulk_of(cmd: u8, status: dagon_proto::Status, body: &[u8]) -> &[u8] {
    match dagon_proto::Cmd::from_u8(cmd) {
        Some(c @ (dagon_proto::Cmd::Relay | dagon_proto::Cmd::I2cRaw))
            if status == dagon_proto::Status::Ok =>
        {
            dagon_proto::payload::parse_bulk_resp(c, body).unwrap()
        }
        _ => body,
    }
}
