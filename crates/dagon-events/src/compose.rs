//! The composed chain: the one order every layer uses, the sink every
//! transition delivers its event to, replay, and the bounded decision
//! calls.

use crate::catalog::{Catalog, Layer};
use crate::decisions;
use crate::event::{Event, EventRecord, Payload};
use crate::extension::{
    Accept, Ctx, DecisionId, Delay, ExtId, Extension, Health, Nop, RebootRequest, TrialInfo,
    UpdateOffer, DECISION_BOUND_MS,
};
use crate::published::{Published, Reader, Snapshot};
use crate::sealed::{SealedHead, SealedSink, Token};

/// The layer's monotonic clock in microseconds. DAGON's own: the MCU
/// package provides it from the timer its definition names (the host,
/// from the OS clock). It is not a port; extensions never provide it.
pub trait Clock {
    fn now_us(&mut self) -> u64;
}

impl<F: FnMut() -> u64> Clock for F {
    fn now_us(&mut self) -> u64 {
        self()
    }
}

/// The watchdog periods of the MCU definition (`[watchdog]`), from which
/// the reboot delay cap is derived. The layer's start passes the values
/// generated from its definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watchdog {
    trial_ms: u32,
    confirmed_ms: u32,
}

impl Watchdog {
    pub const fn new(trial_ms: u32, confirmed_ms: u32) -> Self {
        Watchdog {
            trial_ms,
            confirmed_ms,
        }
    }

    /// The longest delay `allow_reboot_now` may ask for: half the shorter
    /// period, so the watchdog never fires while DAGON waits.
    pub const fn reboot_delay_max_ms(self) -> u32 {
        if self.trial_ms < self.confirmed_ms {
            self.trial_ms / 2
        } else {
            self.confirmed_ms / 2
        }
    }
}

/// What bounds a layer's decision calls: the clock that times them and
/// the watchdog that caps a reboot delay. Layers with decision points
/// (the app and the relay) have both.
pub struct Bounds<C: Clock> {
    clock: C,
    watchdog: Watchdog,
}

impl<C: Clock> Bounds<C> {
    pub fn new(clock: C, watchdog: Watchdog) -> Self {
        Bounds { clock, watchdog }
    }
}

/// The head of a composed chain: the layer's publisher, or none (the boot
/// core and the host publish no waitable state). Sealed.
pub trait Head: SealedHead {
    #[doc(hidden)]
    fn publish(&self, e: &Event<'_>, _: Token);
}

/// No publisher.
#[derive(Clone, Copy, Debug)]
pub struct NoPublisher;

impl SealedHead for NoPublisher {}
impl Head for NoPublisher {
    fn publish(&self, _e: &Event<'_>, _: Token) {}
}

impl<S: Snapshot, const N: usize> SealedHead for &'static Published<S, N> {}
impl<S: Snapshot, const N: usize> Head for &'static Published<S, N> {
    fn publish(&self, e: &Event<'_>, _: Token) {
        Published::publish(self, e)
    }
}

/// Where a transition delivers its event: only a composed chain. Sealed;
/// its method is DAGON's and takes a payload only a machine's crate builds.
///
/// ```compile_fail,E0277
/// struct Tap;
/// impl dagon_events::Sink for Tap {
///     fn deliver<P: dagon_events::Payload>(&mut self, _p: P) {}
/// }
/// ```
pub trait Sink: SealedSink {
    #[doc(hidden)]
    fn deliver<P: Payload>(&mut self, p: P);
}

struct Parts<H, B, U> {
    layer: Layer,
    head: H,
    board: B,
    user: U,
}

impl<H: Head, B: Extension, U: Extension> Parts<H, B, U> {
    fn send(&mut self, e: &Event<'_>) {
        let ctx = Ctx::new(self.layer);
        self.head.publish(e, Token::new());
        self.board.on_event(e, &ctx);
        self.user.on_event(e, &ctx);
    }

    fn leaves(&self) -> usize {
        let k = Token::new();
        self.board.__leaves(k) + self.user.__leaves(k)
    }

    /// Which part holds extension `i`, and its index there.
    fn split(&self, i: usize) -> (bool, usize) {
        let n = self.board.__leaves(Token::new());
        if i < n {
            (true, i)
        } else {
            (false, i - n)
        }
    }

    fn id_at(&self, i: usize) -> &'static str {
        let k = Token::new();
        match self.split(i) {
            (true, j) => self.board.__id_at(j, k),
            (false, j) => self.user.__id_at(j, k),
        }
    }
}

