//! What `state_machine!` generates, seen from another crate; the composed
//! chain's order, bounds and published state.

use core::cell::Cell;
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use dagon_events::{
    compose, compose_unpublished, decisions, extensions, state_machine, Accept, Bounds, Compact,
    Ctx, DecisionId, Delay, Event, EventRecord, Extension, Health, Layer, Nop, Observe, Party,
    Published, RebootRequest, Refused, Snapshot, TrialInfo, UpdateOffer, Watchdog,
};
use dagon_format::version::Version;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cause {
    #[default]
    Idle,
    Store,
}

impl Compact for Cause {
    const SIZE: usize = 1;
    fn put(&self, out: &mut [u8]) {
        out[0] = *self as u8;
    }
}

state_machine! {
    /// A transfer into one slot.
    pub machine transfer in App {
        initial Idle;
        states {
            Idle,
            Header { slot: u8, len: u32 },
            Body { slot: u8, len: u32, next: u32 },
        }
        transitions {
            /// An update passed every guard.
            offered(slot: u8, len: u32): Idle => Header { slot, len }
                emits UpdateOffered = 0x10 { slot: u8, body_len: u32 = len }
                observe(live, host);
            header_ok(): Header { slot, len } => Body { slot, len, next: 0 }
                emits HeaderVerified = 0x11 { slot: u8 }
                observe(live, host) action;
            written(end: u32): Body { slot, len } => Body { slot, len, next: end }
                emits TransferProgress = 0x12 { done: u32 = end, total: u32 = len, last: Option<u8> = None }
                observe(live, replay, host);
            aborted(cause: Cause): Header | Body => Idle
                emits TransferAborted = 0x13 { cause: Cause }
                observe(host) retired;
            reset(): * => Idle
                emits Reset = 0x14 {}
                observe(live);
        }
    }
}

