//! APP state log: a pure fold of records into [`LogState`], plus loading the
//! newest valid face from flash and appending records to it.
//!
//! Not handled yet: compaction into the other face when a face fills up or
//! ends in a torn record. Such a face stays readable but refuses appends
//! ([`LogError::Full`]).

use dagon_format::statelog::{
    kind, slot, target, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE,
};
use dagon_format::Version;
use dagon_port::{Flash, FlashError};

/// The image a record refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRef {
    pub slot: u8,
    pub version: Version,
    /// First 16 bytes of the SHA-256 of the image header.
    pub digest16: [u8; 16],
}

impl SlotRef {
    fn of(r: &Record) -> Self {
        SlotRef {
            slot: r.slot,
            version: r.version,
            digest16: r.digest16,
        }
    }
}

/// APP boot state folded from the log, oldest record first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogState {
    pub confirmed: Option<SlotRef>,
    /// Selected for trial and not yet confirmed or rolled back.
    pub pending: Option<SlotRef>,
    /// BOOT_ATTEMPT records written for `pending`.
    pub attempts: u8,
    /// Slot whose image was last rolled back.
    pub reverted: Option<u8>,
    /// Reason of the last rollback (0 none).
    pub last_reason: u8,
}

impl LogState {
    /// Applies one record. Records for other targets and kinds this release
    /// does not write are ignored.
    pub fn apply(self, r: &Record) -> Self {
        if r.target != target::APP {
            return self;
        }
        let this = SlotRef::of(r);
        let reverted = self.reverted.filter(|&s| s != r.slot);
        match r.kind {
            kind::PROVISION => LogState {
                confirmed: Some(this),
                ..LogState::default()
            },
            kind::CONFIRMED => LogState {
                confirmed: Some(this),
                pending: None,
                attempts: 0,
                reverted,
                ..self
            },
            kind::SLOT_PENDING => LogState {
                pending: Some(this),
                attempts: 0,
                reverted,
                ..self
            },
            kind::BOOT_ATTEMPT if self.pending.map(|p| p.slot) == Some(r.slot) => LogState {
                attempts: self.attempts.saturating_add(1),
                ..self
            },
            // ROLLBACK: `slot` is where the boot goes back to; the pending
            // image is the one reverted.
            kind::ROLLBACK => LogState {
                pending: None,
                attempts: 0,
                reverted: self.pending.map(|p| p.slot),
                last_reason: r.reason,
                ..self
            },
            _ => self,
        }
    }

    /// True once the pending slot has been started at least once.
    pub fn on_trial(&self) -> bool {
        self.pending.is_some() && self.attempts > 0
    }

    /// Slot running now: the pending slot while on trial, else the confirmed one.
    pub fn active(&self) -> u8 {
        match (self.on_trial(), self.pending, self.confirmed) {
            (true, Some(p), _) => p.slot,
            (_, _, Some(c)) => c.slot,
            _ => slot::NONE,
        }
    }
}

/// Where the two log faces live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogLayout {
    pub faces: [u32; 2],
    pub face_len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogError {
    Flash(FlashError),
    /// Neither face has a valid header.
    NoValidFace,
    /// The face cannot take another record (full, or ends in a torn record).
    Full,
}

impl From<FlashError> for LogError {
    fn from(e: FlashError) -> Self {
        LogError::Flash(e)
    }
}

/// The active log face and its folded state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Log {
    pub state: LogState,
    pub generation: u32,
    face_end: u32,
    /// Address of the next erased record, `None` when appends are refused.
    next: Option<u32>,
    next_seq: u32,
}