impl<H: Head, B: Extension, U: Extension> SealedSink for Parts<H, B, U> {}
impl<H: Head, B: Extension, U: Extension> Sink for Parts<H, B, U> {
    fn deliver<P: Payload>(&mut self, p: P) {
        self.send(&Event::live(&p));
    }
}

/// A layer's chain in the one order every layer uses: the publisher (if
/// the layer has one), the board package, then the user's extensions
/// (libraries, then the app's own).
///
/// The trust boundary is the extension interface. Extensions receive only
/// `&Event`, `&Ctx` and the decision calls ([`Extension`]); nothing they
/// receive leads to a chain, a [`Sink`], a [`Published`] or replay.
/// Composing a chain, delivering into it and replaying are the layer
/// start's: the MCU package's start or the host, which make the chain and
/// keep it. Code built into the same image outside the extension
/// interface (DAGON's crates and the application's own start-up code) is
/// in the trusted base, as for everything else in one address space;
/// isolating untrusted extensions in hardware is the sandbox's part.
///
/// Its parts cannot be reached, reordered or reused, and it is not itself
/// an extension.
///
/// ```compile_fail,E0616
/// use dagon_events::{compose_unpublished, Layer, Nop};
/// let c = compose_unpublished(Layer::App, Nop, Nop);
/// let board = c.parts;
/// ```
///
/// An extension cannot turn what it receives into a sink:
///
/// ```compile_fail,E0277
/// use dagon_events::{Ctx, Event, Extension, Sink};
/// fn sink(_: &mut impl Sink) {}
/// struct Probe;
/// impl Extension for Probe {
///     fn id(&self) -> &'static str { "probe" }
///     fn on_event(&mut self, e: &Event<'_>, ctx: &Ctx<'_>) {
///         let mut e = *e;
///         sink(&mut e);
///     }
/// }
/// ```
///
/// ```compile_fail,E0277
/// use dagon_events::{Ctx, Event, Extension, Sink};
/// fn sink(_: &mut impl Sink) {}
/// struct Probe;
/// impl Extension for Probe {
///     fn id(&self) -> &'static str { "probe" }
///     fn on_event(&mut self, _: &Event<'_>, ctx: &Ctx<'_>) {
///         let mut c = ctx;
///         sink(&mut c);
///     }
/// }
/// ```
pub struct Composed<H: Head, B: Extension, U: Extension, C> {
    parts: Parts<H, B, U>,
    clock: C,
    watchdog: Watchdog,
    decisions: decisions::Machine,
}

impl<H: Head, B: Extension, U: Extension, C> SealedSink for Composed<H, B, U, C> {}
impl<H: Head, B: Extension, U: Extension, C> Sink for Composed<H, B, U, C> {
    fn deliver<P: Payload>(&mut self, p: P) {
        self.parts.deliver(p)
    }
}

/// The chain of a layer that publishes its events for display and log
/// tasks and answers decision points (the app, the relay). A layer-start
/// API: the layer package creates `events`, keeps it private and hands it
/// over here by exclusive reference, so the chain is its only writer;
/// tasks read it through [`Composed::reader`].
///
/// ```compile_fail,E0308
/// use dagon_events::{compose, Bounds, Layer, Nop, Published, Watchdog};
/// static EVENTS: Published<(), 4> = Published::new();
/// let c = compose(Layer::App, &EVENTS, Nop, Nop, Bounds::new(|| 0u64, Watchdog::new(8, 8)));
/// ```
pub fn compose<S: Snapshot, const N: usize, B: Extension, U: Extension, C: Clock>(
    layer: Layer,
    events: &'static mut Published<S, N>,
    board: B,
    user: U,
    bounds: Bounds<C>,
) -> Composed<&'static Published<S, N>, B, U, C> {
    Composed {
        parts: Parts {
            layer,
            head: events,
            board,
            user,
        },
        clock: bounds.clock,
        watchdog: bounds.watchdog,
        decisions: decisions::Machine::new(),
    }
}

/// The chain of a layer without published state or decision points (the
/// boot core, the host). A layer-start API, like [`compose`].
pub fn compose_unpublished<B: Extension, U: Extension>(
    layer: Layer,
    board: B,
    user: U,
) -> Composed<NoPublisher, B, U, ()> {
    Composed {
        parts: Parts {
            layer,
            head: NoPublisher,
            board,
            user,
        },
        clock: (),
        watchdog: Watchdog::new(0, 0),
        decisions: decisions::Machine::new(),
    }
}

