//! APP state log: a pure fold of records into [`LogState`], plus loading the
//! newest valid face from flash and appending records to it.
//!
//! When the face is full or ends in a record failing its CRC (a write cut by
//! power loss), the next append first compacts: it erases the other face,
//! copies the folded state there (ROLLBACK for the last reason, SLOT_PENDING,
//! SNAPSHOT carrying the confirmed slot and the attempts) and writes that
//! face's header with `generation + 1` last. Until that header is written the
//! old face stays the valid one, so a cut compaction is simply redone. The
//! "reverted" marker of STATUS is not carried over.

use dagon_format::statelog::{
    kind, reason, slot, target, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE,
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
            // SNAPSHOT (compaction): the confirmed slot, with `reason`
            // holding the attempts of the pending slot copied before it.
            kind::SNAPSHOT => LogState {
                confirmed: Some(this),
                attempts: if self.pending.is_some() { r.reason } else { 0 },
                reverted,
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
    /// Even a freshly compacted face cannot take the record (face too small).
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
    layout: LogLayout,
    /// Index of the active face in `layout.faces`.
    face: usize,
    /// Address of the next erased record, `None` when the face must be
    /// compacted before the next append.
    next: Option<u32>,
    next_seq: u32,
}

impl Log {
    /// Picks the face with a valid header and the largest generation, then
    /// folds its records up to the first erased one. A record failing its CRC
    /// ends the scan.
    pub fn load<F: Flash>(flash: &mut F, layout: &LogLayout) -> Result<Log, LogError> {
        let mut best: Option<(usize, u32)> = None;
        for (i, &face) in layout.faces.iter().enumerate() {
            let mut h = [0u8; FACE_HEADER_SIZE];
            flash.read(face, &mut h)?;
            if let Ok(fh) = FaceHeader::parse(&h) {
                if best.is_none_or(|(_, g)| fh.generation > g) {
                    best = Some((i, fh.generation));
                }
            }
        }
        let (face, generation) = best.ok_or(LogError::NoValidFace)?;
        let mut log = Log {
            state: LogState::default(),
            generation,
            layout: *layout,
            face,
            next: None,
            next_seq: 1,
        };
        let face_end = log.face_end();
        let mut addr = layout.faces[face] + FACE_HEADER_SIZE as u32;
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

    /// Appends one APP record and folds it into `state`, compacting into the
    /// other face first when this one cannot take it.
    pub fn append<F: Flash>(
        &mut self,
        flash: &mut F,
        kind: u8,
        at: SlotRef,
        reason: u8,
    ) -> Result<(), LogError> {
        if self.next.is_none() {
            self.compact(flash)?;
        }
        let addr = self.next.ok_or(LogError::Full)?;
        let r = self.record(kind, at, reason);
        flash.write(addr, &r.to_bytes())?;
        self.state = self.state.apply(&r);
        self.advance(addr);
        Ok(())
    }

    fn face_end(&self) -> u32 {
        self.layout.faces[self.face] + self.layout.face_len
    }

    fn record(&self, kind: u8, at: SlotRef, reason: u8) -> Record {
        Record {
            kind,
            target: target::APP,
            slot: at.slot,
            reason,
            seq: self.next_seq,
            version: at.version,
            digest16: at.digest16,
        }
    }

    /// Moves past the record just written at `addr`.
    fn advance(&mut self, addr: u32) {
        self.next_seq = self.next_seq.wrapping_add(1);
        let after = addr + RECORD_SIZE as u32;
        self.next = (after + RECORD_SIZE as u32 <= self.face_end()).then_some(after);
    }

    /// Copies the folded state into the other face and makes it active. On
    /// any error the log stays on the current face, so a retry never erases
    /// the only valid face.
    fn compact<F: Flash>(&mut self, flash: &mut F) -> Result<(), LogError> {
        let old = *self;
        let result = self.copy_to_other_face(flash);
        if result.is_err() {
            *self = old;
        }
        result
    }

    fn copy_to_other_face<F: Flash>(&mut self, flash: &mut F) -> Result<(), LogError> {
        let s = self.state;
        let carry = [
            s.confirmed
                .filter(|_| s.last_reason != reason::NONE)
                .map(|c| (kind::ROLLBACK, c, s.last_reason)),
            s.pending.map(|p| (kind::SLOT_PENDING, p, reason::NONE)),
            s.confirmed.map(|c| (kind::SNAPSHOT, c, s.attempts)),
        ];
        let generation = self.generation.wrapping_add(1);
        self.face ^= 1;
        let face = self.layout.faces[self.face];
        flash.erase(face, self.layout.face_len)?;
        self.next = Some(face + FACE_HEADER_SIZE as u32);
        self.state = LogState::default();
        for (kind, at, why) in carry.into_iter().flatten() {
            let addr = self.next.ok_or(LogError::Full)?;
            let r = self.record(kind, at, why);
            flash.write(addr, &r.to_bytes())?;
            self.state = self.state.apply(&r);
            self.advance(addr);
        }
        flash.write(face, &FaceHeader { generation }.to_bytes())?;
        self.generation = generation;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

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

    /// Two 96-byte faces at 0 and 96, erased.
    struct Tiny(std::vec::Vec<u8>);

    impl Flash for Tiny {
        fn geometry(&self) -> &[dagon_port::FlashRegion] {
            &[]
        }
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            buf.copy_from_slice(&self.0[addr as usize..][..buf.len()]);
            Ok(())
        }
        fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
            self.0[addr as usize..][..len as usize].fill(0xFF);
            Ok(())
        }
        fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
            self.0[addr as usize..][..data.len()].copy_from_slice(data);
            Ok(())
        }
    }

    #[test]
    fn compaction_that_does_not_fit_keeps_the_active_face() {
        let layout = LogLayout {
            faces: [0, 96],
            face_len: 96,
        };
        let mut flash = Tiny(std::vec![0xFF; 192]);
        flash
            .write(0, &FaceHeader { generation: 1 }.to_bytes())
            .unwrap();
        let a = SlotRef::of(&rec(kind::CONFIRMED, 0));
        let b = SlotRef::of(&rec(kind::SLOT_PENDING, 1));
        // Three records to carry (ROLLBACK, SLOT_PENDING, SNAPSHOT), room
        // for two: the copy runs out of space.
        let mut log = Log {
            state: LogState {
                confirmed: Some(a),
                pending: Some(b),
                last_reason: reason::TRIAL_EXHAUSTED,
                ..LogState::default()
            },
            generation: 1,
            layout,
            face: 0,
            next: None,
            next_seq: 5,
        };
        let before = log;
        for _ in 0..2 {
            assert_eq!(
                log.append(&mut flash, kind::BOOT_ATTEMPT, b, reason::NONE),
                Err(LogError::Full)
            );
            assert_eq!(log, before);
            // Face 0 is never the one erased.
            assert_eq!(&flash.0[..32], &FaceHeader { generation: 1 }.to_bytes()[..]);
        }
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
    fn snapshot_carries_confirmed_and_attempts() {
        let s = fold(&[
            rec(kind::SLOT_PENDING, 1),
            Record {
                reason: 2,
                ..rec(kind::SNAPSHOT, 0)
            },
        ]);
        assert_eq!(s.confirmed.map(|c| c.slot), Some(0));
        assert_eq!((s.attempts, s.active()), (2, 1));
        let s = fold(&[Record {
            reason: 2,
            ..rec(kind::SNAPSHOT, 0)
        }]);
        assert_eq!((s.attempts, s.active()), (0, 0));
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
