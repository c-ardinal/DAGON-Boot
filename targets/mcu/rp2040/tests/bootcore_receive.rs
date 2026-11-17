//! The RP2040 boot core's receiving, on every link kind its MCU definition
//! lets it receive on (`[boot_receive] kinds`): the listen window where the
//! kind takes part, STAY, update mode, its exit after 60 s without a
//! request, recovery mode, and a write through update mode, each through
//! the shared session (dagon-agent as the boot core's writer) and the
//! serving loop every MCU package runs. The app's hand-over (REBOOT mode
//! 1) is the UPDATE_STAY this package's store writes and the boot core
//! consumes.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dagon_agent::bootcore::{listen, Entry, IdleExit, UPDATE_IDLE_MS};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore, Writer};
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::statelog::FaceHeader;
use dagon_format::SigVerifier;
use dagon_mcu_common::serve::{serve, Exit, ServeHost};
use dagon_mcu_rp2040::bl;
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
/// tick a boot core's link reads with, which bounds how far the listen
/// window and the idle exit overrun their time.
const TICK_MS: u64 = 10;

fn rp2040() -> McuDef {
    Registry::builtin()
        .mcu("rp2040")
        .expect("built-in RP2040")
        .clone()
}

/// Every kind the RP2040's definition lets the boot core receive on, with
/// the listen window the definition gives it (0: none).
fn kinds() -> Vec<(LinkKind, u16, f64)> {
    let m = rp2040();
    let r = m.boot_receive.as_ref().expect("the boot core receives");
    let all: Vec<_> = r
        .kinds
        .keys()
        .map(|&k| {
            let port = LinkKind::from_name(k.name()).expect("a link kind");
            let first = sim_listen::first_stay_ms(k).expect("the simulator has the kind");
            (port, m.listen_window_ms(k), first)
        })
        .collect();
    let mut names: Vec<_> = all.iter().map(|k| k.0.name()).collect();
    names.sort();
    assert_eq!(names, ["i2c", "uart", "usb-cdc"]);
    all
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
        max_payload::I2C
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

/// Calls the boot core's update-mode verifier makes between the steps of
/// a signature check (the watchdog feed on the hardware).
static TICKS: AtomicUsize = AtomicUsize::new(0);

fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Verifier = Ed25519Small<fn()>;
type CoreAgent = Agent<SimStore, Chain, Verifier, BootCore>;

/// The boot core's session in `entry`'s mode over the hub's store, with
/// an app chain that carries no app extensions and the verifier that
/// ticks through every check.
fn session(store: SimStore, entry: Entry) -> CoreAgent {
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

/// What the boot core does in update or recovery mode: serves the session
/// until a REBOOT's reply has left (`Exit::Reboot`) or, entered by STAY or
/// the hand-over, until no request came for 60 s (`Exit::Closed` with
/// the idle link expired).
fn receive(link: TestLink, agent: &mut CoreAgent, entry: Entry) -> (Exit, bool, u64) {
    let clock = link.clock.clone();
    let now = {
        let c = clock.clone();
        move || c.get()
    };
    let mut idle = IdleExit::new(link, now, entry);
    let mut host = Host {
        clock: clock.clone(),
        feeds: 0,
    };
    let exit = dagon_sim::block_on(serve(&mut idle, agent, &mut host));
    assert!(host.feeds > 0, "the loop feeds while it serves");
    (exit, idle.expired(), clock.get())
}

/// The listen window on each kind: a STAY that arrives when the hub's
/// first one does is answered OK and keeps the boot core in update mode
/// where the kind takes part; a request other than STAY gets no reply;
/// with no STAY the window ends within one tick of its length (the boot
/// time it adds is at most the window and 10 ms); a kind that does not
/// take part (USB, enumerated first) opens no window and reads nothing.
#[test]
fn the_listen_window_on_each_rp2040_link_kind() {
    for (kind, window, first) in kinds() {
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.push_back((0, hello(1)));
        link.incoming
            .push_back((first.ceil() as u64, request(2, Cmd::Stay, &[])));
        let now = || clock.get();
        let stayed = dagon_sim::block_on(listen(&mut link, window, now));
        if window == 0 {
            assert_eq!(kind, LinkKind::UsbCdc, "only USB opens no window");
            assert!(!stayed);
            assert_eq!(link.incoming.len(), 2, "{kind:?}: nothing read");
            assert_eq!(clock.get(), 0, "{kind:?}: no time added");
            continue;
        }
        assert_eq!(window, rp2040().boot_receive.unwrap().listen_window_ms);
        assert!(stayed, "{kind:?}: STAY in the window");
        assert_eq!(
            link.unanswered, 1,
            "{kind:?}: HELLO in the window is not answered"
        );
        let w = link.written.borrow();
        assert_eq!(w.len(), 1, "{kind:?}");
        assert_eq!(reply(&w[0]).0, Cmd::Stay as u8);
        assert_eq!(reply(&w[0]).1, Status::Ok);
        drop(w);

        let clock = Rc::new(Cell::new(0));
        let mut late = TestLink::new(kind, &clock);
        late.incoming
            .push_back((u64::from(window) + 1, request(3, Cmd::Stay, &[])));
        let now = || clock.get();
        assert!(
            !dagon_sim::block_on(listen(&mut late, window, now)),
            "{kind:?}: late"
        );
        assert!(late.written.borrow().is_empty());
        assert!(
            clock.get() <= u64::from(window) + TICK_MS,
            "{kind:?}: the window ended at {} ms",
            clock.get()
        );
    }
}

/// Update mode on each kind: HELLO answers mode 1 (update), STAY and
/// REBOOT mode 1 keep the boot core there, and with no request for 60 s
/// it leaves for a normal restart; a request resets that time.
#[test]
fn update_mode_and_its_60_s_exit_on_each_rp2040_link_kind() {
    assert_eq!(UPDATE_IDLE_MS, 60_000);
    for (kind, _, _) in kinds() {
        for entry in [Entry::Stay, Entry::HandOver] {
            let mut agent = session(hub().store().clone(), entry);
            let clock = Rc::new(Cell::new(0));
            let mut link = TestLink::new(kind, &clock);
            link.incoming.extend([
                (5, hello(1)),
                (10, request(2, Cmd::Stay, &[])),
                (30_000, request(3, Cmd::Reboot, &[reboot_mode::UPDATE])),
            ]);
            let written = link.written.clone();
            let (exit, expired, at) = receive(link, &mut agent, entry);
            assert_eq!((exit, expired), (Exit::Closed, true), "{kind:?} {entry:?}");
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
        let mut agent = session(hub().store().clone(), Entry::Recovery);
        let clock = Rc::new(Cell::new(0));
        let mut link = TestLink::new(kind, &clock);
        link.incoming.push_back((5, hello(1)));
        link.closes_at = 5 * UPDATE_IDLE_MS;
        let written = link.written.clone();
        let (exit, expired, at) = receive(link, &mut agent, Entry::Recovery);
        assert_eq!((exit, expired), (Exit::Closed, false), "{kind:?}");
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
/// every request is answered OK, the verifier ticks (feeds the watchdog)
/// through its signature checks, REBOOT mode 0 leaves after its reply,
/// and the node's next start tries the image written.
#[test]
fn a_write_through_update_and_recovery_mode_on_each_rp2040_link_kind() {
    for (kind, _, _) in kinds() {
        for entry in [Entry::Stay, Entry::HandOver, Entry::Recovery] {
            let mut node = hub();
            let mut agent = session(node.store().clone(), entry);
            let clock = Rc::new(Cell::new(0));
            let mut link = TestLink::new(kind, &clock);
            link.incoming
                .extend(write_requests().into_iter().map(|r| (1, r)));
            let written = link.written.clone();
            let ticks = TICKS.load(Ordering::Relaxed);
            let (exit, expired, _) = receive(link, &mut agent, entry);
            assert_eq!((exit, expired), (Exit::Reboot, false), "{kind:?} {entry:?}");
            for w in written.borrow().iter() {
                let (cmd, st, _) = reply(w);
                assert_eq!(st, Status::Ok, "{kind:?} {entry:?} cmd {cmd:#x}");
            }
            assert!(
                TICKS.load(Ordering::Relaxed) > ticks,
                "the update-mode verifier feeds the watchdog while it checks"
            );
            node.reset(1);
            assert_eq!(node.run(), Run::App(0), "{kind:?} {entry:?}");
            let st = node.status();
            assert_eq!(st.slots[0].state, SlotState::Trying, "{kind:?} {entry:?}");
        }
    }
}

/// The app's hand-over on this MCU's log: the app's store records
/// UPDATE_STAY (REBOOT mode 1), the boot core's next start finds it and
/// consumes it with UPDATE_DONE, and the start after that does not stay.
#[test]
fn the_hand_over_is_taken_once() {
    // The RP2040's flash as its definition gives it: 4 KiB erase, any
    // number of bytes per write.
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
    assert!(!bl::handed_over(&mut flash), "no stay recorded");
    bl::request_update_mode(&mut flash).unwrap();
    assert!(bl::handed_over(&mut flash), "the next start stays");
    assert!(!bl::handed_over(&mut flash), "consumed for good");
}
