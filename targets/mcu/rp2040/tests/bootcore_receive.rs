//! The RP2040 boot core's receiving, through its own receive entry
//! (`dagon_mcu_rp2040::boot_receive`: the step before the boot decision
//! and update or recovery mode on the boot core's links as one), on every
//! link kind the RP2040 definition lets the boot core receive on
//! (`[boot_receive] kinds`), each in the slot the boot core opens it in:
//! the listen window where the kind takes part, STAY, update mode, its
//! exit after 60 s without a request, recovery mode, and a write. The
//! app's hand-over is the UPDATE_STAY this package writes and the step
//! consumes, refused when the boot core receives on no link.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dagon_agent::bootcore::UPDATE_IDLE_MS;
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore, Writer};
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::statelog::FaceHeader;
use dagon_format::SigVerifier;
use dagon_mcu_common::serve::{Exit, ServeHost};
use dagon_mcu_rp2040::bl::{self, HandOverError};
use dagon_mcu_rp2040::boot_receive::{receive_step, serve, Entry, Links};
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

/// How often the test link's read gives up with nothing complete: the
/// tick the boot core's links read with (5 ms in the listen window), which
/// bounds how far the window and the idle exit overrun their time.
const TICK_MS: u64 = 5;

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
/// time; a read with nothing due by the next tick moves the clock one tick
/// and times out; past `closes_at` the link is closed.
struct TestLink {
    kind: LinkKind,
    clock: Rc<Cell<u64>>,
    incoming: VecDeque<(u64, Vec<u8>)>,
    current: Vec<u8>,
    written: Rc<RefCell<Vec<Vec<u8>>>>,
    unanswered: usize,
    closes_at: u64,
}

impl TestLink {
    fn new(kind: LinkKind, clock: &Rc<Cell<u64>>) -> Self {
        TestLink {
            kind,
            clock: clock.clone(),
            incoming: VecDeque::new(),
            current: Vec::new(),
            written: Rc::default(),
            unanswered: 0,
            closes_at: u64::MAX,
        }
    }
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
        if now >= self.closes_at {
            return Err(LinkError::Closed);
        }
        match self.incoming.front() {
            Some(&(at, _)) if at <= now + TICK_MS => {
                let (at, f) = self.incoming.pop_front().unwrap();
                self.clock.set(now.max(at));
                self.current = f;
                Ok(&self.current)
            }
            _ => {
                self.clock.set(now + TICK_MS);
                Err(LinkError::Timeout)
            }
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.written.borrow_mut().push(frame.to_vec());
        Ok(())
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.unanswered += 1;
        Ok(())
    }
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
    let mut p = [0u8; 8];
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
    (f.cmd, st, body.to_vec())
}

/// Calls the boot core's verifier makes between the steps of a signature
/// check (the watchdog feed on the hardware).
static TICKS: AtomicUsize = AtomicUsize::new(0);

fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Verifier = Ed25519Small<fn()>;
type CoreAgent = Agent<SimStore, Chain, Verifier, BootCore>;

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
    Agent::for_writer(
        cfg,
        Ed25519Small::with_tick(sample::PUBKEY, tick as fn()),
        store,
        chain,
    )
}

fn hub() -> SimNode {
    let mut node = SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_2m(7),
        Layout::RP2040_HUB,
    );
    node.provision(sample::HUB_1_0_0, 0).unwrap();
    assert_eq!(node.run(), Run::App(0));
    node
}

/// The boot core's serving host: its clock, and the feeds the loop gives.
struct Host {
    clock: Rc<Cell<u64>>,
    feeds: usize,
}

impl<S: SlotStore, X: AppChain, V: SigVerifier, W: Writer> ServeHost<S, X, V, W> for Host {
    fn now_ms(&self) -> u64 {
        self.clock.get()
    }
    fn idle(&mut self) {
        self.feeds += 1;
    }
    fn busy(&mut self) {}
    fn feed(&mut self) {
        self.feeds += 1;
    }
    async fn wait_ms(&mut self, _: u32) {}
}

