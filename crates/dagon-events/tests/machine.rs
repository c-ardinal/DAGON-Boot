//! What `state_machine!` generates, seen from another crate; the composed
//! chain's order, replay, bounds and published state.
// `drop(out)` ends a chain's borrows of what a test then reads.
#![allow(clippy::drop_non_drop)]

use core::cell::{Cell, RefCell};
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use dagon_events::{
    compose, compose_unpublished, decisions, extensions, state_machine, Accept, Bounds, Compact,
    Ctx, DecisionId, Delay, Event, EventRecord, ExtId, Extension, Health, Layer, Nop, Observe,
    Published, Reader, RebootRequest, Refused, Snapshot, TrialInfo, Unreadable, UpdateOffer,
    Watchdog,
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
    fn get(b: &[u8]) -> Option<Self> {
        match b {
            [0] => Some(Cause::Idle),
            [1] => Some(Cause::Store),
            _ => None,
        }
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
                observe(host, replay) retired;
            reset(): * => Idle
                emits Reset = 0x14 {}
                observe(live);
        }
    }
}

/// Records every event it sees: (kind, owned form, replayed).
#[derive(Default)]
struct Log(Vec<(&'static str, EventRecord, bool)>);

impl Extension for Log {
    fn id(&self) -> &'static str {
        "log"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.0.push((e.kind(), e.record(0), e.is_replayed()));
    }
}

/// What a layer package does: create its published events and keep them.
fn events<S: Snapshot, const N: usize>() -> &'static mut Published<S, N> {
    Box::leak(Box::new(Published::new()))
}

fn bounds() -> Bounds<impl FnMut() -> u64> {
    Bounds::new(|| 0u64, Watchdog::new(8_000, 30_000))
}

#[test]
fn the_crate_lists_its_machines() {
    assert_eq!(MACHINES.krate(), "dagon-events");
    let names: Vec<_> = MACHINES.catalogs().iter().map(|c| c.machine).collect();
    assert_eq!(names, ["transfer"]);
    assert_eq!(dagon_events::MACHINES.catalogs()[0].machine, "decisions");
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
}

#[test]
fn a_transition_delivers_its_event_then_moves() {
    let mut log = Log::default();
    let mut out = compose_unpublished(Layer::App, Nop, &mut log);
    let mut m = transfer::Machine::new();
    m.offered(&mut out, 1, 300).unwrap();
    assert_eq!(m.state(), transfer::State::Header { slot: 1, len: 300 });
    m.header_ok(&mut out).unwrap();
    m.written(&mut out, 0x0102_0304).unwrap();
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

    let kinds: Vec<_> = log.0.iter().map(|(k, _, _)| *k).collect();
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
    assert_eq!(
        log.0[2].1.payload,
        [4, 3, 2, 1, 44, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn a_record_replays_as_its_typed_event() {
    let mut first = Log::default();
    {
        let mut out = compose_unpublished(Layer::Boot, Nop, &mut first);
        let mut m = transfer::Machine::new();
        m.offered(&mut out, 1, 300).unwrap();
        m.header_ok(&mut out).unwrap();
        m.written(&mut out, 77).unwrap();
        m.aborted(&mut out, Cause::Store).unwrap();
    }
    /// (progress, abort cause, replayed) per event.
    type Seen = RefCell<Vec<(Option<(u32, u32)>, Option<Cause>, bool)>>;
    let seen = Seen::default();
    struct Typed<'a>(&'a Seen);
    impl Extension for Typed<'_> {
        fn id(&self) -> &'static str {
            "typed"
        }
        fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
            let progress = e
                .get::<transfer::TransferProgress>()
                .map(|p| (p.done, p.total));
            let cause = e.get::<transfer::TransferAborted>().map(|a| a.cause);
            self.0.borrow_mut().push((progress, cause, e.is_replayed()));
        }
    }
    let mut out = compose_unpublished(Layer::App, Nop, Typed(&seen));
    let results: Vec<_> = first
        .0
        .iter()
        .map(|(_, rec, _)| out.replay(&[decisions::CATALOG, transfer::CATALOG], rec))
        .collect();
    // Only `transfer_progress` is marked `replay` and not retired:
    // live/host-only kinds and the retired one are refused.
    assert_eq!(
        results,
        [Err(Unreadable), Err(Unreadable), Ok(()), Err(Unreadable)]
    );
    assert_eq!(seen.borrow().clone(), [(Some((77, 300)), None, true)]);

    // Unknown kinds, other layers and values a newer writer added are
    // refused, not guessed.
    let progress = first.0[2].1;
    let mut rec = progress;
    rec.payload[8] = 9; // `last`'s presence byte
    assert_eq!(out.replay(&[transfer::CATALOG], &rec), Err(Unreadable));
    let mut rec = progress;
    rec.kind = 0x7F;
    assert_eq!(out.replay(&[transfer::CATALOG], &rec), Err(Unreadable));
    let mut rec = progress;
    rec.layer = Layer::Host.code();
    assert_eq!(out.replay(&[transfer::CATALOG], &rec), Err(Unreadable));
    assert_eq!(out.replay(&[], &progress), Err(Unreadable));
    assert_eq!(seen.borrow().len(), 1);
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