/// One boot extension's limits, in chain order, as the node's
/// `boot_wiring.rs` states them from its manifest: the time limit per call
/// (`max_run_us`) and the codes of the boot event kinds its `observes`
/// names (resolved against the catalog by `dagon gen`, so the boot core
/// compares one byte per event).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootExtension {
    budget_us: u32,
    codes: &'static [u8],
}

impl BootExtension {
    pub const fn new(budget_us: u32, codes: &'static [u8]) -> Self {
        BootExtension { budget_us, codes }
    }

    pub const fn budget_us(&self) -> u32 {
        self.budget_us
    }

    /// Whether it observes `e`: a boot event whose code it lists.
    pub fn observes(&self, e: &Event<'_>) -> bool {
        let entry = e.entry();
        entry.layer == Layer::Boot && self.codes.contains(&entry.code)
    }
}

/// The boot core's chain: no publisher and no decision point (what the
/// boot core starts is not an extension's to change), the extensions the
/// node's `boot_wiring.rs` lists in its order, each with its
/// [`BootExtension`] limits. An extension is called only for the events
/// its manifest observes, and each call is timed on its own with the boot
/// core's clock against its `max_run_us`. A call over it is not preempted
/// (the boot core runs trusted extensions privileged); it is reported
/// right after it returns as `boot.extension_overran`, delivered to the
/// extensions that observe that kind (those calls are not timed, so a
/// report never causes another), and the boot goes on with the same
/// decision. A layer-start API.
pub struct BootComposed<U: Extension, C: Clock> {
    parts: Parts<NoPublisher, Nop, U>,
    clock: C,
    limits: &'static [BootExtension],
    exts: crate::boot_extensions::Machine,
}

/// See [`BootComposed`]. `limits` has one entry per extension of `user`,
/// in its order (`boot_wiring.rs` generates both from one list); anything
/// else is a generator fault and panics.
pub fn compose_boot<U: Extension, C: Clock>(
    user: U,
    clock: C,
    limits: &'static [BootExtension],
) -> BootComposed<U, C> {
    assert!(
        user.__leaves(Token::new()) == limits.len(),
        "compose_boot takes one BootExtension per extension of the chain"
    );
    BootComposed {
        parts: Parts {
            layer: Layer::Boot,
            head: NoPublisher,
            board: Nop,
            user,
        },
        clock,
        limits,
        exts: crate::boot_extensions::Machine::new(),
    }
}

/// Delivers to the boot extensions that observe the event, untimed: the
/// `boot.extension_overran` reports.
struct Observers<'a, U> {
    user: &'a mut U,
    limits: &'static [BootExtension],
}

impl<U: Extension> SealedSink for Observers<'_, U> {}
impl<U: Extension> Sink for Observers<'_, U> {
    fn deliver<P: Payload>(&mut self, p: P) {
        let e = Event::live(&p);
        let ctx = Ctx::new(Layer::Boot);
        let k = Token::new();
        // Bounded by the chain's own (constant) length, so a boot core
        // without extensions compiles to no delivery at all.
        for i in 0..self.user.__leaves(k) {
            if self.limits.get(i).is_some_and(|l| l.observes(&e)) {
                self.user.__event_at(i, &e, &ctx, k);
            }
        }
    }
}

impl<U: Extension, C: Clock> SealedSink for BootComposed<U, C> {}
impl<U: Extension, C: Clock> Sink for BootComposed<U, C> {
    fn deliver<P: Payload>(&mut self, p: P) {
        let e = Event::live(&p);
        let ctx = Ctx::new(Layer::Boot);
        let k = Token::new();
        // Bounded by the chain's own (constant) length, so a boot core
        // without extensions compiles to no delivery at all.
        for i in 0..self.parts.user.__leaves(k) {
            let Some(l) = self.limits.get(i).filter(|l| l.observes(&e)) else {
                continue;
            };
            let t0 = self.clock.now_us();
            self.parts.user.__event_at(i, &e, &ctx, k);
            let us = self.clock.now_us().saturating_sub(t0);
            if us > u64::from(l.budget_us) {
                let id = ExtId::of(self.parts.user.__id_at(i, k));
                let us = u32::try_from(us).unwrap_or(u32::MAX);
                let mut to = Observers {
                    user: &mut self.parts.user,
                    limits: self.limits,
                };
                // `overran` leaves from every state of its machine.
                let delivered = self.exts.overran(&mut to, id, l.budget_us, us);
                debug_assert!(delivered.is_ok());
            }
        }
    }
}

