//! STATUS from the folded log and the headers currently in the slots.

use dagon_format::statelog::slot;
use dagon_proto::payload::{SlotInfo, SlotState, StatusResp};

use crate::log::{Fold, Mark};

/// `heads[s]`: the image header in slot `s` as a mark (`slot = s`), `None`
/// if empty.
/// A log mark counts for a slot only while that same header is still there.
pub fn status(fold: &Fold, heads: [Option<Mark>; 2], active: u8, generation: u32) -> StatusResp {
    let mut st = StatusResp {
        active,
        confirmed: slot::NONE,
        attempts: fold.attempts,
        last_reason: fold.last_reason,
        log_generation: generation,
        ..Default::default()
    };
    for (s, head) in heads.iter().enumerate() {
        let Some(h) = head else { continue };
        let is = |m: Option<Mark>| m == Some(*h);
        let state = if is(fold.pending) {
            if fold.attempts > 0 {
                SlotState::Trying
            } else {
                SlotState::Pending
            }
        } else if is(fold.confirmed) {
            SlotState::Confirmed
        } else if is(fold.reverted) {
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
    // A confirmed image stays the confirmed slot while another one is on trial.
    if let Some(c) = fold.confirmed {
        if heads.get(c.slot as usize).copied().flatten() == Some(c) {
            st.confirmed = c.slot;
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::tests::mark;

    #[test]
    fn states_follow_log_and_headers() {
        let a = mark(0, 0);
        let b = mark(1, 1);
        let mut f = Fold {
            confirmed: Some(a),
            ..Default::default()
        };
        let st = status(&f, [Some(a), None], 0, 1);
        assert_eq!(st.slots[0].state, SlotState::Confirmed);
        assert_eq!((st.slots[1].state, st.confirmed), (SlotState::Empty, 0));

        f.pending = Some(b);
        f.attempts = 1;
        let st = status(&f, [Some(a), Some(b)], 1, 1);
        assert_eq!(st.slots[1].state, SlotState::Trying);
        assert_eq!((st.slots[0].state, st.confirmed), (SlotState::Confirmed, 0));

        // A new image written over a reverted one is just "written".
        f.pending = None;
        f.reverted = Some(b);
        let b2 = mark(1, 2);
        let st = status(&f, [Some(a), Some(b2)], 0, 1);
        assert_eq!(st.slots[1].state, SlotState::Written);
        let st = status(&f, [Some(a), Some(b)], 0, 1);
        assert_eq!(st.slots[1].state, SlotState::Reverted);
    }
}
