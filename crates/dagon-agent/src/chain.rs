//! The app layer's composed chain as the agent and its image store use it.

use dagon_events::{
    Accept, Clock, Composed, Delay, Extension, Health, NotReplayed, Payload, Published,
    RebootRequest, Sink, Snapshot, TrialInfo, UpdateOffer,
};

/// The app layer's composed chain, as the agent uses it: where its
/// transitions deliver, the decision points, and replay. Implemented only
/// by `dagon_events::compose`'s chain (its supertrait is sealed), which the
/// MCU package's start makes and hands to [`Agent::new`].
pub trait AppChain: Sink {
    fn accept_update(&mut self, o: &UpdateOffer) -> Accept;
    fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay;
    fn self_check(&mut self, t: &TrialInfo) -> Health;
    fn replay(&mut self, p: &dyn Payload) -> Result<(), NotReplayed>;
}

impl<P: Snapshot, const N: usize, B: Extension, U: Extension, C: Clock> AppChain
    for Composed<&'static Published<P, N>, B, U, C>
{
    fn accept_update(&mut self, o: &UpdateOffer) -> Accept {
        Composed::accept_update(self, o)
    }
    fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay {
        Composed::allow_reboot_now(self, r)
    }
    fn self_check(&mut self, t: &TrialInfo) -> Health {
        Composed::self_check(self, t)
    }
    fn replay(&mut self, p: &dyn Payload) -> Result<(), NotReplayed> {
        Composed::replay(self, p)
    }
}
