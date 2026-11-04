//! The extension shape, composition, and the decision points.

use core::marker::PhantomData;

use dagon_format::version::Version;

use crate::catalog::Layer;
use crate::event::Event;

/// What DAGON hands an extension's handler: the layer it runs in and the
/// capabilities its manifest declared, nothing else.
#[derive(Debug)]
pub struct Ctx<'a> {
    layer: Layer,
    _caps: PhantomData<&'a ()>,
}

impl Ctx<'_> {
    pub fn new(layer: Layer) -> Self {
        Ctx {
            layer,
            _caps: PhantomData,
        }
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }
}

/// An update request that passed every DAGON guard (decision 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct UpdateOffer {
    pub target: u8,
    pub slot: u8,
    pub body_len: u32,
}

impl UpdateOffer {
    pub const fn new(target: u8, slot: u8, body_len: u32) -> Self {
        UpdateOffer {
            target,
            slot,
            body_len,
        }
    }
}

/// A reboot DAGON has accepted (decision 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RebootRequest {
    /// The protocol's reboot mode.
    pub mode: u8,
}

impl RebootRequest {
    pub const fn new(mode: u8) -> Self {
        RebootRequest { mode }
    }
}

/// The slot on trial that a confirm asks about (decision 3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TrialInfo {
    pub slot: u8,
    pub version: Version,
    pub attempt: u8,
    pub max: u8,
}

impl TrialInfo {
    pub const fn new(slot: u8, version: Version, attempt: u8, max: u8) -> Self {
        TrialInfo {
            slot,
            version,
            attempt,
            max,
        }
    }
}

/// Answer to `accept_update`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accept {
    Yes,
    /// Refused; the protocol answers its "state" status.
    No,
}

/// Answer to `allow_reboot_now`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    Now,
    /// Reset after this many milliseconds, capped by the layer.
    After(u32),
}

/// Answer to `self_check`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Unhealthy,
    /// Not decided yet: the confirm answers "state" and the host retries.
    Pending,
}

/// The one extension shape, in every layer: observe events, answer
/// decision points. Every method has a default (observe nothing, abstain),
/// and every method added to it has one. A port is a separate trait on the
/// same type.
pub trait Extension {
    /// Every event, in order.
    fn on_event(&mut self, _e: &Event<'_>, _ctx: &Ctx<'_>) {}

    /// Decision 3.1; `None` abstains.
    fn accept_update(&mut self, _o: &UpdateOffer, _ctx: &Ctx<'_>) -> Option<Accept> {
        None
    }

    /// Decision 3.2; `None` abstains.
    fn allow_reboot_now(&mut self, _r: &RebootRequest, _ctx: &Ctx<'_>) -> Option<Delay> {
        None
    }

    /// Decision 3.3; `None` abstains.
    fn self_check(&mut self, _t: &TrialInfo, _ctx: &Ctx<'_>) -> Option<Health> {
        None
    }
}

/// No extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct Nop;

impl Extension for Nop {}

/// `A`, then `B`: notifications reach both, decisions stop at the first
/// answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Chain<A, B>(pub A, pub B);

impl<A: Extension, B: Extension> Extension for Chain<A, B> {
    fn on_event(&mut self, e: &Event<'_>, ctx: &Ctx<'_>) {
        self.0.on_event(e, ctx);
        self.1.on_event(e, ctx);
    }

    fn accept_update(&mut self, o: &UpdateOffer, ctx: &Ctx<'_>) -> Option<Accept> {
        self.0
            .accept_update(o, ctx)
            .or_else(|| self.1.accept_update(o, ctx))
    }

    fn allow_reboot_now(&mut self, r: &RebootRequest, ctx: &Ctx<'_>) -> Option<Delay> {
        self.0
            .allow_reboot_now(r, ctx)
            .or_else(|| self.1.allow_reboot_now(r, ctx))
    }

    fn self_check(&mut self, t: &TrialInfo, ctx: &Ctx<'_>) -> Option<Health> {
        self.0
            .self_check(t, ctx)
            .or_else(|| self.1.self_check(t, ctx))
    }
}

impl<T: Extension + ?Sized> Extension for &mut T {
    fn on_event(&mut self, e: &Event<'_>, ctx: &Ctx<'_>) {
        (**self).on_event(e, ctx)
    }

    fn accept_update(&mut self, o: &UpdateOffer, ctx: &Ctx<'_>) -> Option<Accept> {
        (**self).accept_update(o, ctx)
    }

    fn allow_reboot_now(&mut self, r: &RebootRequest, ctx: &Ctx<'_>) -> Option<Delay> {
        (**self).allow_reboot_now(r, ctx)
    }

    fn self_check(&mut self, t: &TrialInfo, ctx: &Ctx<'_>) -> Option<Health> {
        (**self).self_check(t, ctx)
    }
}

/// The user's extensions, in order (libraries first, then the app's own):
/// `extensions![a, b, c]` is `Chain(a, Chain(b, c))`; `extensions![]` is
/// [`Nop`]. The layer's start puts DAGON's publisher and the board
/// package before them ([`crate::compose`]).
#[macro_export]
macro_rules! extensions {
    () => { $crate::Nop };
    ($a:expr $(,)?) => { $a };
    ($a:expr, $($rest:expr),+ $(,)?) => { $crate::Chain($a, $crate::extensions!($($rest),+)) };
}

/// Every decision call is answered within this bound; the serving task
/// measures it and reports an overrun.
pub const DECISION_BOUND_MS: u32 = 50;

/// The decision points with their defaults and bounds applied. The layer
/// calls these, never the trait methods directly.
pub mod decide {
    use super::*;

