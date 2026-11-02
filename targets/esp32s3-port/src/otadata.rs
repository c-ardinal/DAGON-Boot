//! otadata: what the selected entry says about its slot in STATUS terms, and
//! the encoding of a new trial entry.
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
        raw::VALID | raw::UNDEFINED if is_running => SlotState::Confirmed,
        // Selected and final but not booted: its image failed to load, the
        // 2nd stage fell back. Only the header can tell what is there.
        _ => return None,
    };
    Some(SelectedView {
        slot: selected,
        state: slot_state,
        reason: (slot_state == SlotState::Reverted).then_some(reason::TRIAL_EXHAUSTED),
    })
}

/// One otadata entry: `ota_seq` u32, `seq_label` [20], `ota_state` u32, `crc` u32.
pub const ENTRY_LEN: usize = 32;
/// Entry `i` lives at the start of otadata sector `i`.
pub const SECTOR_LEN: u32 = 0x1000;
const OTA_SLOT_COUNT: u32 = 2;

/// CRC of an entry: the 2nd stage's `esp_rom_crc32_le(UINT32_MAX, &ota_seq, 4)`
/// (reflected 0x04C11DB7, init 0, final xor 0xFFFFFFFF).
pub fn seq_crc(seq: u32) -> u32 {
    let mut crc = 0u32;
    for b in seq.to_le_bytes() {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
        }
    }
    !crc
}

fn u32_at(e: &[u8; ENTRY_LEN], at: usize) -> u32 {
    u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]])
}

/// `ota_seq` of an entry the 2nd stage counts (state is not considered).
fn valid_seq(e: &[u8; ENTRY_LEN]) -> Option<u32> {
    let seq = u32_at(e, 0);
    (seq != u32::MAX && u32_at(e, 28) == seq_crc(seq)).then_some(seq)
}

/// A counted entry: its `ota_seq`, the OTA slot it selects and its raw
/// `ota_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counted {
    pub seq: u32,
    pub slot: u8,
    pub state: u32,
}

impl Counted {
    pub fn of(e: &[u8; ENTRY_LEN]) -> Option<Self> {
        let seq = valid_seq(e)?;
        Some(Self {
            seq,
            slot: ((seq - 1) % OTA_SLOT_COUNT) as u8,
            state: u32_at(e, 24),
        })
    }

    /// Not Aborted/Invalid: the 2nd stage may still boot it.
    pub fn bootable(&self) -> bool {
        !matches!(self.state, raw::ABORTED | raw::INVALID)
    }
}

/// The entry that selects OTA slot `ota_index` for one trial (state New), and
/// the sector it goes to. Written whole in one program after erasing that
/// sector, so a power cut leaves either the old selection (erased entry,
/// ignored) or the complete new one; never a new seq with an old state.
///
/// Seq and CRC follow ESP-IDF's `esp_rewrite_ota_data` (the 2nd stage boots
/// ota_((seq - 1) % 2) from the highest counted seq), with two differences
/// that keep the fallback alive: the seq is always above every counted seq,
/// and an entry the 2nd stage could still boot is only overwritten when the
/// other one is not bootable anyway. When both are bootable, a New entry
/// (selected but never booted) goes first, then the one not selecting
/// `running` (the booted slot, 2 = factory), so the running app's own entry
/// survives repeated calls without a reboot.
///
/// `None` when no seq above the counted ones is left (`u32::MAX` means
/// "erased" to the 2nd stage).
pub fn trial_entry(
    current: &[[u8; ENTRY_LEN]; 2],
    ota_index: u8,
    running: u8,
) -> Option<(usize, [u8; ENTRY_LEN])> {
    let id = u32::from(ota_index);
    let seqs = [valid_seq(&current[0]), valid_seq(&current[1])];
    let boot = [&current[0], &current[1]].map(|e| Counted::of(e).filter(Counted::bootable));
    let sector = match boot {
        [Some(a), Some(b)] => {
            let new = [a.state == raw::NEW, b.state == raw::NEW];
            let other = [a.slot != running, b.slot != running];
            if new[0] != new[1] {
                usize::from(new[1])
            } else if other[0] != other[1] {
                usize::from(other[1])
            } else {
                usize::from(a.seq > b.seq) // the older one
            }
        }
        [Some(_), None] => 1,
        _ => 0,
    };
    let seq = match seqs[0].max(seqs[1]) {
        None => id + 1,
        // Smallest seq above `top` that maps to `id`.
        Some(top) => {
            let base = top - top % OTA_SLOT_COUNT + (id + 1) % OTA_SLOT_COUNT;
            if base > top {
                base
            } else {
                base.checked_add(OTA_SLOT_COUNT)?
            }
        }
    };
    if seq == u32::MAX {
        return None;
    }
    let mut e = current[sector];
    e[0..4].copy_from_slice(&seq.to_le_bytes());
    e[24..28].copy_from_slice(&raw::NEW.to_le_bytes());
    e[28..32].copy_from_slice(&seq_crc(seq).to_le_bytes());
    Some((sector, e))
}

