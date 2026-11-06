//! A trusted boot extension to copy: this one counts the events and keeps
//! what the decision was; replace `on_event` with what the node should do.
//! The rules it runs under: <https://c-ardinal.github.io/DAGON-Boot/doc/reference/extension-manifest/#boot-extensions>
#![no_std]

use dagon_boot::sequence;
use dagon_events::{Ctx, Event, Extension};

/// What the boot core links: `boot_wiring.rs` names these two.
pub type DagonExtension = BootCount;
pub const DAGON_EXTENSION: DagonExtension = BootCount {
    events: 0,
    chosen: None,
};

pub struct BootCount {
    events: u32,
    /// The slot the boot core chose, and whether on trial.
    chosen: Option<(u8, bool)>,
}

impl BootCount {
    pub fn events(&self) -> u32 {
        self.events
    }

    pub fn chosen(&self) -> Option<(u8, bool)> {
        self.chosen
    }
}

impl Extension for BootCount {
    fn id(&self) -> &'static str {
        // The manifest's id.
        "boot-count"
    }

    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        self.events += 1;
        // Each event's payload as its type (<https://c-ardinal.github.io/DAGON-Boot/doc/reference/events/>, layer `boot`).
        if let Some(c) = e.get::<sequence::SlotChosen>() {
            self.chosen = Some((c.slot, c.trial));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use critical_section as _;

    #[test]
    fn it_starts_with_nothing_seen() {
        let x = DAGON_EXTENSION;
        assert_eq!((x.events(), x.chosen(), x.id()), (0, None, "boot-count"));
    }
}
