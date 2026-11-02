//! STATUS from dagon-core's folded APP log and the headers currently in the
//! slots.

use dagon_core::log::{LogState, SlotRef};
use dagon_format::statelog::slot;
use dagon_proto::payload::{SlotInfo, SlotState, StatusResp};

/// `heads[s]`: the image header in slot `s` as a reference (`slot = s`),
/// `None` if empty. A log reference counts for a slot only while that same
/// header is still there.
pub fn status(
    log: &LogState,
    heads: [Option<SlotRef>; 2],
    active: u8,
    generation: u32,
) -> StatusResp {
    let mut st = StatusResp {
        active,
        confirmed: slot::NONE,
        attempts: log.attempts,
        last_reason: log.last_reason,
        log_generation: generation,
        ..Default::default()
    };
    for (s, head) in heads.iter().enumerate() {
        let Some(h) = head else { continue };
        let state = if log.pending == Some(*h) {
            if log.attempts > 0 {
                SlotState::Trying
            } else {
                SlotState::Pending
            }
        } else if log.confirmed == Some(*h) {
            st.confirmed = s as u8;
            SlotState::Confirmed
        } else if log.reverted == Some(s as u8) {
            SlotState::Reverted
        } else {
            SlotState::Written
        };
        st.slots[s] = SlotInfo {
            state,
            version: h.version,
            digest16: h.digest16,
        };
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_format::Version;

    fn r(slot: u8, minor: u8) -> SlotRef {
        SlotRef {
            slot,
            version: Version::new(1, minor, 0),
            digest16: [slot * 16 + minor; 16],
        }
    }

    #[test]
    fn states_follow_log_and_headers() {
        let (a, b) = (r(0, 0), r(1, 1));
        let mut log = LogState {
            confirmed: Some(a),
            ..Default::default()
        };
        let st = status(&log, [Some(a), None], 0, 1);
        assert_eq!(
            (st.slots[0].state, st.slots[1].state),
            (SlotState::Confirmed, SlotState::Empty)
        );
        assert_eq!(st.confirmed, 0);

        log.pending = Some(b);
        let st = status(&log, [Some(a), Some(b)], 0, 1);
        assert_eq!(st.slots[1].state, SlotState::Pending);
        log.attempts = 1;
        let st = status(&log, [Some(a), Some(b)], 1, 1);
        assert_eq!((st.slots[1].state, st.confirmed), (SlotState::Trying, 0));

        log.pending = None;
        log.reverted = Some(1);
        let st = status(&log, [Some(a), Some(b)], 0, 1);
        assert_eq!(st.slots[1].state, SlotState::Reverted);
    }
}
