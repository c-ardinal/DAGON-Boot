//! The extension shape, composition, and the decision points.

use core::marker::PhantomData;

use dagon_format::version::Version;

use crate::catalog::Layer;
use crate::compact::Compact;
use crate::event::Event;
use crate::sealed::Token;

/// What DAGON hands an extension's handler: the layer it runs in and the
/// capabilities its manifest declared, nothing else.
#[derive(Debug)]
pub struct Ctx<'a> {
    layer: Layer,
    _caps: PhantomData<&'a ()>,
}

impl Ctx<'_> {
    pub(crate) fn new(layer: Layer) -> Self {
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
/// decision points. Every method but [`Extension::id`] has a default
/// (observe nothing, abstain), and every method added to it has one. A
/// port is a separate trait on the same type.
///
/// The composed chain calls and times each extension on its own; the
/// methods that do that are DAGON's and cannot be overridden or called
/// outside this crate.
///
/// ```compile_fail,E0603
/// struct Sneaky;
/// impl dagon_events::Extension for Sneaky {
///     fn id(&self) -> &'static str { "sneaky" }
///     fn __leaves(&self, _: dagon_events::sealed::Token) -> usize { 0 }
/// }
/// ```
pub trait Extension {
    /// The extension's identity in events (`app.extension_overran`): its
    /// manifest id where it has one; otherwise a name unique in the image.
    fn id(&self) -> &'static str;

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

    #[doc(hidden)]
    fn __leaves(&self, _: Token) -> usize {
        1
    }

    #[doc(hidden)]
    fn __id_at(&self, _i: usize, _: Token) -> &'static str {
        self.id()
    }

    #[doc(hidden)]
    fn __accept_at(
        &mut self,
        _i: usize,
        o: &UpdateOffer,
        ctx: &Ctx<'_>,
        _: Token,
    ) -> Option<Accept> {
        self.accept_update(o, ctx)
    }

    #[doc(hidden)]
    fn __reboot_at(
        &mut self,
        _i: usize,
        r: &RebootRequest,
        ctx: &Ctx<'_>,
        _: Token,
    ) -> Option<Delay> {
        self.allow_reboot_now(r, ctx)
    }

    #[doc(hidden)]
    fn __check_at(&mut self, _i: usize, t: &TrialInfo, ctx: &Ctx<'_>, _: Token) -> Option<Health> {
        self.self_check(t, ctx)
    }
}

/// No extension. It has no parts, so the chain asks it nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Nop;

impl Extension for Nop {
    fn id(&self) -> &'static str {
        "nop"
    }

    fn __leaves(&self, _: Token) -> usize {
        0
    }
}

/// `A`, then `B`: notifications reach both; each part is asked a decision
/// on its own, in order, and the first answer stands. Built by
/// [`crate::extensions!`]; its parts stay inside.
#[derive(Clone, Copy, Debug, Default)]
pub struct Chain<A, B> {
    first: A,
    then: B,
}

impl<A: Extension, B: Extension> Chain<A, B> {
    pub const fn new(first: A, then: B) -> Self {
        Chain { first, then }
    }
}

impl<A: Extension, B: Extension> Extension for Chain<A, B> {
    fn id(&self) -> &'static str {
        "chain"
    }

    fn on_event(&mut self, e: &Event<'_>, ctx: &Ctx<'_>) {
        self.first.on_event(e, ctx);
        self.then.on_event(e, ctx);
    }

    fn accept_update(&mut self, o: &UpdateOffer, ctx: &Ctx<'_>) -> Option<Accept> {
        self.first
            .accept_update(o, ctx)
            .or_else(|| self.then.accept_update(o, ctx))
    }

    fn allow_reboot_now(&mut self, r: &RebootRequest, ctx: &Ctx<'_>) -> Option<Delay> {
        self.first
            .allow_reboot_now(r, ctx)
            .or_else(|| self.then.allow_reboot_now(r, ctx))
    }

    fn self_check(&mut self, t: &TrialInfo, ctx: &Ctx<'_>) -> Option<Health> {
        self.first
            .self_check(t, ctx)
            .or_else(|| self.then.self_check(t, ctx))
    }

    fn __leaves(&self, k: Token) -> usize {
        self.first.__leaves(k) + self.then.__leaves(k)
    }

    fn __id_at(&self, i: usize, k: Token) -> &'static str {
        let n = self.first.__leaves(k);
        if i < n {
            self.first.__id_at(i, k)
        } else {
            self.then.__id_at(i - n, k)
        }
    }

    fn __accept_at(
        &mut self,
        i: usize,
        o: &UpdateOffer,
        ctx: &Ctx<'_>,
        k: Token,
    ) -> Option<Accept> {
        let n = self.first.__leaves(k);
        if i < n {
            self.first.__accept_at(i, o, ctx, k)
        } else {
            self.then.__accept_at(i - n, o, ctx, k)
        }
    }

    fn __reboot_at(
        &mut self,
        i: usize,
        r: &RebootRequest,
        ctx: &Ctx<'_>,
        k: Token,
    ) -> Option<Delay> {
        let n = self.first.__leaves(k);
        if i < n {
            self.first.__reboot_at(i, r, ctx, k)
        } else {
            self.then.__reboot_at(i - n, r, ctx, k)
        }
    }

    fn __check_at(&mut self, i: usize, t: &TrialInfo, ctx: &Ctx<'_>, k: Token) -> Option<Health> {
        let n = self.first.__leaves(k);
        if i < n {
            self.first.__check_at(i, t, ctx, k)
        } else {
            self.then.__check_at(i - n, t, ctx, k)
        }
    }
}

