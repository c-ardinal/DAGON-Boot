//! The relay on the host: its serving loop, a worker per link and a
//! scripted PC run together on a local executor whose wakers are real.
//! Time is virtual: it moves only when every task waits, to the earliest
//! deadline one of them waits for. The nodes are `dagon_sim`'s: an F401 on
//! an addressed bus (its reset and boot-mode lines wired) and a watch on a
//! UART.

use std::cell::{Cell, RefCell};
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use dagon_agent::snapshot::AppSnapshot;
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Reader, Watchdog};
use dagon_format::Version;
use dagon_port::{
    hello_request, BusError, Flash, FlashError, FlashRegion, Link, LinkError, LinkKind, NodeLink,
    Nrst, Reconnect, SubControl,
};
use dagon_port::{Addressing, RelayLinkDef};
use dagon_proto::payload::{boot0, nrst, I2cRaw, Relay as RelayReq, Show, StatusResp, SubCtl};
use dagon_proto::{finish, frame_len, max_payload, split_status, Cmd, Frame, Status};
use dagon_relay::nodes::{named, Identity};
use dagon_relay::view::Shade;
use dagon_relay::{
    run_link, serve, task, Config, LinkRoute, Liveness, OwnAgent, Port, Recovery, Relay,
    RelayClock, State,
};
use dagon_sim::store::SimStore;
use dagon_sim::{sample_hub, SimBus, NODE_I2C_ADDR, NODE_I2C_PORT, WATCH_UART_PORT};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::channel::Channel;

/// Link ids of this test's node (any ids: the relay takes them from its
/// list).
const BUS: u8 = 0x11;
const STREAM: u8 = 0x12;
const STREAM_ADDR: u8 = 0xFF;
/// A link the node has but the board does not wire.
const SPARE: u8 = 0x13;

/// This test's own relay link table, one port each.
const LINKS: [RelayLinkDef; 3] = [
    RelayLinkDef {
        id: BUS,
        port: 0,
        kind: LinkKind::I2c,
        addressing: Addressing::SevenBit,
        max_payload: max_payload::I2C as u16,
    },
    RelayLinkDef {
        id: STREAM,
        port: 1,
        kind: LinkKind::Uart,
        addressing: Addressing::Fixed(STREAM_ADDR),
        max_payload: max_payload::UART as u16,
    },
    RelayLinkDef {
        id: SPARE,
        port: 2,
        kind: LinkKind::Uart,
        addressing: Addressing::Fixed(STREAM_ADDR),
        max_payload: max_payload::UART as u16,
    },
];

/// This node is node 3; more nodes are linked to it than a home screen
/// has rows for (the last three answer nobody).
const ME: u8 = 3;
static FALLBACK: [dagon_relay::nodes::LinkedNode; 5] = [
    named(1, "seg", BUS, NODE_I2C_ADDR),
    named(2, "watch", STREAM, STREAM_ADDR),
    named(4, "fourth", BUS, 0x51),
    named(5, "fifth", BUS, 0x52),
    named(6, "sixth", BUS, 0x53),
];

// ---- virtual time and the executor ----

#[derive(Default)]
struct Timeline {
    now: Cell<u64>,
    timers: RefCell<Vec<(u64, Waker)>>,
}

#[derive(Clone, Default)]
struct VirtualClock(Rc<Timeline>);

impl RelayClock for VirtualClock {
    fn now_ms(&self) -> u64 {
        self.0.now.get()
    }

    async fn wait_until(&self, at_ms: u64) {
        poll_fn(|cx| {
            if self.0.now.get() >= at_ms {
                return Poll::Ready(());
            }
            self.0.timers.borrow_mut().push((at_ms, cx.waker().clone()));
            Poll::Pending
        })
        .await
    }
}

impl VirtualClock {
    /// Moves to the earliest deadline anyone waits for and wakes its
    /// waiters; false if nobody waits for time.
    fn advance(&self) -> bool {
        let mut timers = self.0.timers.borrow_mut();
        let Some(next) = timers.iter().map(|(t, _)| *t).min() else {
            return false;
        };
        self.0.now.set(self.0.now.get().max(next));
        let now = self.0.now.get();
        let (due, rest): (Vec<_>, Vec<_>) = timers.drain(..).partition(|(t, _)| *t <= now);
        *timers = rest;
        drop(timers);
        due.into_iter().for_each(|(_, w)| w.wake());
        true
    }
}

struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Polls `workers` and `scenario` whenever their waker fired, and moves
/// time on (keeping the simulated bus in step) only while every one of
/// them waits; returns when `scenario` completes.
fn run(
    clock: &VirtualClock,
    bus: &RefCell<SimBus>,
    mut workers: Vec<Pin<&mut dyn Future<Output = ()>>>,
    mut scenario: Pin<&mut dyn Future<Output = ()>>,
) {
    let flags: Vec<Arc<Flag>> = (0..=workers.len())
        .map(|_| Arc::new(Flag(AtomicBool::new(true))))
        .collect();
    let wakers: Vec<Waker> = flags.iter().map(|f| Waker::from(f.clone())).collect();
    loop {
        let mut polled = false;
        for (i, flag) in flags.iter().enumerate() {
            if !flag.0.swap(false, Ordering::SeqCst) {
                continue;
            }
            polled = true;
            let mut cx = Context::from_waker(&wakers[i]);
            if i == workers.len() {
                if scenario.as_mut().poll(&mut cx).is_ready() {
                    return;
                }
            } else if workers[i].as_mut().poll(&mut cx).is_ready() {
                panic!("a relay task returned");
            }
        }
        if polled {
            continue;
        }
        assert!(clock.advance(), "every task waits and none for time");
        let mut b = bus.borrow_mut();
        let now = clock.now_ms();
        if b.now_ms < now {
            let d = now - b.now_ms;
            b.advance_ms(d);
        }
    }
}

// ---- the node's parts ----

/// A link to `dagon_sim`'s bus; the time an exchange takes on the bus
/// passes on the virtual clock too.
struct SimNodeLink<'a> {
    bus: &'a RefCell<SimBus>,
    clock: VirtualClock,
    id: u8,
    addressed: bool,
    /// The simulated port the node is on.
    port: u8,
    /// The address of every exchange, in order.
    asked: Rc<RefCell<Vec<u8>>>,
}

impl SimNodeLink<'_> {
    async fn spend(&self, since_bus_ms: u64) {
        let spent = self.bus.borrow().now_ms - since_bus_ms;
        self.clock.wait_until(self.clock.now_ms() + spent).await;
    }
}

impl NodeLink for SimNodeLink<'_> {
    fn link_id(&self) -> u8 {
        self.id
    }

    fn max_payload(&self) -> usize {
        if self.addressed {
            max_payload::I2C
        } else {
            max_payload::UART
        }
    }

    fn hello_timeout_ms(&self) -> u16 {
        if self.addressed {
            50
        } else {
            200
        }
    }

    async fn exchange(
        &mut self,
        addr: u8,
        frame: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        self.asked.borrow_mut().push(addr);
        let t0 = self.bus.borrow().now_ms;
        let resp = {
            let mut b = self.bus.borrow_mut();
            if self.addressed {
                b.i2c_exchange(self.port, addr, frame, timeout_ms.into())
            } else {
                b.uart_exchange(self.port, frame, timeout_ms.into())
            }
        };
        self.spend(t0).await;
        let resp = resp.ok_or(Status::Timeout)?;
        let out = out.get_mut(..resp.len()).ok_or(Status::Frame)?;
        out.copy_from_slice(&resp);
        Ok(resp.len())
    }

    async fn raw(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), Status> {
        if !self.addressed {
            return Err(Status::Unsupported);
        }
        let mut b = self.bus.borrow_mut();
        if !write.is_empty() {
            b.i2c_write(self.port, addr, write)
                .map_err(|_| Status::Timeout)?;
        }
        if !read.is_empty() {
            b.i2c_read(self.port, addr, read)
                .map_err(|_| Status::Timeout)?;
        }
        Ok(())
    }
}

/// The reset and boot-mode lines through `SimBus::set_lines`.
struct SimLines<'a>(&'a RefCell<SimBus>);

impl SubControl for SimLines<'_> {
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        self.0.borrow_mut().set_lines(nrst, boot0_high)
    }
}

#[derive(Clone, Copy)]
struct NoWatchdog;

impl Liveness for NoWatchdog {
    fn busy(&self, _: usize) {}
    fn busy_for(&self, _: usize, _: u64) {}
    fn idle(&self, _: usize) {}
    fn progress(&self, _: usize) {}
    fn reboot(&self) -> ! {
        panic!("the relay reset the node")
    }
}

/// A flash with no topology stored: the relay polls its fallback list.
struct Erased;