/// Records every event it sees: (kind, owned form).
#[derive(Default)]
struct Log(Vec<(&'static str, EventRecord)>);

impl Extension for Log {
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.0.push((e.kind(), e.record(0)));
    }
}

fn bounds() -> Bounds<impl FnMut() -> u64> {
    Bounds::new(|| 0u64, Watchdog::new(8_000, 30_000))
}

#[test]
fn the_catalog_is_the_declaration() {
    let c = transfer::CATALOG;
    assert_eq!(c.machine, "transfer");
    assert_eq!(c.layer, Layer::App);
    assert_eq!(c.initial, "Idle");
    assert_eq!(c.states, ["Idle", "Header", "Body"]);
    assert_eq!(c.doc.trim(), "A transfer into one slot.");
    let kinds: Vec<_> = c.entries.iter().map(|e| (e.kind, e.code)).collect();
    assert_eq!(
        kinds,
        [
            ("update_offered", 0x10),
            ("header_verified", 0x11),
            ("transfer_progress", 0x12),
            ("transfer_aborted", 0x13),
            ("reset", 0x14),
        ]
    );
    let p = c.entry(0x12).unwrap();
    assert_eq!(
        (p.trigger, p.from, p.to),
        ("written", &["Body"][..], "Body")
    );
    let fields: Vec<_> = p.payload.iter().map(|f| (f.name, f.bytes)).collect();
    assert_eq!(fields, [("done", 4), ("total", 4), ("last", 2)]);
    assert_eq!(
        p.observe,
        Observe {
            live: true,
            replay: true,
            host: true
        }
    );
    let a = c.entry(0x13).unwrap();
    assert_eq!(a.from, ["Header", "Body"]);
    assert!(a.flags.retired && !a.flags.action && !a.live());
    assert!(c.entry(0x11).unwrap().flags.action);
    assert_eq!(c.entry(0x14).unwrap().from, ["*"]);
    assert_eq!(
        c.entry(0x10).unwrap().doc.trim(),
        "An update passed every guard."
    );
}

#[test]
fn a_transition_delivers_its_event_then_moves() {
    let mut log = Log::default();
    let mut out = compose_unpublished(Layer::App, Nop, &mut log, bounds());
    let mut m = transfer::Machine::new();
    m.offered(&mut out, 1, 300).unwrap();
    assert_eq!(m.state(), transfer::State::Header { slot: 1, len: 300 });
    m.header_ok(&mut out).unwrap();
    m.written(&mut out, 0x0102_0304).unwrap();
    assert_eq!(
        m.state(),
        transfer::State::Body {
            slot: 1,
            len: 300,
            next: 0x0102_0304
        }
    );
    let r = m.offered(&mut out, 2, 1).unwrap_err();
    assert_eq!(
        r,
        Refused {
            machine: "transfer",
            trigger: "offered",
            state: "Body"
        }
    );
    assert_eq!(
        r.to_string(),
        "transfer: offered is not a transition from Body"
    );
    m.aborted(&mut out, Cause::Store).unwrap();
    assert!(m.aborted(&mut out, Cause::Idle).is_err());
    m.reset(&mut out).unwrap();
    drop(out);

    let kinds: Vec<_> = log.0.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        [
            "update_offered",
            "header_verified",
            "transfer_progress",
            "transfer_aborted",
            "reset"
        ]
    );
    let (_, progress) = log.0[2];
    assert_eq!(
        progress,
        EventRecord {
            layer: Layer::App.code(),
            kind: 0x12,
            flags: 0,
            dropped: 0,
            seq: 0,
            payload: [4, 3, 2, 1, 44, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    );
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Progress(Option<(u32, u32)>);

impl Snapshot for Progress {
    fn apply(&mut self, e: &Event<'_>) {
        if let Some(p) = e.get::<transfer::TransferProgress>() {
            self.0 = Some((p.done, p.total));
        }
    }
}

fn poll_once<F: Future>(f: F) -> Poll<F::Output> {
    pin!(f)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn the_publisher_sees_every_event_first_and_then_board_then_user() {
    static EVENTS: Published<Progress, 2> = Published::new();
    let order = core::cell::RefCell::new(Vec::new());
    struct Mark<'a>(
        &'static str,
        &'a core::cell::RefCell<Vec<(&'static str, Option<u32>)>>,
    );
    impl Extension for Mark<'_> {
        fn on_event(&mut self, _: &Event<'_>, _: &Ctx<'_>) {
            // The publisher has already counted this event.
            let seq = EVENTS.latest().map(|l| l.last.seq);
            self.1.borrow_mut().push((self.0, seq));
        }
    }
    let mut rx = EVENTS.receiver().unwrap();
    let mut out = compose(
        Layer::App,
        &EVENTS,
        Mark("board", &order),
        extensions![Mark("library", &order), Mark("app", &order)],
        bounds(),
    );
    let mut m = transfer::Machine::new();
    m.offered(&mut out, 0, 100).unwrap();
    m.header_ok(&mut out).unwrap();
    for end in [25, 50, 75, 100] {
        m.written(&mut out, end).unwrap();
    }
    drop(out);

    let order = order.into_inner();
    assert_eq!(
        &order[..3],
        [("board", Some(1)), ("library", Some(1)), ("app", Some(1))]
    );
    assert_eq!(order.len(), 18);
    let Poll::Ready(l) = poll_once(rx.changed()) else {
        panic!("a change was published")
    };
    assert_eq!(l.state, Progress(Some((100, 100))));
    assert_eq!((l.last.seq, l.last.kind), (6, 0x12));

    // A ring of 2 keeps the newest two; they account for the four dropped.
    let a = EVENTS.recent().try_take().unwrap();
    let b = EVENTS.recent().try_take().unwrap();
    assert_eq!((a.seq, b.seq), (5, 6));
    assert_eq!(u32::from(a.dropped) + u32::from(b.dropped), 4);
    assert_eq!(EVENTS.recent().try_take(), None);
    assert!(poll_once(EVENTS.recent().take()).is_pending());
}

#[test]
fn drops_beyond_255_are_carried_to_later_records() {
    static EVENTS: Published<(), 1> = Published::new();
    let mut out = compose_unpublished(Layer::App, Nop, Nop, bounds());
    let mut chain = compose(Layer::App, &EVENTS, Nop, Nop, bounds());
    let mut m = transfer::Machine::new();
    let mut emit = |m: &mut transfer::Machine| {
        m.reset(&mut chain).unwrap();
    };
    for _ in 0..600 {
        emit(&mut m);
    }
    // 600 pushed into a ring of one: 599 lost.
    let mut reported = 0u32;
    let first = EVENTS.recent().try_take().unwrap();
    reported += u32::from(first.dropped);
    assert_eq!(first.dropped, 255);
    for _ in 0..3 {
        emit(&mut m);
        reported += u32::from(EVENTS.recent().try_take().unwrap().dropped);
    }
    assert_eq!(reported, 599);
    m.offered(&mut out, 0, 1).unwrap();
}

/// Answers after `cost_us` on the shared clock.
struct Slow<'a> {
    now: &'a Cell<u64>,
    cost_us: u64,
    accept: Option<Accept>,
    delay: Option<Delay>,
    health: Option<Health>,
}