type Marks = RefCell<Vec<(&'static str, Option<u32>)>>;

#[test]
fn the_publisher_sees_every_event_first_and_then_board_then_user() {
    static READER: std::sync::OnceLock<Reader<Progress, 2>> = std::sync::OnceLock::new();
    let order = Marks::default();
    struct Mark<'a>(&'static str, &'a Marks);
    impl Extension for Mark<'_> {
        fn id(&self) -> &'static str {
            self.0
        }
        fn on_event(&mut self, _: &Event<'_>, _: &Ctx<'_>) {
            // The publisher has already counted this event.
            let seq = READER.get().and_then(|r| r.latest()).map(|l| l.last.seq);
            self.1.borrow_mut().push((self.0, seq));
        }
    }
    let mut out = compose(
        Layer::App,
        events(),
        Mark("board", &order),
        extensions![Mark("library", &order), Mark("app", &order)],
        bounds(),
    );
    let reader = out.reader();
    READER.set(reader).ok();
    let mut rx = reader.receiver().unwrap();
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
    let a = reader.recent().try_take().unwrap();
    let b = reader.recent().try_take().unwrap();
    assert_eq!((a.seq, b.seq), (5, 6));
    assert_eq!(u32::from(a.dropped) + u32::from(b.dropped), 4);
    assert_eq!(reader.recent().try_take(), None);
    assert!(poll_once(reader.recent().take()).is_pending());
}

#[test]
fn drops_beyond_255_are_carried_to_later_records() {
    let mut chain = compose(Layer::App, events::<(), 1>(), Nop, Nop, bounds());
    let reader = chain.reader();
    let mut m = transfer::Machine::new();
    for _ in 0..600 {
        m.reset(&mut chain).unwrap();
    }
    // 600 pushed into a ring of one: 599 lost.
    let first = reader.recent().try_take().unwrap();
    assert_eq!(first.dropped, 255);
    let mut reported = u32::from(first.dropped);
    for _ in 0..3 {
        m.reset(&mut chain).unwrap();
        reported += u32::from(reader.recent().try_take().unwrap().dropped);
    }
    assert_eq!(reported, 599);
}

/// Answers after `cost_us` on the shared clock.
struct Slow<'a> {
    id: &'static str,
    now: &'a Cell<u64>,
    cost_us: u64,
    accept: Option<Accept>,
    delay: Option<Delay>,
    health: Option<Health>,
}

impl Extension for Slow<'_> {
    fn id(&self) -> &'static str {
        self.id
    }
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

fn slow<'a>(id: &'static str, now: &'a Cell<u64>, cost_us: u64) -> Slow<'a> {
    Slow {
        id,
        now,
        cost_us,
        accept: None,
        delay: None,
        health: None,
    }
}