impl Flash for Erased {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, _: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.fill(0xFF);
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

type Chain = Composed<&'static Published<AppSnapshot, 8>, Nop, Nop, fn() -> u64>;

/// This node's agent: the agent crate's, over the simulator's store.
struct NodeAgent(dagon_agent::Agent<SimStore, Chain, Ed25519Compact>);

impl OwnAgent for NodeAgent {
    fn link_up(&mut self, link: u8, addr: u8) {
        self.0.link_up(link, addr)
    }
    fn link_down(&mut self, link: u8, addr: u8) {
        self.0.link_down(link, addr)
    }
    fn request(&mut self, frame: &[u8], via: u8) {
        self.0.request(frame, via)
    }
    fn replied(&mut self, frame: &[u8], ms: u32) {
        if let Some(status) = dagon_agent::reply_status(frame) {
            self.0.replied(status, ms)
        }
    }
    fn handle(&mut self, request: &[u8]) -> Option<(&[u8], Option<u32>)> {
        let r = self.0.handle(request)?;
        Some((r.frame, r.reboot.then_some(r.reboot_delay_ms)))
    }
    fn status(&self) -> &StatusResp {
        self.0.images().status()
    }
    fn before_reboot(&mut self) {
        self.0.before_reboot()
    }
}

/// This node's agent, and what its app reads from it.
fn agent() -> (NodeAgent, Reader<AppSnapshot, 8>) {
    let events: &'static mut Published<AppSnapshot, 8> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    let chain = compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 4000)),
    );
    let cfg = dagon_sim::hub_config();
    let verifier = Ed25519Compact::new(dagon_sim::sample::PUBKEY);
    let app = chain.reader();
    (
        NodeAgent(dagon_agent::Agent::new(
            cfg,
            verifier,
            SimStore::default(),
            chain,
        )),
        app,
    )
}

/// The PC end of the relay's uplink: the scenario sends requests and reads
/// the replies.
type Frames = Channel<Cs, Vec<u8>, 2>;

struct ScriptedPc<'a> {
    requests: &'a Frames,
    replies: &'a Frames,
    frame: Vec<u8>,
}

impl Link for ScriptedPc<'_> {
    fn max_payload(&self) -> usize {
        max_payload::USB
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Reenumerates
    }
    fn kind(&self) -> LinkKind {
        LinkKind::UsbCdc
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        self.frame = self.requests.receive().await;
        Ok(&self.frame)
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.replies.send(frame.to_vec()).await;
        Ok(())
    }
}

// ---- the scenario's requests ----

struct Pc<'a> {
    requests: &'a Frames,
    replies: &'a Frames,
    seq: Cell<u8>,
}

impl Pc<'_> {
    async fn ask(&self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        let seq = self.seq.get().wrapping_add(1);
        self.seq.set(seq);
        let mut buf = vec![0u8; frame_len(payload.len())];
        buf[8..8 + payload.len()].copy_from_slice(payload);
        finish(&mut buf, 0, seq, cmd as u8, payload.len()).unwrap();
        self.requests.send(buf).await;
        let reply = self.replies.receive().await;
        let (f, _) = Frame::decode(&reply, max_payload::USB).unwrap();
        assert_eq!((f.seq, f.cmd), (seq, cmd as u8));
        let (status, body) = split_status(f.payload).unwrap();
        (status, body.to_vec())
    }

    async fn relay(&self, link: u8, addr: u8, timeout_ms: u16, inner: &[u8]) -> (Status, Vec<u8>) {
        let mut p = vec![0u8; RelayReq::HEAD_LEN + inner.len()];
        let r = RelayReq {
            link,
            addr,
            timeout_ms,
            inner,
        };
        let n = r.encode(&mut p).unwrap();
        self.ask(Cmd::Relay, &p[..n]).await
    }

    async fn hello(&self, link: u8, addr: u8) -> (Status, Vec<u8>) {
        let max = LINKS
            .iter()
            .find(|l| l.id == link)
            .map_or(64, |l| usize::from(l.max_payload));
        let mut req = [0u8; 32];
        let n = hello_request(max, 0x40, &mut req).unwrap();
        self.relay(link, addr, 300, &req[..n]).await
    }

    async fn lines(&self, boot0_high: bool, line: u8) -> Status {
        let s = SubCtl {
            nrst: line,
            boot0: if boot0_high { boot0::HIGH } else { boot0::LOW },
        };
        let mut p = [0u8; 8];
        let n = s.encode(&mut p).unwrap();
        self.ask(Cmd::SubCtl, &p[..n]).await.0
    }

    /// ROM loader GET: its first reply byte.
    async fn rom_get(&self) -> (Status, Vec<u8>) {
        let raw = I2cRaw {
            read_len: 1,
            data: &[0x00, 0xFF],
        };
        let mut p = [0u8; 16];
        let n = raw.encode(&mut p).unwrap();
        self.ask(Cmd::I2cRaw, &p[..n]).await
    }

    async fn show(&self) -> Status {
        let s = Show {
            node: 1,
            phase: dagon_proto::payload::phase::TRANSFER,
            percent: 10,
            version: Version::new(1, 1, 0),
            text: b"seg",
        };
        let mut p = [0u8; 64];
        let n = s.encode(&mut p).unwrap();
        self.ask(Cmd::Show, &p[..n]).await.0
    }
}

