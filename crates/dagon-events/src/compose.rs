//! The composed chain: the one order every layer uses, the sink every
//! transition delivers its event to, and the bounded decision calls.

use crate::catalog::Layer;
use crate::event::{Event, Payload};
use crate::extension::{
    decisions, Accept, Ctx, DecisionId, Delay, Extension, Health, Party, RebootRequest, TrialInfo,
    UpdateOffer, DECISION_BOUND_MS,
};
use crate::published::{Published, Snapshot};

mod sealed {
    pub trait Sealed {}
}

/// The layer's clock in microseconds, monotonic. A port: the MCU package
/// provides it from a hardware timer; the host from the OS clock.
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

/// What bounds the decision calls: the clock that times them and the
/// watchdog that caps a reboot delay.
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
pub trait Head: sealed::Sealed {
    #[doc(hidden)]
    fn publish(&self, e: &Event<'_>);
}

/// No publisher.
#[derive(Clone, Copy, Debug)]
pub struct NoPublisher;

impl sealed::Sealed for NoPublisher {}
impl Head for NoPublisher {
    fn publish(&self, _e: &Event<'_>) {}
}

impl<S: Snapshot, const N: usize> sealed::Sealed for &'static Published<S, N> {}
impl<S: Snapshot, const N: usize> Head for &'static Published<S, N> {
    fn publish(&self, e: &Event<'_>) {
        Published::publish(self, e)
    }
}

/// Where a transition delivers its event: only a composed chain. Sealed.
pub trait Sink: sealed::Sealed {
    /// Hands a payload that happened now to every part of the chain, in
    /// order.
    #[doc(hidden)]
    fn deliver<P: Payload>(&mut self, p: P);
}

struct Parts<H, B, U> {
    layer: Layer,
    head: H,
    board: B,
    user: U,
}

impl<H: Head, B: Extension, U: Extension> sealed::Sealed for Parts<H, B, U> {}
impl<H: Head, B: Extension, U: Extension> Sink for Parts<H, B, U> {
    fn deliver<P: Payload>(&mut self, p: P) {
        let e = Event::live(&p);
        let ctx = Ctx::new(self.layer);
        self.head.publish(&e);
        self.board.on_event(&e, &ctx);
        self.user.on_event(&e, &ctx);
    }
}

/// A layer's chain in the one order every layer uses: the publisher (if
/// the layer has one), the board package, then the user's extensions
/// (libraries, then the app's own). Made only by [`compose`] and
/// [`compose_unpublished`], which the layer's start calls; its parts
/// cannot be reached, reordered or reused, and it is not itself an
/// extension.
///
/// ```compile_fail,E0616
/// use dagon_events::{compose_unpublished, Bounds, Layer, Nop, Watchdog};
/// let c = compose_unpublished(Layer::App, Nop, Nop, Bounds::new(|| 0u64, Watchdog::new(8, 8)));
/// let board = c.parts;
/// ```
pub struct Composed<H: Head, B: Extension, U: Extension, C: Clock> {
    parts: Parts<H, B, U>,
    clock: C,
    watchdog: Watchdog,
    decisions: decisions::Machine,
}

impl<H: Head, B: Extension, U: Extension, C: Clock> sealed::Sealed for Composed<H, B, U, C> {}
impl<H: Head, B: Extension, U: Extension, C: Clock> Sink for Composed<H, B, U, C> {
    fn deliver<P: Payload>(&mut self, p: P) {
        self.parts.deliver(p)
    }
}

/// The chain of a layer that publishes its events (`events`) for display
/// and log tasks.
pub fn compose<S: Snapshot, const N: usize, B: Extension, U: Extension, C: Clock>(
    layer: Layer,
    events: &'static Published<S, N>,
    board: B,
    user: U,
    bounds: Bounds<C>,
) -> Composed<&'static Published<S, N>, B, U, C> {
    Composed::new(layer, events, board, user, bounds)
}

/// The chain of a layer without a publisher.
pub fn compose_unpublished<B: Extension, U: Extension, C: Clock>(
    layer: Layer,
    board: B,
    user: U,
    bounds: Bounds<C>,
) -> Composed<NoPublisher, B, U, C> {
    Composed::new(layer, NoPublisher, board, user, bounds)
}

const BOUND_US: u64 = DECISION_BOUND_MS as u64 * 1000;

impl<H: Head, B: Extension, U: Extension, C: Clock> Composed<H, B, U, C> {
    fn new(layer: Layer, head: H, board: B, user: U, bounds: Bounds<C>) -> Self {
        Composed {
            parts: Parts {
                layer,
                head,
                board,
                user,
            },
            clock: bounds.clock,
            watchdog: bounds.watchdog,
            decisions: decisions::Machine::new(),
        }
    }

    /// Decision 3.1: asked after every DAGON guard passed. Default: accept.
    pub fn accept_update(&mut self, o: &UpdateOffer) -> Accept {
        self.decide(DecisionId::AcceptUpdate, |x, c| x.accept_update(o, c))
            .unwrap_or(Accept::Yes)
    }

    /// Decision 3.2. Default: now; a delay is capped at the watchdog's
    /// [`Watchdog::reboot_delay_max_ms`].
    pub fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay {
        let cap = self.watchdog.reboot_delay_max_ms();
        match self.decide(DecisionId::AllowRebootNow, |x, c| x.allow_reboot_now(r, c)) {
            None | Some(Delay::Now) | Some(Delay::After(0)) => Delay::Now,
            Some(Delay::After(ms)) => Delay::After(ms.min(cap)),
        }
    }

    /// Decision 3.3: asked when a slot on trial is confirmed. Default:
    /// healthy.
    pub fn self_check(&mut self, t: &TrialInfo) -> Health {
        self.decide(DecisionId::SelfCheck, |x, c| x.self_check(t, c))
            .unwrap_or(Health::Healthy)
    }

    /// The first answer in chain order, each party timed; a late answer is
    /// an abstention and an `extension_overran` event.
    fn decide<T>(
        &mut self,
        point: DecisionId,
        mut ask: impl FnMut(&mut dyn Extension, &Ctx<'_>) -> Option<T>,
    ) -> Option<T> {
        let ctx = Ctx::new(self.parts.layer);
        for party in [Party::Board, Party::User] {
            let ext: &mut dyn Extension = match party {
                Party::Board => &mut self.parts.board,
                Party::User => &mut self.parts.user,
            };
            let t0 = self.clock.now_us();
            let answer = ask(ext, &ctx);
            let us = self.clock.now_us().saturating_sub(t0);
            if us > BOUND_US {
                let us = u32::try_from(us).unwrap_or(u32::MAX);
                // `overran` leaves from every state of its machine.
                let delivered = self.decisions.overran(&mut self.parts, point, party, us);
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