impl Extension for Slow<'_> {
    fn accept_update(&mut self, _: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
        self.now.set(self.now.get() + self.cost_us);
        self.accept
    }
    fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
        self.now.set(self.now.get() + self.cost_us);
        self.delay
    }
    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        self.now.set(self.now.get() + self.cost_us);
        self.health
    }
}

#[test]
fn decisions_default_when_every_party_abstains() {
    let mut out = compose_unpublished(Layer::App, Nop, extensions![], bounds());
    assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::Yes);
    assert_eq!(out.allow_reboot_now(&RebootRequest::new(0)), Delay::Now);
    let t = TrialInfo::new(1, Version::new(1, 0, 0), 1, 3);
    assert_eq!(out.self_check(&t), Health::Healthy);
}

#[test]
fn the_board_answers_first_and_a_delay_is_capped_by_the_watchdog() {
    let now = Cell::new(0u64);
    let board = Slow {
        now: &now,
        cost_us: 10,
        accept: Some(Accept::No),
        delay: Some(Delay::After(60_000)),
        health: None,
    };
    let user = Slow {
        now: &now,
        cost_us: 10,
        accept: Some(Accept::Yes),
        delay: None,
        health: Some(Health::Pending),
    };
    let wd = Watchdog::new(8_000, 30_000);
    assert_eq!(wd.reboot_delay_max_ms(), 4_000);
    let mut out = compose_unpublished(Layer::App, board, user, Bounds::new(|| now.get(), wd));
    assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::No);
    assert_eq!(
        out.allow_reboot_now(&RebootRequest::new(0)),
        Delay::After(4_000)
    );
    let t = TrialInfo::new(1, Version::new(1, 0, 0), 1, 3);
    assert_eq!(out.self_check(&t), Health::Pending);
}

#[test]
fn a_late_answer_is_ignored_and_reported() {
    let now = Cell::new(0u64);
    let mut log = Log::default();
    let board = Slow {
        now: &now,
        cost_us: 50_001,
        accept: Some(Accept::No),
        delay: None,
        health: Some(Health::Unhealthy),
    };
    {
        let mut out = compose_unpublished(
            Layer::App,
            board,
            &mut log,
            Bounds::new(|| now.get(), Watchdog::new(8_000, 8_000)),
        );
        assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::Yes);
        let t = TrialInfo::new(1, Version::new(1, 0, 0), 1, 3);
        assert_eq!(out.self_check(&t), Health::Healthy);
    }
    let kinds: Vec<_> = log
        .0
        .iter()
        .map(|(k, r)| (*k, r.payload[0], r.payload[1]))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "extension_overran",
                DecisionId::AcceptUpdate as u8,
                Party::Board as u8
            ),
            (
                "extension_overran",
                DecisionId::SelfCheck as u8,
                Party::Board as u8
            ),
        ]
    );
    let us = u32::from_le_bytes(log.0[0].1.payload[2..6].try_into().unwrap());
    assert_eq!(us, 50_001);
    assert_eq!(decisions::ExtensionOverran::ENTRY.layer, Layer::App);
}