/// The sample's simulated ports, and the ports its F401 and its watch are on.
fn sample_bus() -> (RefCell<SimBus>, [u8; 2]) {
    let sample = sample_hub(7);
    let ports = [
        sample.port(NODE_I2C_PORT, LinkKind::I2c),
        sample.port(WATCH_UART_PORT, LinkKind::Uart),
    ];
    (RefCell::new(sample.bus), ports)
}

/// The relayed answer frame is a successful HELLO answer.
fn hello_answered(status: Status, body: &[u8], max: usize) -> bool {
    status == Status::Ok && dagon_port::hello_answer(body, max).is_some()
}

#[test]
fn the_relay_serves_the_pc_over_its_links() {
    let (bus, sim_ports) = sample_bus();
    let clock = VirtualClock::default();
    let state = State::new();
    let config = Config {
        fallback: &FALLBACK,
        ..Config::default()
    };
    let ports = [Port::new(), Port::new()];
    let links = [
        LinkRoute {
            link: LINKS[0],
            port: Some(&ports[0]),
        },
        LinkRoute {
            link: LINKS[1],
            port: Some(&ports[1]),
        },
        LinkRoute {
            link: LINKS[2],
            port: None,
        },
    ];
    let (requests, replies) = (Frames::new(), Frames::new());
    let bus_asked = Rc::new(RefCell::new(Vec::new()));
    let link = |i: usize, asked: Rc<RefCell<Vec<u8>>>| SimNodeLink {
        bus: &bus,
        clock: clock.clone(),
        id: LINKS[i].id,
        addressed: i == 0,
        port: sim_ports[i],
        asked,
    };
    let (own, app) = agent();
    let relay = Relay {
        agent: own,
        flash: Erased,
        topology: 0,
        identity: Identity {
            index: ME,
            tag: "relay-rp2040",
        },
        links: &links,
        recovery: Some(Recovery {
            lines: SimLines(&bus),
            link: BUS,
        }),
        state: &state,
        watch: NoWatchdog,
        clock: clock.clone(),
        config,
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
    };
    let pc = Pc {
        requests: &requests,
        replies: &replies,
        seq: Cell::new(0),
    };

    let scenario = async {
        // RELAY to a node on each link.
        let (s, body) = pc.hello(BUS, NODE_I2C_ADDR).await;
        assert!(hello_answered(s, &body, max_payload::I2C), "{s:?}");
        let (s, body) = pc.hello(STREAM, STREAM_ADDR).await;
        assert!(hello_answered(s, &body, max_payload::UART), "{s:?}");
        let home = state.home();
        assert_eq!(home.own.node, named(ME, "relay", 0, 0));
        assert_eq!(home.linked.len(), 3, "the screen's rows");
        assert!(home.linked[..2].iter().all(|n| n.shade != Shade::Unknown));
        assert_eq!(
            state.linked().as_slice(),
            FALLBACK,
            "the relay keeps them all"
        );

        // A link the node has but the board does not wire: E_UNSUPPORTED.
        assert_eq!(pc.hello(SPARE, STREAM_ADDR).await.0, Status::Unsupported);

        // An unknown link or an address the link cannot have: E_ARG.
        assert_eq!(pc.hello(0x33, NODE_I2C_ADDR).await.0, Status::Arg);
        assert_eq!(pc.hello(BUS, 0x80).await.0, Status::Arg);
        assert_eq!(pc.hello(STREAM, 0x01).await.0, Status::Arg);
        let inner = vec![0u8; frame_len(max_payload::I2C) + 1];
        assert_eq!(
            pc.relay(BUS, NODE_I2C_ADDR, 300, &inner).await.0,
            Status::Arg
        );

        // A valid address nobody answers: E_TIMEOUT once the request's own
        // timeout has passed (the port's margin is not needed).
        let t0 = clock.now_ms();
        assert_eq!(pc.hello(BUS, 0x50).await.0, Status::Timeout);
        let took = clock.now_ms() - t0;
        assert!((300..300 + 1_000).contains(&took), "{took} ms");

        // I2C_RAW only while BOOT0 is high, as last set successfully.
        assert_eq!(pc.rom_get().await.0, Status::State);
        let wired = bus.borrow().sub;
        bus.borrow_mut().sub = None;
        assert_eq!(pc.lines(true, nrst::PULSE).await, Status::Timeout);
        assert_eq!(pc.rom_get().await.0, Status::State);
        bus.borrow_mut().sub = wired;
        assert_eq!(pc.lines(true, nrst::PULSE).await, Status::Ok);
        assert!(bus.borrow_mut().sub_mut().unwrap().boot0());
        clock.wait_until(clock.now_ms() + 100).await;
        assert_eq!(pc.rom_get().await, (Status::Ok, vec![dagon_sim::rom::ACK]));
        bus.borrow_mut().sub = None;
        assert_eq!(pc.lines(false, nrst::PULSE).await, Status::Timeout);
        bus.borrow_mut().sub = wired;
        assert_ne!(pc.rom_get().await.0, Status::State);
        assert_eq!(pc.lines(false, nrst::PULSE).await, Status::Ok);
        assert_eq!(pc.rom_get().await.0, Status::State);

        // The PC quiet window: after a RELAY or SHOW the links do not poll
        // and the progress stays for 10 s; then they poll again.
        assert_eq!(pc.show().await, Status::Ok);
        let shown = clock.now_ms();
        let before = bus_asked.borrow().len();
        assert!(state.home().progress.is_some());
        // The SHOW reaches this node's own agent too: its app reads the
        // progress the host told it.
        let told = app.latest().map(|l| l.state.percent);
        assert_eq!(told, Some(Some(10)), "the app's progress");
        clock.wait_until(shown + 9_500).await;
        assert_eq!(
            bus_asked.borrow().len(),
            before,
            "no HELLO poll while the PC is busy"
        );
        assert!(!state.expire_progress(clock.now_ms()));
        clock.wait_until(shown + 12_500).await;
        let polled = bus_asked.borrow()[before..].to_vec();
        for addr in [NODE_I2C_ADDR, 0x51, 0x52, 0x53] {
            assert!(polled.contains(&addr), "{addr:#x} polled: {polled:x?}");
        }
        assert!(state.expire_progress(clock.now_ms()));
        assert!(state.home().progress.is_none());
    };

    let serving = async move {
        serve(relay, up).await;
    };
    let on_bus = async {
        let mut l = link(0, bus_asked.clone());
        run_link(
            &mut l,
            &ports[0],
            clock.clone(),
            NoWatchdog,
            task::link(0),
            &state,
            &config,
        )
        .await;
    };
    let on_stream = async {
        let mut l = link(1, Rc::new(RefCell::new(Vec::new())));
        run_link(
            &mut l,
            &ports[1],
            clock.clone(),
            NoWatchdog,
            task::link(1),
            &state,
            &config,
        )
        .await;
    };
    let (mut serving, mut on_bus, mut on_stream, mut scenario) = (
        std::pin::pin!(serving),
        std::pin::pin!(on_bus),
        std::pin::pin!(on_stream),
        std::pin::pin!(scenario),
    );
    let workers: Vec<Pin<&mut dyn Future<Output = ()>>> =
        vec![serving.as_mut(), on_bus.as_mut(), on_stream.as_mut()];
    run(&clock, &bus, workers, scenario.as_mut());
}