    /// Decision 3.1. Default: accept.
    pub fn accept_update(ext: &mut impl Extension, o: &UpdateOffer, ctx: &Ctx<'_>) -> Accept {
        ext.accept_update(o, ctx).unwrap_or(Accept::Yes)
    }

    /// Decision 3.2. Default: now. A delay is capped at `max_ms`
    /// ([`reboot_delay_max_ms`]).
    pub fn allow_reboot_now(
        ext: &mut impl Extension,
        r: &RebootRequest,
        ctx: &Ctx<'_>,
        max_ms: u32,
    ) -> Delay {
        match ext.allow_reboot_now(r, ctx) {
            None | Some(Delay::Now) | Some(Delay::After(0)) => Delay::Now,
            Some(Delay::After(ms)) => Delay::After(ms.min(max_ms)),
        }
    }

    /// Decision 3.3. Default: healthy.
    pub fn self_check(ext: &mut impl Extension, t: &TrialInfo, ctx: &Ctx<'_>) -> Health {
        ext.self_check(t, ctx).unwrap_or(Health::Healthy)
    }

    /// The reboot delay cap: half the shorter of the MCU definition's
    /// trial and confirmed watchdog periods.
    pub const fn reboot_delay_max_ms(trial_ms: u32, confirmed_ms: u32) -> u32 {
        if trial_ms < confirmed_ms {
            trial_ms / 2
        } else {
            confirmed_ms / 2
        }
    }
}

/// One decision point. Everything not listed in [`DECISIONS`] is a
/// notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionPoint {
    pub id: &'static str,
    pub layer: Layer,
    pub answers: &'static str,
    pub answered_by: &'static str,
    pub bound: &'static str,
    pub default: &'static str,
    pub never: &'static str,
}

/// The decision points. There is none in the boot layer: what the boot
/// core starts is changed only by a trusted `DecisionPolicy` port.
pub const DECISIONS: &[DecisionPoint] = &[
    DecisionPoint {
        id: "accept_update",
        layer: Layer::App,
        answers: "`UpdateOffer` -> `Accept`",
        answered_by: "board package, libraries, user (first answer wins)",
        bound: "`DECISION_BOUND_MS`; asked after every DAGON guard passed, before erase",
        default: "accept",
        never: "accepting what DAGON refused (not asked then)",
    },
    DecisionPoint {
        id: "allow_reboot_now",
        layer: Layer::App,
        answers: "`RebootRequest` -> `Delay`",
        answered_by: "board package, libraries, user (first answer wins)",
        bound: "`Now` or `After(ms)`, ms capped at `reboot_delay_max_ms` of the MCU definition's watchdog periods; DAGON replies first and keeps feeding while it waits",
        default: "now",
        never: "refusing a reboot; a delay beyond the cap",
    },
    DecisionPoint {
        id: "self_check",
        layer: Layer::App,
        answers: "`TrialInfo` -> `Health`",
        answered_by: "board package, libraries, user (first answer wins)",
        bound: "`DECISION_BOUND_MS`; `Pending` keeps the \"state\" status and the host retries",
        default: "healthy",
        never: "confirming a slot not on trial; confirming outside the `confirmed` transition",
    },
    DecisionPoint {
        id: "confirm_plan",
        layer: Layer::Host,
        answers: "the deploy listing -> yes or no",
        answered_by: "the interactive user (terminal, page) or `--yes`; never actions or host extensions",
        bound: "until the user answers",
        default: "ask",
        never: "skipping the key check and image verification, done before the listing is shown",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    struct Refuse;
    impl Extension for Refuse {
        fn accept_update(&mut self, _: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
            Some(Accept::No)
        }
    }

    struct Wait(u32);
    impl Extension for Wait {
        fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
            Some(Delay::After(self.0))
        }
        fn accept_update(&mut self, _: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
            Some(Accept::Yes)
        }
    }

    #[test]
    fn decisions_default_when_every_extension_abstains() {
        let ctx = Ctx::new(Layer::App);
        let mut none = extensions![];
        let o = UpdateOffer::new(0, 1, 10);
        assert_eq!(decide::accept_update(&mut none, &o, &ctx), Accept::Yes);
        let r = RebootRequest::new(0);
        assert_eq!(
            decide::allow_reboot_now(&mut none, &r, &ctx, 100),
            Delay::Now
        );
        let t = TrialInfo::new(1, Version::new(1, 0, 0), 1, 3);
        assert_eq!(decide::self_check(&mut none, &t, &ctx), Health::Healthy);
    }

    #[test]
    fn the_first_answer_wins_and_delays_are_capped() {
        let ctx = Ctx::new(Layer::App);
        let o = UpdateOffer::new(0, 1, 10);
        let mut a = extensions![Nop, Refuse, Wait(10_000)];
        assert_eq!(decide::accept_update(&mut a, &o, &ctx), Accept::No);
        let mut b = extensions![Wait(10_000), Refuse];
        assert_eq!(decide::accept_update(&mut b, &o, &ctx), Accept::Yes);
        let r = RebootRequest::new(0);
        let cap = decide::reboot_delay_max_ms(8_000, 30_000);
        assert_eq!(cap, 4_000);
        assert_eq!(
            decide::allow_reboot_now(&mut b, &r, &ctx, cap),
            Delay::After(4_000)
        );
    }

    #[test]
    fn decision_ids_are_unique() {
        for (i, a) in DECISIONS.iter().enumerate() {
            assert!(DECISIONS[i + 1..].iter().all(|b| b.id != a.id), "{}", a.id);
        }
    }
}