/// The boot core's update or recovery mode on `link`
/// (`boot_receive::serve`): the exit, whether 60 s passed without a
/// request, and the time it left.
fn run_mode(link: TestLink, store: SimStore, entry: Entry) -> (Exit, bool, u64) {
    let clock = link.clock.clone();
    let links = links(link);
    let mut agent = session(store, entry, &links);
    let mut host = Host {
        clock: clock.clone(),
        feeds: 0,
    };
    let now = {
        let c = clock.clone();
        move || c.get()
    };
    let (exit, idle) = dagon_sim::block_on(serve(links, &mut agent, &mut host, entry, now));
    assert!(host.feeds > 0, "the loop feeds while it serves");
    (exit, idle, clock.get())
}

/// The step before the boot decision on `link`: what it decided, the
/// link's replies and unanswered requests, and the time it took.
fn step(link: TestLink, flash: &mut SimFlash) -> (Option<Entry>, Vec<Vec<u8>>, usize, u64) {
    let clock = link.clock.clone();
    let written = link.written.clone();
    let kind = link.kind;
    // USB is opened only once the boot core receives: the step runs on
    // the I2C and UART links alone.
    let mut l = match kind {
        LinkKind::UsbCdc => Links::new(None, None, None),
        _ => links(link),
    };
    let w = if kind == LinkKind::UsbCdc {
        0
    } else {
        window(kind)
    };
    let now = || clock.get();
    let entry = dagon_sim::block_on(receive_step(flash, &mut l, true, w, now));
    let unanswered = l.i2c.iter().chain(&l.uart).map(|x| x.unanswered).sum();
    let out = written.borrow().clone();
    (entry, out, unanswered, clock.get())
}

