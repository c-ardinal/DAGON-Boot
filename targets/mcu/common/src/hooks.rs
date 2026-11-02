//! The agent hooks an app supplies: its health check on CONFIRM.

use dagon_agent::Hooks;

/// Hooks whose CONFIRM self-check is the app's `fn() -> bool` (`false`
/// keeps the version on trial, which then rolls back). Everything else is
/// dagon-agent's default.
#[derive(Clone, Copy)]
pub struct SelfCheck(pub fn() -> bool);

impl Hooks for SelfCheck {
    fn self_check(&mut self) -> bool {
        (self.0)()
    }
}

/// Always confirm (dagon-agent's default).
pub fn always() -> bool {
    true
}
