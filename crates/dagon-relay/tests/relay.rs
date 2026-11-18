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
    hello_request, BusError, Link, LinkError, LinkKind, NodeLink, Nrst, Reconnect, SubControl,
};
use dagon_port::{Addressing, RelayLinkDef};
use dagon_proto::payload::{
    boot0, encode_caps_req, nrst, CapsResp, CmdSet, HelloResp, I2cRaw, Relay as RelayReq, Show,
    SubCtl,
};
use dagon_proto::{finish, frame_len, max_payload, plant, split_status, Cmd, Frame, Status};
use dagon_relay::{
    run_link, serve, task, Config, LinkRoute, Liveness, OwnAgent, Port, Recovery, Relay, RelayClock,
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
    fn before_reboot(&mut self) {
        self.0.before_reboot()
    }
    fn also_serves(&mut self, front: dagon_proto::payload::CmdSet) {
        self.0.also_serves(front)
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
    /// The uplink's largest payload.
    max: usize,
}

impl Link for ScriptedPc<'_> {
    fn max_payload(&self) -> usize {
        self.max
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
    /// The status a request of any command number is answered with.
    async fn status(&self, cmd: u8, payload: &[u8]) -> Status {
        let seq = self.seq.get().wrapping_add(1);
        self.seq.set(seq);
        let mut buf = vec![0u8; frame_len(payload.len())];
        buf[8..8 + payload.len()].copy_from_slice(payload);
        finish(&mut buf, 0, seq, cmd, payload.len()).unwrap();
        self.requests.send(buf).await;
        let reply = self.replies.receive().await;
        let (f, _) = Frame::decode(&reply, max_payload::USB).unwrap();
        assert_eq!((f.seq, f.cmd), (seq, cmd));
        split_status(f.payload).unwrap().0
    }

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
        // RELAY and I2C_RAW answer with bulk: the node's frame, the bytes read.
        let body = match (status, cmd) {
            (Status::Ok, Cmd::Relay | Cmd::I2cRaw) => {
                dagon_proto::payload::parse_bulk_resp(cmd, body).unwrap()
            }
            _ => body,
        };
        (status, body.to_vec())
    }

    async fn relay(&self, link: u8, addr: u8, timeout_ms: u16, inner: &[u8]) -> (Status, Vec<u8>) {
        let mut p = vec![0u8; RelayReq::OVERHEAD_MAX + inner.len()];
        let r = RelayReq {
            link,
            addr,
            timeout_ms: Some(timeout_ms),
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
        self.raw(1, &[0x00, 0xFF]).await
    }

    async fn raw(&self, read_len: u16, data: &[u8]) -> (Status, Vec<u8>) {
        let raw = I2cRaw { read_len, data };
        let mut p = vec![0u8; I2cRaw::OVERHEAD_MAX + data.len()];
        let n = raw.encode(&mut p).unwrap();
        self.ask(Cmd::I2cRaw, &p[..n]).await
    }

    /// RELAY of a request frame carrying the link's whole `max` payload (a
    /// command the node does not know): the node's reply comes back whole.
    async fn full_frame(&self, link: u8, addr: u8, max: usize) -> (Status, Vec<u8>) {
        let mut inner = vec![0u8; frame_len(max)];
        finish(&mut inner, 0, 0x5A, 0x7F, max).unwrap();
        self.relay(link, addr, 300, &inner).await
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
    // Each port one frame of its link's max_payload.
    let bus_port: Port<[u8; frame_len(max_payload::I2C)]> = Port::new(&LINKS[0]);
    let stream_port: Port<[u8; frame_len(max_payload::UART)]> = Port::new(&LINKS[1]);
    let links = [
        LinkRoute {
            link: LINKS[0],
            port: Some(&bus_port),
        },
        LinkRoute {
            link: LINKS[1],
            port: Some(&stream_port),
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
        links: &links,
        recovery: Some(Recovery {
            lines: SimLines(&bus),
            link: BUS,
            // The wired node's MCU definition gives its loader's address.
            rom_addr: bus.borrow_mut().sub_mut().and_then(|n| n.rom_addr).unwrap(),
        }),
        watch: NoWatchdog,
        clock: clock.clone(),
        config: Config::default(),
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
        max: max_payload::USB,
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

        // A frame of each link's whole payload goes through, and so does
        // its node's reply.
        for (link, addr, max) in [
            (BUS, NODE_I2C_ADDR, max_payload::I2C),
            (STREAM, STREAM_ADDR, max_payload::UART),
        ] {
            let (s, body) = pc.full_frame(link, addr, max).await;
            assert_eq!(s, Status::Ok, "link {link}");
            let (f, n) = Frame::decode(&body, max).expect("the node's reply");
            assert_eq!((f.seq, f.cmd, n), (0x5A, 0x7F, body.len()), "link {link}");
        }

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
        // The recovery link's buffer bounds both directions: E_ARG above
        // it, never a shortened transfer.
        let limit = frame_len(max_payload::I2C);
        assert_eq!(pc.raw(limit as u16 + 1, &[]).await.0, Status::Arg);
        assert_eq!(pc.raw(0, &vec![0; limit + 1]).await.0, Status::Arg);
        let (s, read) = pc.raw(limit as u16, &[]).await;
        assert!(
            s != Status::Ok || read.len() == limit,
            "{s:?} {}",
            read.len()
        );
        assert_ne!(s, Status::Arg);
        bus.borrow_mut().sub = None;
        assert_eq!(pc.lines(false, nrst::PULSE).await, Status::Timeout);
        bus.borrow_mut().sub = wired;
        assert_ne!(pc.rom_get().await.0, Status::State);
        assert_eq!(pc.lines(false, nrst::PULSE).await, Status::Ok);
        assert_eq!(pc.rom_get().await.0, Status::State);

        // SHOW is this node's own agent's: its app reads the progress the
        // host told it.
        assert_eq!(pc.show().await, Status::Ok);
        let told = app.latest().map(|l| l.state.percent);
        assert_eq!(told, Some(Some(10)), "the app's progress");

        // The links carry only what the PC asks: however long the PC is
        // quiet, the relay sends nothing to a node on its own.
        let before = bus_asked.borrow().len();
        clock.wait_until(clock.now_ms() + 60_000).await;
        assert_eq!(bus_asked.borrow().len(), before, "nothing sent unasked");
    };

    let serving = async move {
        serve(relay, up).await;
    };
    let on_bus = async {
        let mut l = link(0, bus_asked.clone());
        run_link(&mut l, &bus_port, NoWatchdog, task::link(0)).await;
    };
    let on_stream = async {
        let mut l = link(1, Rc::new(RefCell::new(Vec::new())));
        run_link(&mut l, &stream_port, NoWatchdog, task::link(1)).await;
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

/// A relay whose package has no recovery loader address (no node on its
/// recovery lines, `RECOVERY_ROM_ADDR` None) is built without a recovery:
/// SUBCTL and I2C_RAW answer E_UNSUPPORTED, and the bus is never touched.
#[test]
fn a_relay_without_a_recovery_answers_i2c_raw_unsupported() {
    let (bus, _) = sample_bus();
    let clock = VirtualClock::default();
    let links = [LinkRoute {
        link: LINKS[0],
        port: None,
    }];
    let (requests, replies) = (Frames::new(), Frames::new());
    let (own, _app) = agent();
    let relay: Relay<'_, _, SimLines<'_>, _, _> = Relay {
        agent: own,
        links: &links,
        recovery: None,
        watch: NoWatchdog,
        clock: clock.clone(),
        config: Config::default(),
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
        max: max_payload::USB,
    };
    let pc = Pc {
        requests: &requests,
        replies: &replies,
        seq: Cell::new(0),
    };
    let scenario = async {
        assert_eq!(pc.lines(true, nrst::PULSE).await, Status::Unsupported);
        assert_eq!(pc.rom_get().await.0, Status::Unsupported);
        assert!(
            !bus.borrow_mut().sub_mut().unwrap().boot0(),
            "lines untouched"
        );
    };
    let serving = async move {
        serve(relay, up).await;
    };
    let (mut serving, mut scenario) = (std::pin::pin!(serving), std::pin::pin!(scenario));
    let workers: Vec<Pin<&mut dyn Future<Output = ()>>> = vec![serving.as_mut()];
    run(&clock, &bus, workers, scenario.as_mut());
}

/// A node link and recovery lines that count what reaches them.
struct Counted<'a>(&'a Cell<usize>);

impl Counted<'_> {
    fn reached(&self) {
        self.0.set(self.0.get() + 1);
    }
}

impl NodeLink for Counted<'_> {
    fn link_id(&self) -> u8 {
        BUS
    }
    fn max_payload(&self) -> usize {
        max_payload::I2C
    }
    fn hello_timeout_ms(&self) -> u16 {
        50
    }
    async fn exchange(&mut self, _: u8, _: &[u8], _: u16, _: &mut [u8]) -> Result<usize, Status> {
        self.reached();
        Err(Status::Timeout)
    }
    async fn raw(&mut self, _: u8, _: &[u8], _: &mut [u8]) -> Result<(), Status> {
        self.reached();
        Err(Status::Timeout)
    }
}

impl SubControl for Counted<'_> {
    async fn set_lines(&mut self, _: Nrst, _: bool) -> Result<(), BusError> {
        self.reached();
        Ok(())
    }
}

/// The node's HELLO names the relay's commands with its agent's, and a
/// request with an unknown must field, a field sent twice or a bad length
/// is refused in every one of them with nothing sent on a link and the
/// recovery lines left alone.
#[test]
fn the_relay_says_what_it_serves_and_reads_before_it_acts() {
    let (bus, _) = sample_bus();
    let clock = VirtualClock::default();
    let reached = Cell::new(0);
    let port: Port<[u8; frame_len(max_payload::I2C)]> = Port::new(&LINKS[0]);
    let links = [LinkRoute {
        link: LINKS[0],
        port: Some(&port),
    }];
    let (requests, replies) = (Frames::new(), Frames::new());
    let (own, _app) = agent();
    let relay = Relay {
        agent: own,
        links: &links,
        recovery: Some(Recovery {
            lines: Counted(&reached),
            link: BUS,
            rom_addr: 0x38,
        }),
        watch: NoWatchdog,
        clock: clock.clone(),
        config: Config::default(),
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
        max: max_payload::USB,
    };
    let pc = Pc {
        requests: &requests,
        replies: &replies,
        seq: Cell::new(0),
    };
    let scenario = async {
        use Cmd::*;
        let want = [
            Hello, Status, Version, Caps, Stay, XferBegin, XferData, XferEnd, XferHdr, SetPending,
            Reboot, Confirm, Show, Relay, SubCtl, I2cRaw,
        ];
        let mut req = [0u8; 32];
        let n = hello_request(max_payload::USB, 1, &mut req).unwrap();
        let (f, _) = Frame::decode(&req[..n], max_payload::USB).unwrap();
        let (st, body) = pc.ask(Hello, f.payload).await;
        assert_eq!(st, dagon_proto::Status::Ok);
        let cmds = HelloResp::parse(&body).unwrap().cmds;
        assert_eq!(
            cmds,
            want.iter().fold(CmdSet::EMPTY, |s, c| s.with(*c as u8))
        );

        let mut probes = Vec::new();
        plant::each_probe(|n, p| probes.push((n, p.to_vec())));
        let mut served = CmdSet::EMPTY;
        for (n, p) in probes {
            if pc.status(n, &p).await == dagon_proto::Status::Field {
                served = served.with(n);
            }
        }
        assert_eq!(served, cmds, "what answers is what HELLO says");

        let mut planted = Vec::new();
        let commands = plant::each(&cmds, |n, plant, p| planted.push((n, plant, p.to_vec())));
        assert_eq!(commands, want.len(), "every command it serves");
        for (n, plant, p) in planted {
            assert_eq!(pc.status(n, &p).await, plant.status(), "{n:#x} {plant:?}");
            assert_eq!(reached.get(), 0, "{n:#x} {plant:?}");
        }

        // CAPS about a command of the relay's own.
        let mut p = [0u8; 4];
        let n = encode_caps_req(Relay as u8, &mut p).unwrap();
        let (st, body) = pc.ask(Caps, &p[..n]).await;
        assert_eq!(st, dagon_proto::Status::Ok);
        assert_eq!(CapsResp::parse(&body), Ok(CapsResp::of(Relay)));

        // What is well formed does reach them.
        assert_eq!(pc.lines(true, nrst::RELEASE).await, dagon_proto::Status::Ok);
        assert_eq!(
            pc.hello(BUS, NODE_I2C_ADDR).await.0,
            dagon_proto::Status::Timeout
        );
        assert_eq!(pc.rom_get().await.0, dagon_proto::Status::Timeout);
        assert_eq!(reached.get(), 3);
    };
    let serving = async move {
        serve(relay, up).await;
    };
    let on_bus = async {
        let mut l = Counted(&reached);
        run_link(&mut l, &port, NoWatchdog, task::link(0)).await;
    };
    let (mut serving, mut on_bus, mut scenario) = (
        std::pin::pin!(serving),
        std::pin::pin!(on_bus),
        std::pin::pin!(scenario),
    );
    let workers: Vec<Pin<&mut dyn Future<Output = ()>>> = vec![serving.as_mut(), on_bus.as_mut()];
    run(&clock, &bus, workers, scenario.as_mut());
}

/// A relay without recovery lines does not say it serves SUBCTL and
/// I2C_RAW.
#[test]
fn a_relay_without_a_recovery_serves_relay_alone() {
    assert_eq!(
        dagon_relay::cmds(false),
        CmdSet::EMPTY.with(Cmd::Relay as u8)
    );
    let all = [Cmd::Relay, Cmd::SubCtl, Cmd::I2cRaw];
    assert_eq!(
        dagon_relay::cmds(true),
        all.iter().fold(CmdSet::EMPTY, |s, c| s.with(*c as u8))
    );
}

/// Over an uplink smaller than the link it relays to, the tunnel is what a
/// RELAY on the uplink carries: an inner frame over it, or one that is not a
/// whole frame of this wire, is refused with E_ARG and nothing reaches the
/// link.
#[test]
fn the_relay_refuses_what_it_cannot_carry_before_forwarding() {
    let (bus, sim_ports) = sample_bus();
    let clock = VirtualClock::default();
    let stream_port: Port<[u8; frame_len(max_payload::UART)]> = Port::new(&LINKS[1]);
    let links = [LinkRoute {
        link: LINKS[1],
        port: Some(&stream_port),
    }];
    let (requests, replies) = (Frames::new(), Frames::new());
    let asked = Rc::new(RefCell::new(Vec::new()));
    let (own, _app) = agent();
    let relay: Relay<'_, _, SimLines<'_>, _, _> = Relay {
        agent: own,
        links: &links,
        recovery: None,
        watch: NoWatchdog,
        clock: clock.clone(),
        config: Config::default(),
    };
    let up = ScriptedPc {
        requests: &requests,
        replies: &replies,
        frame: Vec::new(),
        max: max_payload::I2C,
    };
    let pc = Pc {
        requests: &requests,
        replies: &replies,
        seq: Cell::new(0),
    };
    // I2C uplink, UART link: the I2C payload less the RELAY around it.
    let mtu = max_payload::I2C - RelayReq::OVERHEAD_MAX;
    let frame = |len: usize| {
        let mut f = vec![0u8; len];
        finish(&mut f, 0, 0x5A, 0x7F, len - frame_len(0)).unwrap();
        f
    };
    let scenario = async {
        let over = frame(mtu + 1);
        // The request itself fits the uplink.
        assert!(RelayReq::OVERHEAD_MAX - 3 + over.len() <= max_payload::I2C);
        assert_eq!(
            pc.relay(STREAM, STREAM_ADDR, 300, &over).await.0,
            Status::Arg
        );
        let mut bad_crc = frame(32);
        bad_crc[12] ^= 1;
        assert_eq!(
            pc.relay(STREAM, STREAM_ADDR, 300, &bad_crc).await.0,
            Status::Arg
        );
        let mut other_wire = frame(32);
        other_wire[2] ^= 0x80;
        assert_eq!(
            pc.relay(STREAM, STREAM_ADDR, 300, &other_wire).await.0,
            Status::Arg
        );
        let mut trailing = frame(32);
        trailing.push(0);
        assert_eq!(
            pc.relay(STREAM, STREAM_ADDR, 300, &trailing).await.0,
            Status::Arg
        );
        assert!(asked.borrow().is_empty(), "nothing reached the link");

        // At the tunnel's size the frame goes through.
        let (s, body) = pc.relay(STREAM, STREAM_ADDR, 300, &frame(mtu)).await;
        assert_eq!(s, Status::Ok);
        assert!(Frame::decode(&body, max_payload::UART).is_ok());
        assert_eq!(asked.borrow().len(), 1);
    };
    let serving = async move {
        serve(relay, up).await;
    };
    let on_stream = async {
        let mut l = SimNodeLink {
            bus: &bus,
            clock: clock.clone(),
            id: STREAM,
            addressed: false,
            port: sim_ports[1],
            asked: asked.clone(),
        };
        run_link(&mut l, &stream_port, NoWatchdog, task::link(0)).await;
    };
    let (mut serving, mut on_stream, mut scenario) = (
        std::pin::pin!(serving),
        std::pin::pin!(on_stream),
        std::pin::pin!(scenario),
    );
    let workers: Vec<Pin<&mut dyn Future<Output = ()>>> =
        vec![serving.as_mut(), on_stream.as_mut()];
    run(&clock, &bus, workers, scenario.as_mut());
}

#[test]
fn the_defaults_are_the_relays_values() {
    let c = Config::default();
    assert_eq!((c.raw_timeout_ms, c.port_margin_ms), (1_000, 1_000));
    assert_eq!((task::PC, task::link(0), task::count(2)), (0, 1, 3));
}