/// A record [`Composed::replay`] could not deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

impl<H: Head, B: Extension, U: Extension, C> Composed<H, B, U, C> {
    /// Re-emits a record an earlier stage kept (the boot record, the state
    /// log) as its typed event with `replayed: true`, decoded through the
    /// catalog of the machine that declared it. Only kinds the catalog
    /// marks `replay` and not `retired` are replayed. `Unreadable` when no
    /// catalog in `machines` reads it (another layer, an unknown or
    /// non-replayable kind, a value from a newer writer). A layer-start
    /// API.
    pub fn replay(&mut self, machines: &[Catalog], rec: &EventRecord) -> Result<(), Unreadable> {
        let parts = &mut self.parts;
        let mut deliver = |p: &dyn Payload| parts.send(&Event::replayed(p));
        if machines.iter().any(|m| (m.__replay)(rec, &mut deliver)) {
            Ok(())
        } else {
            Err(Unreadable)
        }
    }
}

const BOUND_US: u64 = DECISION_BOUND_MS as u64 * 1000;

impl<S: Snapshot, const N: usize, B: Extension, U: Extension, C: Clock>
    Composed<&'static Published<S, N>, B, U, C>
{
    /// Read access to the published events, for the layer start to hand
    /// to display, log and application tasks.
    pub fn reader(&self) -> Reader<S, N> {
        Reader::new(self.parts.head)
    }

    /// Decision 3.1: asked after every DAGON guard passed. Default: accept.
    pub fn accept_update(&mut self, o: &UpdateOffer) -> Accept {
        self.decide(DecisionId::AcceptUpdate, |p, i, c, k| match p.split(i) {
            (true, j) => p.board.__accept_at(j, o, c, k),
            (false, j) => p.user.__accept_at(j, o, c, k),
        })
        .unwrap_or(Accept::Yes)
    }

    /// Decision 3.2. Default: now; a delay is capped at the watchdog's
    /// [`Watchdog::reboot_delay_max_ms`].
    pub fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay {
        let cap = self.watchdog.reboot_delay_max_ms();
        let answer = self.decide(DecisionId::AllowRebootNow, |p, i, c, k| match p.split(i) {
            (true, j) => p.board.__reboot_at(j, r, c, k),
            (false, j) => p.user.__reboot_at(j, r, c, k),
        });
        match answer {
            None | Some(Delay::Now) | Some(Delay::After(0)) => Delay::Now,
            Some(Delay::After(ms)) => Delay::After(ms.min(cap)),
        }
    }

    /// Decision 3.3: asked when a slot on trial is confirmed. Default:
    /// healthy.
    pub fn self_check(&mut self, t: &TrialInfo) -> Health {
        self.decide(DecisionId::SelfCheck, |p, i, c, k| match p.split(i) {
            (true, j) => p.board.__check_at(j, t, c, k),
            (false, j) => p.user.__check_at(j, t, c, k),
        })
        .unwrap_or(Health::Healthy)
    }

    /// Asks each extension in chain order, timing each call on its own;
    /// a late answer is an abstention and an `extension_overran` event
    /// naming that extension.
    fn decide<T>(
        &mut self,
        point: DecisionId,
        mut ask: impl FnMut(
            &mut Parts<&'static Published<S, N>, B, U>,
            usize,
            &Ctx<'_>,
            Token,
        ) -> Option<T>,
    ) -> Option<T> {
        let ctx = Ctx::new(self.parts.layer);
        for i in 0..self.parts.leaves() {
            let t0 = self.clock.now_us();
            let answer = ask(&mut self.parts, i, &ctx, Token::new());
            let us = self.clock.now_us().saturating_sub(t0);
            if us > BOUND_US {
                let id = ExtId::of(self.parts.id_at(i));
                let us = u32::try_from(us).unwrap_or(u32::MAX);
                // `overran` leaves from every state of its machine.
                let delivered = self.decisions.overran(&mut self.parts, id, point, us);
                debug_assert!(delivered.is_ok());
                continue;
            }
            if answer.is_some() {
                return answer;
            }
        }
        None
    }
}
