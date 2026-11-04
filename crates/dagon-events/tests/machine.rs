//! What `state_machine!` generates, seen from another crate, and the
//! published state fed through the composed chain.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use dagon_events::{
    compose, extensions, state_machine, Compact, Ctx, Event, EventRecord, Extension, Layer, Nop,
    Observe, Payload, Published, Refused, Snapshot,
};

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
            written(end: u32): Body { slot, len, .. } => Body { slot, len, next: end }
                emits TransferProgress = 0x12 { done: u32 = end, total: u32 = len, last: Option<u8> = None }
                observe(live, replay, host);
            aborted(cause: Cause): Header { .. } | Body { .. } => Idle
                emits TransferAborted = 0x13 { cause: Cause }
                observe(host) retired;
            reset(): * => Idle
                emits Reset = 0x14 {}
                observe(live);
        }
    }
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
fn transitions_move_the_state_and_return_their_event() {
    let mut m = transfer::Machine::new();
    let ev = m.offered(1, 300).unwrap();
    assert_eq!((ev.slot, ev.body_len), (1, 300));
    assert_eq!(m.state(), transfer::State::Header { slot: 1, len: 300 });
    let _ = m.header_ok().unwrap();
    let p = m.written(128).unwrap();
    assert_eq!((p.done, p.total), (128, 300));
    assert_eq!(
        m.state(),
        transfer::State::Body {
            slot: 1,
            len: 300,
            next: 128
        }
    );
    let r = m.offered(2, 1).unwrap_err();
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
    assert_eq!(m.state().name(), "Body");
    let a = m.aborted(Cause::Store).unwrap();
    assert_eq!(a.cause, Cause::Store);
    assert_eq!(m.state(), transfer::State::Idle);
    assert!(m.aborted(Cause::Idle).is_err());
    let _ = m.reset().unwrap();
}

#[test]
fn an_event_carries_its_entry_its_payload_and_its_owned_form() {
    let mut m = transfer::Machine::new();
    let _ = m.offered(3, 10).unwrap();
    let _ = m.header_ok().unwrap();
    let p = m.written(0x0102_0304).unwrap();
    let e = Event::replayed(&p);
    assert_eq!((e.layer(), e.kind()), (Layer::App, "transfer_progress"));
    assert!(e.is_replayed());
    assert_eq!(e.get::<transfer::TransferProgress>().unwrap().total, 10);
    assert!(e.get::<transfer::UpdateOffered>().is_none());
    let r = e.record(9);
    assert_eq!(
        r,
        EventRecord {
            layer: Layer::App.code(),
            kind: 0x12,
            flags: EventRecord::REPLAYED,
            dropped: 0,
            seq: 9,
            payload: [4, 3, 2, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    );
    assert_eq!(p.compact(), r.payload);
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

#[derive(Default)]
struct Seen(Vec<&'static str>);

struct Log(&'static std::sync::Mutex<Seen>);

impl Extension for Log {
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.0.lock().unwrap().0.push(e.kind());
    }
}

#[test]
fn the_composed_chain_publishes_first_and_reaches_every_extension() {
    static EVENTS: Published<Progress, 2> = Published::new();
    static BOARD: std::sync::Mutex<Seen> = std::sync::Mutex::new(Seen(Vec::new()));
    static USER: std::sync::Mutex<Seen> = std::sync::Mutex::new(Seen(Vec::new()));
    let mut rx = EVENTS.latest().receiver().unwrap();
    let mut chain = compose(&EVENTS, Log(&BOARD), extensions![Nop, Log(&USER)]);
    let ctx = Ctx::new(Layer::App);
    let mut m = transfer::Machine::new();
    let mut emit = |p: &dyn Payload| chain.on_event(&Event::live(p), &ctx);
    emit(&m.offered(0, 100).unwrap());
    emit(&m.header_ok().unwrap());
    for end in [25, 50, 75, 100] {
        emit(&m.written(end).unwrap());
    }

    let Poll::Ready(l) = poll_once(rx.changed()) else {
        panic!("a change was published")
    };
    assert_eq!(l.state, Progress(Some((100, 100))));
    assert_eq!((l.last.seq, l.last.kind), (6, 0x12));
    assert_eq!(BOARD.lock().unwrap().0.len(), 6);
    assert_eq!(USER.lock().unwrap().0, BOARD.lock().unwrap().0);

    // A ring of 2 keeps the newest two; they account for the four dropped.
    let a = EVENTS.recent().try_take().unwrap();
    let b = EVENTS.recent().try_take().unwrap();
    assert_eq!((a.seq, b.seq), (5, 6));
    assert_eq!(u32::from(a.dropped) + u32::from(b.dropped), 4);
    assert_eq!(EVENTS.recent().try_take(), None);
    assert!(poll_once(EVENTS.recent().take()).is_pending());
}