impl Log {
    /// Picks the face with a valid header and the largest generation, then
    /// folds its records up to the first erased one. A record failing its CRC
    /// ends the scan.
    pub fn load<F: Flash>(flash: &mut F, layout: &LogLayout) -> Result<Log, LogError> {
        let mut best: Option<(u32, u32)> = None;
        for &face in &layout.faces {
            let mut h = [0u8; FACE_HEADER_SIZE];
            flash.read(face, &mut h)?;
            if let Ok(fh) = FaceHeader::parse(&h) {
                if best.is_none_or(|(_, g)| fh.generation > g) {
                    best = Some((face, fh.generation));
                }
            }
        }
        let (face, generation) = best.ok_or(LogError::NoValidFace)?;
        let face_end = face + layout.face_len;
        let mut log = Log {
            state: LogState::default(),
            generation,
            face_end,
            next: None,
            next_seq: 1,
        };
        let mut addr = face + FACE_HEADER_SIZE as u32;
        while addr + RECORD_SIZE as u32 <= face_end {
            let mut b = [0u8; RECORD_SIZE];
            flash.read(addr, &mut b)?;
            if Record::is_erased(&b) {
                log.next = Some(addr);
                break;
            }
            let Ok(r) = Record::parse(&b) else { break };
            log.state = log.state.apply(&r);
            log.next_seq = log.next_seq.max(r.seq.wrapping_add(1));
            addr += RECORD_SIZE as u32;
        }
        Ok(log)
    }

    /// Appends one APP record and folds it into `state`.
    pub fn append<F: Flash>(
        &mut self,
        flash: &mut F,
        kind: u8,
        at: SlotRef,
        reason: u8,
    ) -> Result<(), LogError> {
        let addr = self.next.ok_or(LogError::Full)?;
        let r = Record {
            kind,
            target: target::APP,
            slot: at.slot,
            reason,
            seq: self.next_seq,
            version: at.version,
            digest16: at.digest16,
        };
        flash.write(addr, &r.to_bytes())?;
        self.state = self.state.apply(&r);
        self.next_seq = self.next_seq.wrapping_add(1);
        let after = addr + RECORD_SIZE as u32;
        self.next = (after + RECORD_SIZE as u32 <= self.face_end).then_some(after);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_format::statelog::reason;

    fn rec(kind: u8, slot: u8) -> Record {
        Record {
            kind,
            target: target::APP,
            slot,
            reason: reason::NONE,
            seq: 0,
            version: Version::new(1, 0, slot as u16),
            digest16: [slot; 16],
        }
    }

    fn fold(rs: &[Record]) -> LogState {
        rs.iter().fold(LogState::default(), |s, r| s.apply(r))
    }

    #[test]
    fn trial_then_rollback() {
        let s = fold(&[
            rec(kind::PROVISION, 0),
            rec(kind::CONFIRMED, 0),
            rec(kind::SLOT_PENDING, 1),
            rec(kind::BOOT_ATTEMPT, 1),
            rec(kind::BOOT_ATTEMPT, 1),
        ]);
        assert_eq!((s.attempts, s.active(), s.on_trial()), (2, 1, true));
        let s = s.apply(&Record {
            reason: reason::TRIAL_EXHAUSTED,
            ..rec(kind::ROLLBACK, 0)
        });
        assert_eq!(s.pending, None);
        assert_eq!(s.active(), 0);
        assert_eq!(s.reverted, Some(1));
        assert_eq!(s.last_reason, reason::TRIAL_EXHAUSTED);
    }

    #[test]
    fn confirm_moves_confirmed_slot() {
        let s = fold(&[
            rec(kind::PROVISION, 0),
            rec(kind::SLOT_PENDING, 1),
            rec(kind::BOOT_ATTEMPT, 1),
            rec(kind::CONFIRMED, 1),
        ]);
        assert_eq!(s.confirmed.map(|c| c.slot), Some(1));
        assert_eq!((s.pending, s.attempts, s.active()), (None, 0, 1));
    }

    #[test]
    fn pending_not_started_runs_confirmed() {
        let s = fold(&[rec(kind::PROVISION, 0), rec(kind::SLOT_PENDING, 1)]);
        assert!(!s.on_trial());
        assert_eq!(s.active(), 0);
    }

    #[test]
    fn other_targets_ignored() {
        let s = fold(&[Record {
            target: target::BOOTCORE,
            ..rec(kind::PROVISION, 0)
        }]);
        assert_eq!(s, LogState::default());
        assert_eq!(s.active(), slot::NONE);
    }
}
