//! What otadata's selected entry says about its slot, in STATUS terms.
//!
//! The rollback-enabled 2nd stage turns an entry left PendingVerify into
//! Aborted on the next boot and falls back to the other slot; STATUS reports
//! that tried slot as Reverted with reason TRIAL_EXHAUSTED.

use dagon_format::statelog::reason;
use dagon_proto::payload::SlotState;

use crate::layout::SLOT_FACTORY;

/// Raw `ota_state` values (same as `esp_bootloader_esp_idf::ota::OtaImageState`).
pub mod raw {
    pub const NEW: u32 = 0x0;
    pub const PENDING_VERIFY: u32 = 0x1;
    pub const VALID: u32 = 0x2;
    pub const INVALID: u32 = 0x3;
    pub const ABORTED: u32 = 0x4;
    pub const UNDEFINED: u32 = 0xFFFF_FFFF;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedView {
    /// OTA slot otadata selects.
    pub slot: u8,
    pub state: SlotState,
    /// Last rollback reason, when the selection was rolled back.
    pub reason: Option<u8>,
}

/// Maps otadata's selection to the selected slot's STATUS state. `None` when
/// otadata is blank (factory) or the state value is unknown; the other slot's
/// state then comes from its header alone.
pub fn selected_view(running: u8, selected: u8, state: Option<u32>) -> Option<SelectedView> {
    if selected == SLOT_FACTORY {
        return None;
    }
    let is_running = selected == running;
    let slot_state = match state? {
        raw::NEW => SlotState::Pending,
        raw::PENDING_VERIFY if is_running => SlotState::Trying,
        // Not booted although selected and on trial: the 2nd stage fell back.
        raw::PENDING_VERIFY | raw::ABORTED | raw::INVALID => SlotState::Reverted,
        raw::VALID | raw::UNDEFINED => SlotState::Confirmed,
        _ => return None,
    };
    Some(SelectedView {
        slot: selected,
        state: slot_state,
        reason: (slot_state == SlotState::Reverted).then_some(reason::TRIAL_EXHAUSTED),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{SLOT_A, SLOT_B};

    fn view(running: u8, selected: u8, state: u32) -> Option<(SlotState, Option<u8>)> {
        selected_view(running, selected, Some(state)).map(|v| (v.state, v.reason))
    }

    #[test]
    fn aborted_trial_reports_reverted_trial_exhausted() {
        // B was tried, never confirmed; the 2nd stage aborted it and booted A.
        assert_eq!(
            view(SLOT_A, SLOT_B, raw::ABORTED),
            Some((SlotState::Reverted, Some(reason::TRIAL_EXHAUSTED)))
        );
        assert_eq!(
            view(SLOT_A, SLOT_B, raw::INVALID),
            Some((SlotState::Reverted, Some(reason::TRIAL_EXHAUSTED)))
        );
        assert_eq!(
            view(SLOT_A, SLOT_B, raw::PENDING_VERIFY).unwrap().0,
            SlotState::Reverted
        );
    }

    #[test]
    fn trial_pending_and_confirmed() {
        assert_eq!(
            view(SLOT_B, SLOT_B, raw::PENDING_VERIFY),
            Some((SlotState::Trying, None))
        );
        assert_eq!(
            view(SLOT_A, SLOT_B, raw::NEW),
            Some((SlotState::Pending, None))
        );
        assert_eq!(
            view(SLOT_B, SLOT_B, raw::VALID),
            Some((SlotState::Confirmed, None))
        );
        assert_eq!(
            view(SLOT_A, SLOT_A, raw::UNDEFINED),
            Some((SlotState::Confirmed, None))
        );
    }

    #[test]
    fn blank_or_unknown_otadata_says_nothing() {
        assert_eq!(selected_view(SLOT_FACTORY, SLOT_FACTORY, None), None);
        assert_eq!(selected_view(SLOT_A, SLOT_A, None), None);
        assert_eq!(view(SLOT_A, SLOT_A, 0x7), None);
    }
}
