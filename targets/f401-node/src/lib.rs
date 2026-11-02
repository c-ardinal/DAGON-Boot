//! Board-independent parts of the NUCLEO-F401RE app, kept apart from the
//! HAL so they run in host tests: the kind S slot store behind dagon-agent,
//! the I2C target mailbox and the 7-segment text.
#![no_std]
#![forbid(unsafe_code)]

pub mod mailbox;
pub mod seg;
mod store;

pub use store::KindSStore;

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogState;

/// Scratch address when no slot may be destroyed: outside the flash, so the
/// conformance erase/write tests fail ("outside flash") without erasing.
pub const NO_SCRATCH: u32 = 0xFFFF_FFFF;

/// The APP slot a conformance build may destroy as scratch: neither running,
/// nor confirmed, nor holding the pending (staged or on trial) image. `None`
/// on trial and while an update waits for its first boot.
pub fn scratch_slot(state: &LogState, running: u8) -> Option<u8> {
    let held = [state.confirmed, state.pending].map(|r| r.map(|r| r.slot));
    (0..2u8).find(|&s| s != running && !held.contains(&Some(s)))
}

/// Start of the scratch slot, or [`NO_SCRATCH`].
pub fn scratch_addr(state: &LogState, running: u8, slots: &SlotLayout) -> u32 {
    scratch_slot(state, running).map_or(NO_SCRATCH, |s| slots.slots[s as usize])
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
        // B staged as pending, not started yet: it must survive too.
        let staged = LogState {
            confirmed: at(0),
            pending: at(1),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&staged, 0), None);
    }
}