/// The listen window on each kind: a STAY arriving when the hub's first
/// one does is answered OK and keeps the boot core (update mode) where the
/// kind takes part; a request other than STAY gets no reply; with no STAY
/// the window ends within one tick of its length (the boot time it adds is
/// at most the window and 10 ms); USB, enumerated first, opens no window.
#[test]
fn the_listen_window_on_each_rp2040_link_kind() {
    for (kind, takes_part, first) in kinds() {
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.push_back((0, hello(1)));
        link.incoming
            .push_back((first.ceil() as u64, request(2, Cmd::Stay, &[])));
        let (entry, w, unanswered, at) = step(link, &mut rp_flash());
        if !takes_part {
            assert_eq!(kind, LinkKind::UsbCdc, "only USB takes no part");
            assert_eq!((entry, w.len(), at), (None, 0, 0), "{kind:?}: no window");
            continue;
        }
        assert_eq!(
            window(kind),
            rp2040().boot_receive.unwrap().listen_window_ms
        );
        assert_eq!(entry, Some(Entry::Stay), "{kind:?}: STAY in the window");
        assert_eq!(
            unanswered, 1,
            "{kind:?}: HELLO in the window is not answered"
        );
        assert_eq!(w.len(), 1, "{kind:?}");
        assert_eq!(
            (reply(&w[0]).0, reply(&w[0]).1),
            (Cmd::Stay as u8, Status::Ok)
        );

        let clock = Rc::new(Cell::new(0));
        let mut late = TestLink::new(kind, &clock);
        let wms = u64::from(window(kind));
        late.incoming
            .push_back((wms + 1, request(3, Cmd::Stay, &[])));
        let (entry, w, _, at) = step(late, &mut rp_flash());
        assert_eq!((entry, w.len()), (None, 0), "{kind:?}: late");
        assert!(at <= wms + 10, "{kind:?}: the window ended at {at} ms");
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
        let (entry, _, _, at) = step(TestLink::new(kind, &clock), &mut flash);
        assert_eq!((entry, at), (Some(Entry::HandOver), 0), "{kind:?}");
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
    let entry = dagon_sim::block_on(receive_step(&mut flash, &mut none, false, 50, || 0));
    assert_eq!(entry, None, "a boot core without a link takes no hand-over");
}

/// Update mode on each kind: HELLO answers mode 1 (update), STAY and
/// REBOOT mode 1 keep the boot core there, and with no request for 60 s
/// it leaves for a normal restart; a request resets that time.
#[test]
fn update_mode_and_its_60_s_exit_on_each_rp2040_link_kind() {
    assert_eq!(UPDATE_IDLE_MS, 60_000);
    for (kind, _, _) in kinds() {
        for entry in [Entry::Stay, Entry::HandOver] {
            let clock = Rc::new(Cell::new(0));
            let mut link = TestLink::new(kind, &clock);
            link.incoming.extend([
                (5, hello(1)),
                (10, request(2, Cmd::Stay, &[])),
                (30_000, request(3, Cmd::Reboot, &[reboot_mode::UPDATE])),
            ]);
            let written = link.written.clone();
            let (exit, idle, at) = run_mode(link, hub().store().clone(), entry);
            assert_eq!((exit, idle), (Exit::Closed, true), "{kind:?} {entry:?}");
            assert!(
                (30_000 + UPDATE_IDLE_MS..=30_000 + UPDATE_IDLE_MS + TICK_MS).contains(&at),
                "{kind:?}: left at {at} ms"
            );
            let w = written.borrow();
            let (_, st, body) = reply(&w[0]);
            assert_eq!(st, Status::Ok);
            assert_eq!(HelloResp::parse(&body).unwrap().mode, mode::UPDATE);
            assert_eq!(reply(&w[1]).1, Status::Ok, "STAY");
            assert_eq!(reply(&w[2]).1, Status::Ok, "REBOOT mode 1 stays");
        }
    }
}

/// Recovery mode on each kind: HELLO answers mode 2, and the boot core
/// does not leave by itself however long no request comes (it has
/// nothing to start).
#[test]
fn recovery_mode_never_leaves_by_itself_on_each_rp2040_link_kind() {
    for (kind, _, _) in kinds() {
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.push_back((5, hello(1)));
        link.closes_at = 5 * UPDATE_IDLE_MS;
        let written = link.written.clone();
        let (exit, idle, at) = run_mode(link, hub().store().clone(), Entry::Recovery);
        assert_eq!((exit, idle), (Exit::Closed, false), "{kind:?}");
        assert!(
            at >= 5 * UPDATE_IDLE_MS,
            "{kind:?}: served until the link closed"
        );
        let (_, st, body) = reply(&written.borrow()[0]);
        assert_eq!(st, Status::Ok);
        assert_eq!(HelloResp::parse(&body).unwrap().mode, mode::RECOVERY);
    }
}

/// The requests that write the hub's app into slot A (the boot core may
/// write the slot the app runs from) and set it pending.
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
    push(Cmd::Reboot, &[reboot_mode::NORMAL]);
    out
}

/// A write through update mode and through recovery mode on each kind:
/// every request is answered OK on the link it came on, the verifier ticks
/// (feeds the watchdog) through its signature checks, REBOOT mode 0 leaves
/// after its reply, and the node's next start tries the image written.
#[test]
fn a_write_through_update_and_recovery_mode_on_each_rp2040_link_kind() {
    for (kind, _, _) in kinds() {
        for entry in [Entry::Stay, Entry::HandOver, Entry::Recovery] {
            let mut node = hub();
            let clock = Rc::new(Cell::new(0));
            let mut link = TestLink::new(kind, &clock);
            link.incoming
                .extend(write_requests().into_iter().map(|r| (1, r)));
            let written = link.written.clone();
            let ticks = TICKS.load(Ordering::Relaxed);
            let (exit, idle, _) = run_mode(link, node.store().clone(), entry);
            assert_eq!((exit, idle), (Exit::Reboot, false), "{kind:?} {entry:?}");
            for w in written.borrow().iter() {
                let (cmd, st, _) = reply(w);
                assert_eq!(st, Status::Ok, "{kind:?} {entry:?} cmd {cmd:#x}");
            }
            assert!(
                TICKS.load(Ordering::Relaxed) > ticks,
                "the boot core's verifier feeds the watchdog while it checks"
            );
            node.reset(1);
            assert_eq!(node.run(), Run::App(0), "{kind:?} {entry:?}");
            let st = node.status();
            assert_eq!(st.slots[0].state, SlotState::Trying, "{kind:?} {entry:?}");
        }
    }
}
