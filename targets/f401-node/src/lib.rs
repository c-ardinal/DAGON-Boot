//! Board-independent parts of the NUCLEO-F401RE app, kept apart from the
//! HAL so they run in host tests: the kind S slot store behind dagon-agent,
//! the I2C target mailbox and the 7-segment text.
#![no_std]
#![forbid(unsafe_code)]

pub mod mailbox;
pub mod seg;
mod store;

pub use store::KindSStore;

use dagon_core::log::LogState;

/// The APP slot a conformance build may destroy as scratch: the one neither
/// running nor confirmed. `None` while on trial (one slot runs, the other is
/// the confirmed one to roll back to).
pub fn scratch_slot(state: &LogState, running: u8) -> Option<u8> {
    let confirmed = state.confirmed.map(|c| c.slot);
    (0..2u8).find(|&s| s != running && Some(s) != confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_core::log::SlotRef;
    use dagon_format::Version;

    fn at(slot: u8) -> Option<SlotRef> {
        Some(SlotRef {
            slot,
            version: Version::default(),
            digest16: [0; 16],
        })
    }

    #[test]
    fn scratch_is_never_running_or_confirmed() {
        let confirmed_a = LogState {
            confirmed: at(0),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&confirmed_a, 0), Some(1));
        let confirmed_b = LogState {
            confirmed: at(1),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&confirmed_b, 1), Some(0));
        // Trial of B with A confirmed: no slot to spare.
        let trial = LogState {
            confirmed: at(0),
            pending: at(1),
            attempts: 1,
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&trial, 1), None);
    }
}