/// Whether otadata, as the 2nd stage counts it, selects OTA slot `ota_index`
/// for a trial through the entry in `sector`: that entry is counted, New and
/// above every other counted seq. An entry that does not count (bad CRC,
/// erased) is ignored, as the 2nd stage does.
pub fn selects_trial(current: &[[u8; ENTRY_LEN]; 2], sector: usize, ota_index: u8) -> bool {
    let Some(c) = current.get(sector).and_then(Counted::of) else {
        return false;
    };
    let other = Counted::of(&current[1 - sector]);
    c.slot == ota_index && c.state == raw::NEW && other.is_none_or(|o| o.seq < c.seq)
}

/// What CONFIRM has to do to otadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmStep {
    /// Nothing: otadata does not select `running` (factory, or booted as the
    /// fallback), or the entry is already final.
    Done,
    /// Rewrite this sector with this entry (same seq, state Valid).
    Write(usize, [u8; ENTRY_LEN]),
    /// The selected entry for `running` is Aborted, Invalid or unknown.
    Refused,
}

/// CONFIRM of the running slot, counted as the 2nd stage does: the selected
/// entry is the highest counted seq; an entry that does not count (bad CRC,
/// erased) is ignored.
pub fn confirm_entry(current: &[[u8; ENTRY_LEN]; 2], running: u8) -> ConfirmStep {
    let selected = (0..2)
        .filter_map(|i| Counted::of(&current[i]).map(|c| (i, c)))
        .max_by_key(|(_, c)| c.seq);
    let Some((i, c)) = selected.filter(|(_, c)| c.slot == running) else {
        return ConfirmStep::Done;
    };
    match c.state {
        raw::VALID | raw::UNDEFINED => ConfirmStep::Done,
        raw::NEW | raw::PENDING_VERIFY => {
            let mut e = current[i];
            e[24..28].copy_from_slice(&raw::VALID.to_le_bytes());
            ConfirmStep::Write(i, e)
        }
        _ => ConfirmStep::Refused,
    }
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
    fn final_selection_that_did_not_boot_is_not_confirmed() {
        assert_eq!(view(SLOT_A, SLOT_B, raw::VALID), None);
        assert_eq!(view(SLOT_B, SLOT_A, raw::UNDEFINED), None);
    }

    // Entries from esp-bootloader-esp-idf's own otadata tests.
    const BLANK: [u8; ENTRY_LEN] = [0xFF; ENTRY_LEN];

    fn entry(seq: u32, state: u32, crc: [u8; 4]) -> [u8; ENTRY_LEN] {
        let mut e = BLANK;
        e[0..4].copy_from_slice(&seq.to_le_bytes());
        e[24..28].copy_from_slice(&state.to_le_bytes());
        e[28..32].copy_from_slice(&crc);
        e
    }

    #[test]
    fn seq_crc_matches_known_entries() {
        assert_eq!(seq_crc(1).to_le_bytes(), [154, 152, 67, 71]);
        assert_eq!(seq_crc(2).to_le_bytes(), [116, 55, 246, 85]);
        assert_eq!(seq_crc(3).to_le_bytes(), [17, 80, 74, 237]);
    }

    #[test]
    fn trial_entry_from_blank_otadata() {
        assert_eq!(
            trial_entry(&[BLANK, BLANK], 0, SLOT_FACTORY),
            Some((0, entry(1, raw::NEW, [154, 152, 67, 71])))
        );
        // ota_1 from blank gets seq 2 (never 0), like ESP-IDF.
        assert_eq!(
            trial_entry(&[BLANK, BLANK], 1, SLOT_FACTORY),
            Some((0, entry(2, raw::NEW, [116, 55, 246, 85])))
        );
    }

    #[test]
    fn trial_entry_goes_to_the_other_sector_with_next_seq() {
        let valid_a = entry(1, raw::VALID, [154, 152, 67, 71]);
        assert_eq!(
            trial_entry(&[valid_a, BLANK], 1, SLOT_A),
            Some((1, entry(2, raw::NEW, [116, 55, 246, 85])))
        );
        // Both bootable (A confirmed earlier, B confirmed now): the older
        // entry (A's) is replaced, B's stays as the fallback.
        let valid_b = entry(2, raw::VALID, [116, 55, 246, 85]);
        assert_eq!(
            trial_entry(&[valid_a, valid_b], 0, SLOT_B),
            Some((0, entry(3, raw::NEW, [17, 80, 74, 237])))
        );
    }

    fn seq_of(e: &[u8; ENTRY_LEN]) -> u32 {
        u32_at(e, 0)
    }

    #[test]
    fn trial_entry_after_rollback_keeps_the_running_entry() {
        // B was tried and aborted, A runs from sector 0.
        let valid_a = entry(1, raw::VALID, [154, 152, 67, 71]);
        let aborted_b = entry(2, raw::ABORTED, [116, 55, 246, 85]);
        // Retrying B: sector 1 (the aborted one) gets seq 4, A stays.
        let (sector, e) = trial_entry(&[valid_a, aborted_b], 1, SLOT_A).unwrap();
        assert_eq!((sector, seq_of(&e)), (1, 4));
        assert_eq!((seq_of(&e) - 1) % 2, 1);
        // Trying A again: still sector 1, seq 3.
        let (sector, e) = trial_entry(&[valid_a, aborted_b], 0, SLOT_A).unwrap();
        assert_eq!((sector, seq_of(&e)), (1, 3));
        // Mirrored: A aborted in sector 0, B runs from sector 1.
        let aborted_a = entry(3, raw::ABORTED, [17, 80, 74, 237]);
        let valid_b = entry(2, raw::VALID, [116, 55, 246, 85]);
        let (sector, e) = trial_entry(&[aborted_a, valid_b], 0, SLOT_B).unwrap();
        assert_eq!((sector, seq_of(&e)), (0, 5));
    }

    #[test]
    fn new_seq_is_always_the_highest_and_maps_to_the_slot() {
        for top in 1..20u32 {
            let current = [entry(top, raw::VALID, seq_crc(top).to_le_bytes()), BLANK];
            for id in 0..2u8 {
                let (_, e) = trial_entry(&current, id, SLOT_FACTORY).unwrap();
                let seq = seq_of(&e);
                assert!(seq > top && seq <= top + 2, "top {top} id {id} seq {seq}");
                assert_eq!((seq - 1) % 2, u32::from(id));
                assert_eq!(u32_at(&e, 28), seq_crc(seq));
                assert_eq!(u32_at(&e, 24), raw::NEW);
            }
        }
    }

    #[test]
    fn trial_entry_ignores_entries_with_bad_crc() {
        let corrupt = entry(5, raw::VALID, [0, 0, 0, 0]);
        let (sector, e) = trial_entry(&[corrupt, BLANK], 0, SLOT_FACTORY).unwrap();
        assert_eq!((sector, seq_of(&e)), (0, 1));
    }

    fn apply(current: &mut [[u8; ENTRY_LEN]; 2], slot: u8, running: u8) {
        let (sector, e) = trial_entry(current, slot, running).unwrap();
        current[sector] = e;
    }

    #[test]
    fn repeated_trial_entries_keep_the_running_valid_entry() {
        // A runs confirmed from sector 0; B selected twice without a reboot.
        let valid_a = entry(1, raw::VALID, [154, 152, 67, 71]);
        let mut cur = [valid_a, BLANK];
        apply(&mut cur, SLOT_B, SLOT_A);
        apply(&mut cur, SLOT_B, SLOT_A);
        assert_eq!(cur[0], valid_a, "running entry overwritten");
        assert!(selects_trial(&cur, 1, SLOT_B));
        // Then back to A (selecting A again): replaces the New entry for B.
        apply(&mut cur, SLOT_A, SLOT_A);
        assert_eq!(cur[0], valid_a);
        assert!(selects_trial(&cur, 1, SLOT_A));
        // Both Valid, running B: A's (not running) entry is the one replaced,
        // even though it is the newer one.
        let mut cur = [
            entry(3, raw::VALID, seq_crc(3).to_le_bytes()),
            entry(2, raw::VALID, seq_crc(2).to_le_bytes()),
        ];
        let running_b = cur[1];
        apply(&mut cur, SLOT_A, SLOT_B);
        assert_eq!(cur[1], running_b);
        assert!(selects_trial(&cur, 0, SLOT_A));
    }

    #[test]
    fn trial_entry_refuses_seq_overflow() {
        for top in [u32::MAX - 1, u32::MAX - 2] {
            let cur = [entry(top, raw::VALID, seq_crc(top).to_le_bytes()), BLANK];
            for id in 0..2u8 {
                let r = trial_entry(&cur, id, SLOT_FACTORY);
                assert!(r.is_none_or(|(_, e)| seq_of(&e) > top && seq_of(&e) != u32::MAX));
            }
        }
        let top = u32::MAX - 1;
        let cur = [entry(top, raw::VALID, seq_crc(top).to_le_bytes()), BLANK];
        assert_eq!(trial_entry(&cur, 0, SLOT_FACTORY), None);
        assert_eq!(trial_entry(&cur, 1, SLOT_FACTORY), None);
    }

    #[test]
    fn corrupt_other_sector_does_not_block_the_selection_check() {
        let corrupt = entry(9, raw::VALID, [1, 2, 3, 4]);
        let unknown = entry(9, 0x77, seq_crc(9).to_le_bytes());
        for other in [corrupt, BLANK] {
            let mut cur = [BLANK, other];
            apply(&mut cur, SLOT_B, SLOT_FACTORY);
            assert_eq!(cur[1], other, "the uncounted entry is left alone");
            assert!(selects_trial(&cur, 0, SLOT_B));
            assert!(!selects_trial(&cur, 0, SLOT_A));
            assert!(!selects_trial(&cur, 1, SLOT_B));
        }
        // Counted with an unknown state: still above it, still selected.
        let mut cur = [BLANK, unknown];
        apply(&mut cur, SLOT_A, SLOT_FACTORY);
        assert!(selects_trial(&cur, 0, SLOT_A));
    }

    #[test]
    fn confirm_rewrites_the_running_trial_entry_despite_a_corrupt_other_sector() {
        let pv_b = entry(2, raw::PENDING_VERIFY, seq_crc(2).to_le_bytes());
        let mut valid_b = pv_b;
        valid_b[24..28].copy_from_slice(&raw::VALID.to_le_bytes());
        for other in [
            BLANK,
            entry(9, raw::VALID, [1, 2, 3, 4]),
            entry(1, 0x77, seq_crc(1).to_le_bytes()),
        ] {
            assert_eq!(
                confirm_entry(&[other, pv_b], SLOT_B),
                ConfirmStep::Write(1, valid_b)
            );
        }
        assert_eq!(confirm_entry(&[BLANK, valid_b], SLOT_B), ConfirmStep::Done);
        // Booted as the fallback, or factory: nothing to confirm.
        assert_eq!(confirm_entry(&[BLANK, pv_b], SLOT_A), ConfirmStep::Done);
        assert_eq!(
            confirm_entry(&[BLANK, BLANK], SLOT_FACTORY),
            ConfirmStep::Done
        );
        let aborted = entry(2, raw::ABORTED, seq_crc(2).to_le_bytes());
        assert_eq!(
            confirm_entry(&[BLANK, aborted], SLOT_B),
            ConfirmStep::Refused
        );
    }

    #[test]
    fn blank_or_unknown_otadata_says_nothing() {
        assert_eq!(selected_view(SLOT_FACTORY, SLOT_FACTORY, None), None);
        assert_eq!(selected_view(SLOT_A, SLOT_A, None), None);
        assert_eq!(view(SLOT_A, SLOT_A, 0x7), None);
    }
}