impl<T: Extension + ?Sized> Extension for &mut T {
    fn id(&self) -> &'static str {
        (**self).id()
    }

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

    fn __leaves(&self, k: Token) -> usize {
        (**self).__leaves(k)
    }

    fn __id_at(&self, i: usize, k: Token) -> &'static str {
        (**self).__id_at(i, k)
    }

    fn __accept_at(
        &mut self,
        i: usize,
        o: &UpdateOffer,
        ctx: &Ctx<'_>,
        k: Token,
    ) -> Option<Accept> {
        (**self).__accept_at(i, o, ctx, k)
    }

    fn __reboot_at(
        &mut self,
        i: usize,
        r: &RebootRequest,
        ctx: &Ctx<'_>,
        k: Token,
    ) -> Option<Delay> {
        (**self).__reboot_at(i, r, ctx, k)
    }

    fn __check_at(&mut self, i: usize, t: &TrialInfo, ctx: &Ctx<'_>, k: Token) -> Option<Health> {
        (**self).__check_at(i, t, ctx, k)
    }
}

/// The user's extensions, in order (libraries first, then the app's own):
/// `extensions![a, b, c]` is `Chain::new(a, Chain::new(b, c))`;
/// `extensions![]` is [`Nop`]. The layer's start puts DAGON's publisher
/// and the board package before them ([`crate::compose`]); a composed
/// chain is not an extension, so it cannot be nested in another.
///
/// ```compile_fail,E0277
/// use dagon_events::{compose_unpublished, extensions, Layer, Nop};
/// let inner = compose_unpublished(Layer::App, Nop, Nop);
/// let outer = extensions![inner, Nop];
/// ```
///
/// ```compile_fail,E0423
/// let c = dagon_events::Chain(dagon_events::Nop, dagon_events::Nop);
/// ```
#[macro_export]
macro_rules! extensions {
    () => { $crate::Nop };
    ($a:expr $(,)?) => { $crate::Chain::new($a, $crate::Nop) };
    ($a:expr, $($rest:expr),+ $(,)?) => { $crate::Chain::new($a, $crate::extensions!($($rest),+)) };
}

/// The bound on every decision call. The composed chain times each
/// extension's call on its own with the layer's clock; an answer that
/// took longer is ignored (that extension abstains) and
/// `app.extension_overran` names it. Where the MCU's isolation level
/// allows it, the sandbox's timer also preempts the call at this bound;
/// elsewhere the call runs to its end and only its answer is dropped.
pub const DECISION_BOUND_MS: u32 = 50;

/// A decision point, as named in `app.extension_overran`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecisionId {
    #[default]
    AcceptUpdate,
    AllowRebootNow,
    SelfCheck,
}

impl Compact for DecisionId {
    const SIZE: usize = 1;
    fn put(&self, out: &mut [u8]) {
        out[0] = *self as u8;
    }
    fn get(b: &[u8]) -> Option<Self> {
        match b {
            [0] => Some(DecisionId::AcceptUpdate),
            [1] => Some(DecisionId::AllowRebootNow),
            [2] => Some(DecisionId::SelfCheck),
            _ => None,
        }
    }
}

/// An extension's identity in an event: its id, and in the owned record
/// the id's 64-bit FNV-1a hash (a replayed event carries the hash only;
/// its `name` is empty).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtId {
    pub name: &'static str,
    pub hash: u64,
}

impl ExtId {
    pub const fn of(name: &'static str) -> Self {
        let b = name.as_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        ExtId { name, hash: h }
    }
}

impl Compact for ExtId {
    const SIZE: usize = 8;
    fn put(&self, out: &mut [u8]) {
        self.hash.put(out);
    }
    fn get(b: &[u8]) -> Option<Self> {
        Some(ExtId {
            name: "",
            hash: u64::get(b)?,
        })
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
        bound: "each call within `DECISION_BOUND_MS` (a later answer is ignored); asked after every DAGON guard passed, before erase",
        default: "accept",
        never: "accepting what DAGON refused (not asked then)",
    },
    DecisionPoint {
        id: "allow_reboot_now",
        layer: Layer::App,
        answers: "`RebootRequest` -> `Delay`",
        answered_by: "board package, libraries, user (first answer wins)",
        bound: "each call within `DECISION_BOUND_MS`; `Now` or `After(ms)`, ms capped at half the shorter watchdog period of the MCU definition; DAGON replies first and keeps feeding while it waits",
        default: "now",
        never: "refusing a reboot; a delay beyond the cap",
    },
    DecisionPoint {
        id: "self_check",
        layer: Layer::App,
        answers: "`TrialInfo` -> `Health`",
        answered_by: "board package, libraries, user (first answer wins)",
        bound: "each call within `DECISION_BOUND_MS` (a later answer is ignored); `Pending` keeps the \"state\" status and the host retries",
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