fn trial() -> TrialInfo {
    TrialInfo::new(1, Version::new(1, 0, 0), 1, 3)
}

#[test]
fn decisions_default_when_every_extension_abstains() {
    let events = events::<(), 4>();
    let mut out = compose(Layer::App, events, Nop, extensions![], bounds());
    assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::Yes);
    assert_eq!(out.allow_reboot_now(&RebootRequest::new(0)), Delay::Now);
    assert_eq!(out.self_check(&trial()), Health::Healthy);
}

#[test]
fn the_first_answer_in_order_wins_and_a_delay_is_capped() {
    let events = events::<(), 4>();
    let now = Cell::new(0u64);
    let board = Slow {
        accept: Some(Accept::No),
        delay: Some(Delay::After(60_000)),
        ..slow("board", &now, 10)
    };
    let library = Slow {
        accept: Some(Accept::Yes),
        health: Some(Health::Pending),
        ..slow("library", &now, 10)
    };
    let wd = Watchdog::new(8_000, 30_000);
    assert_eq!(wd.reboot_delay_max_ms(), 4_000);
    let mut out = compose(
        Layer::App,
        events,
        board,
        extensions![library, slow("app", &now, 10)],
        Bounds::new(|| now.get(), wd),
    );
    assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::No);
    assert_eq!(
        out.allow_reboot_now(&RebootRequest::new(0)),
        Delay::After(4_000)
    );
    assert_eq!(out.self_check(&trial()), Health::Pending);
}

#[test]
fn each_extension_is_timed_alone_and_a_late_one_is_named() {
    let events = events::<(), 8>();
    let now = Cell::new(0u64);
    // Two libraries at 30 ms each stay within the bound one by one; the
    // app's 50.001 ms answer does not.
    let late_app = Slow {
        accept: Some(Accept::No),
        health: Some(Health::Unhealthy),
        ..slow("late-app", &now, 50_001)
    };
    let mut out = compose(
        Layer::App,
        events,
        Nop,
        extensions![
            slow("lib-a", &now, 30_000),
            slow("lib-b", &now, 30_000),
            late_app
        ],
        Bounds::new(|| now.get(), Watchdog::new(8_000, 8_000)),
    );
    assert_eq!(out.accept_update(&UpdateOffer::new(0, 1, 10)), Accept::Yes);
    assert_eq!(out.self_check(&trial()), Health::Healthy);
    let reader = out.reader();
    drop(out);

    let late = ExtId::of("late-app");
    let mut overruns = Vec::new();
    while let Some(r) = reader.recent().try_take() {
        overruns.push((
            r.kind,
            u64::from_le_bytes(r.payload[0..8].try_into().unwrap()),
            r.payload[8],
            u32::from_le_bytes(r.payload[9..13].try_into().unwrap()),
        ));
    }
    let code = decisions::ExtensionOverran::ENTRY.code;
    assert_eq!(
        overruns,
        [
            (code, late.hash, DecisionId::AcceptUpdate as u8, 50_001),
            (code, late.hash, DecisionId::SelfCheck as u8, 50_001),
        ]
    );
}

#[test]
fn a_live_overrun_names_the_extension() {
    let events = events::<(), 4>();
    let names: RefCell<Vec<&'static str>> = RefCell::default();
    struct Names<'a>(&'a RefCell<Vec<&'static str>>);
    impl Extension for Names<'_> {
        fn id(&self) -> &'static str {
            "names"
        }
        fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
            if let Some(o) = e.get::<decisions::ExtensionOverran>() {
                self.0.borrow_mut().push(o.id.name);
            }
        }
    }
    let now = Cell::new(0u64);
    let mut out = compose(
        Layer::App,
        events,
        Names(&names),
        slow("stuck", &now, 60_000),
        Bounds::new(|| now.get(), Watchdog::new(8_000, 8_000)),
    );
    assert_eq!(out.allow_reboot_now(&RebootRequest::new(0)), Delay::Now);
    drop(out);
    assert_eq!(names.into_inner(), ["stuck"]);
}