#[test]
fn the_defaults_are_the_relays_values() {
    let c = Config::default();
    assert_eq!(
        (
            c.rom_addr,
            c.raw_timeout_ms,
            c.hello_poll_ms,
            c.quiet_ms,
            c.port_margin_ms
        ),
        (0x39, 1_000, 2_000, 10_000, 1_000)
    );
    assert!(c.fallback.is_empty());
    assert_eq!((task::PC, task::link(0), task::count(2)), (0, 1, 3));
}

/// A flash holding a topology display copy at address 0.
struct Stored(Vec<u8>);

impl Flash for Stored {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let at = addr as usize;
        let src = self.0.get(at..at + buf.len()).ok_or(FlashError::Range)?;
        buf.copy_from_slice(src);
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

/// Nodes 1 and 2 run images with the same tag; 2 drives the bus link to
/// node 0 (address 0x40), 1 the bus link to node 3 (0x43).
fn same_tag_topology() -> Stored {
    use dagon_format::header::target_tag;
    use dagon_format::topology::{encode, iface, TopoEnd, TopoLink, TopoNode, MAX_SIZE, NONE};
    let tags = [
        "seg-f401-7seg",
        "relay-rp2040",
        "relay-rp2040",
        "watch-esp32",
    ];
    let nodes: Vec<TopoNode> = tags
        .iter()
        .enumerate()
        .map(|(i, tag)| TopoNode {
            index: i as u8,
            target_tag: target_tag(tag),
            ..TopoNode::default()
        })
        .collect();
    // `from` drives the bus; `to` is reached at 0x40 + to.
    let link = |from: u8, to: u8| {
        let end = |node, addr| TopoEnd {
            node,
            port: 0,
            addr,
        };
        let mut ends = [end(from, NONE), end(to, 0x40 + to)];
        ends.sort_by_key(|e| e.node);
        TopoLink {
            ends,
            iface: iface::I2C_BUS,
            ..TopoLink::default()
        }
    };
    let mut flash = vec![0xFF; MAX_SIZE];
    encode(&nodes, &[link(2, 0), link(1, 3)], &[], &mut flash).unwrap();
    Stored(flash)
}

/// The relay of node `index` (tag `relay-rp2040`) over
/// [`same_tag_topology`]: the nodes it lists and the addresses its bus
/// worker polls.
fn serves(index: u8) -> (Vec<dagon_relay::nodes::LinkedNode>, Vec<u8>) {
    let (bus, sim_ports) = sample_bus();
    let clock = VirtualClock::default();
    let state = State::new();
    let config = Config::default();
    let port = Port::new();
    let links = [LinkRoute {
        // The topology's links are on port 0 at both ends.
        link: LINKS[0],
        port: Some(&port),
    }];
    let (requests, replies) = (Frames::new(), Frames::new());
    let asked = Rc::new(RefCell::new(Vec::new()));
    let relay = Relay {
        agent: agent().0,
        flash: same_tag_topology(),
        topology: 0,
        identity: Identity {
            index,
            tag: "relay-rp2040",
        },
        links: &links,
        recovery: None::<Recovery<SimLines>>,
        state: &state,
        watch: NoWatchdog,
        clock: clock.clone(),
        config,
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
    };
    let serving = async move {
        serve(relay, up).await;
    };
    let on_bus = async {
        let mut l = SimNodeLink {
            bus: &bus,
            clock: clock.clone(),
            id: BUS,
            addressed: true,
            port: sim_ports[0],
            asked: asked.clone(),
        };
        run_link(
            &mut l,
            &port,
            clock.clone(),
            NoWatchdog,
            task::link(0),
            &state,
            &config,
        )
        .await;
    };
    // Past the PC quiet window, then several HELLO polls.
    let scenario =
        clock.wait_until(u64::from(config.quiet_ms) + 3 * u64::from(config.hello_poll_ms));
    let (mut serving, mut on_bus, mut scenario) = (
        std::pin::pin!(serving),
        std::pin::pin!(on_bus),
        std::pin::pin!(scenario),
    );
    let workers: Vec<Pin<&mut dyn Future<Output = ()>>> = vec![serving.as_mut(), on_bus.as_mut()];
    run(&clock, &bus, workers, scenario.as_mut());
    let polled = asked.borrow().clone();
    (state.linked().to_vec(), polled)
}

#[test]
fn relays_of_nodes_with_the_same_tag_each_serve_only_their_own_links() {
    let (linked, polled) = serves(1);
    assert_eq!(linked, [named(3, "watch", BUS, 0x43)]);
    assert!(
        !polled.is_empty() && polled.iter().all(|&a| a == 0x43),
        "{polled:x?}"
    );

    let (linked, polled) = serves(2);
    assert_eq!(linked, [named(0, "seg", BUS, 0x40)]);
    assert!(
        !polled.is_empty() && polled.iter().all(|&a| a == 0x40),
        "{polled:x?}"
    );
}
