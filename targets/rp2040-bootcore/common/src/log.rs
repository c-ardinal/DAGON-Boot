//! State log on two ping-pong faces: pick the valid face with the larger
//! generation, fold its records, append after the last record.

use dagon_format::statelog::{kind, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE};
use dagon_format::Version;
use dagon_port::{Flash, FlashError};

use crate::layout::LOG_FACE_SIZE;

/// The image a record refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub slot: u8,
    pub version: Version,
    pub digest16: [u8; 16],
}

impl Mark {
    fn of(r: &Record) -> Self {
        Mark {
            slot: r.slot,
            version: r.version,
            digest16: r.digest16,
        }
    }
}

/// What the records of one target say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fold {
    pub confirmed: Option<Mark>,
    /// Selected for trial, not yet confirmed or rolled back.
    pub pending: Option<Mark>,
    /// BOOT_ATTEMPT records for `pending`.
    pub attempts: u8,
    pub reverted: Option<Mark>,
    pub last_reason: u8,
}

impl Fold {
    pub fn apply(&mut self, r: &Record) {
        let m = Mark::of(r);
        match r.kind {
            kind::PROVISION | kind::SNAPSHOT => self.confirmed = Some(m),
            kind::SLOT_PENDING => {
                self.pending = Some(m);
                self.attempts = 0;
            }
            kind::BOOT_ATTEMPT if self.pending.is_some_and(|p| p.slot == m.slot) => {
                self.attempts = self.attempts.saturating_add(1);
            }
            kind::CONFIRMED => {
                self.confirmed = Some(m);
                self.pending = None;
                self.attempts = 0;
                if self.reverted.is_some_and(|x| x.slot == m.slot) {
                    self.reverted = None;
                }
            }
            kind::ROLLBACK => {
                self.reverted = Some(m);
                self.pending = None;
                self.attempts = 0;
                self.last_reason = r.reason;
            }
            _ => {}
        }
    }
}

/// Where the next record goes. `next == None`: the face cannot take more
/// records (full, or ends in a torn record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub generation: u32,
    next: Option<u32>,
    last_seq: u32,
}

/// Folds the records of `target`. `Ok((fold, None))` when neither face is valid.
pub fn read<F: Flash>(
    flash: &mut F,
    faces: [u32; 2],
    target: u8,
) -> Result<(Fold, Option<Cursor>), FlashError> {
    let mut fold = Fold::default();
    let Some((face, generation)) = pick_face(flash, faces)? else {
        return Ok((fold, None));
    };
    let mut cur = Cursor {
        generation,
        next: None,
        last_seq: 0,
    };
    let end = face + LOG_FACE_SIZE;
    let mut at = face + FACE_HEADER_SIZE as u32;
    while at < end {
        let mut b = [0u8; RECORD_SIZE];
        flash.read(at, &mut b)?;
        if Record::is_erased(&b) {
            cur.next = Some(at);
            break;
        }
        // ponytail: a corrupt record ends the scan and blocks appends; no
        // compaction to the other face yet.
        let Ok(r) = Record::parse(&b) else { break };
        cur.last_seq = cur.last_seq.max(r.seq);
        if r.target == target {
            fold.apply(&r);
        }
        at += RECORD_SIZE as u32;
    }
    Ok((fold, Some(cur)))
}

fn pick_face<F: Flash>(flash: &mut F, faces: [u32; 2]) -> Result<Option<(u32, u32)>, FlashError> {
    let mut best: Option<(u32, u32)> = None;
    for face in faces {
        let mut b = [0u8; FACE_HEADER_SIZE];
        flash.read(face, &mut b)?;
        if let Ok(h) = FaceHeader::parse(&b) {
            if best.is_none_or(|(_, g)| h.generation > g) {
                best = Some((face, h.generation));
            }
        }
    }
    Ok(best)
}

/// Appends one record (its `seq` is assigned here).
pub fn append<F: Flash>(
    flash: &mut F,
    faces: [u32; 2],
    kind: u8,
    target: u8,
    mark: Mark,
    reason: u8,
) -> Result<(), FlashError> {
    let (_, cur) = read(flash, faces, target)?;
    let cur = cur.ok_or(FlashError::Range)?;
    let at = cur.next.ok_or(FlashError::Range)?;
    let r = Record {
        kind,
        target,
        slot: mark.slot,
        reason,
        seq: cur.last_seq.wrapping_add(1),
        version: mark.version,
        digest16: mark.digest16,
    };
    flash.write(at, &r.to_bytes())
}

/// Bytes of a fresh face: header (generation `generation`) then `records`,
/// the rest erased. Used to build the initial log of an install image.
pub fn face_bytes(generation: u32, records: &[Record], out: &mut [u8]) {
    out.fill(0xFF);
    out[..FACE_HEADER_SIZE].copy_from_slice(&FaceHeader { generation }.to_bytes());
    for (i, r) in records.iter().enumerate() {
        let at = FACE_HEADER_SIZE + i * RECORD_SIZE;
        out[at..at + RECORD_SIZE].copy_from_slice(&r.to_bytes());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use super::*;
    use crate::layout::APP_LOG;
    use dagon_format::statelog::{reason, target};
    use dagon_port::FlashRegion;
    use std::vec;
    use std::vec::Vec;

    pub struct Mem(pub Vec<u8>);

    impl Mem {
        pub fn new() -> Self {
            Mem(vec![0xFF; 0x8000])
        }
    }

    impl Flash for Mem {
        fn geometry(&self) -> &[FlashRegion] {
            &[]
        }
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            let a = addr as usize;
            buf.copy_from_slice(self.0.get(a..a + buf.len()).ok_or(FlashError::Range)?);
            Ok(())
        }
        fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
            self.0[addr as usize..(addr + len) as usize].fill(0xFF);
            Ok(())
        }
        fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
            let dst = &mut self.0[addr as usize..addr as usize + data.len()];
            if dst.iter().any(|&b| b != 0xFF) {
                return Err(FlashError::Hw);
            }
            dst.copy_from_slice(data);
            Ok(())
        }
    }

    pub fn mark(slot: u8, minor: u8) -> Mark {
        Mark {
            slot,
            version: Version::new(1, minor, 0),
            digest16: [slot * 16 + minor; 16],
        }
    }

    /// Log provisioned with A (1.0.0) as confirmed.
    pub fn provisioned() -> Mem {
        let mut m = Mem::new();
        let a = mark(0, 0);
        let rec = Record {
            kind: kind::PROVISION,
            target: target::APP,
            slot: 0,
            reason: 0,
            seq: 1,
            version: a.version,
            digest16: a.digest16,
        };
        face_bytes(1, &[rec], &mut m.0[APP_LOG[0] as usize..][..4096]);
        m
    }

    fn add(m: &mut Mem, k: u8, mk: Mark, why: u8) {
        append(m, APP_LOG, k, target::APP, mk, why).unwrap();
    }

    fn fold(m: &mut Mem) -> Fold {
        read(m, APP_LOG, target::APP).unwrap().0
    }

    #[test]
    fn update_trial_confirm_and_return_to_previous() {
        let mut m = provisioned();
        assert_eq!(fold(&mut m).confirmed, Some(mark(0, 0)));
        add(&mut m, kind::SLOT_PENDING, mark(1, 1), 0);
        add(&mut m, kind::BOOT_ATTEMPT, mark(1, 1), 0);
        let f = fold(&mut m);
        assert_eq!((f.pending, f.attempts), (Some(mark(1, 1)), 1));
        add(&mut m, kind::CONFIRMED, mark(1, 1), 0);
        let f = fold(&mut m);
        assert_eq!((f.confirmed, f.pending), (Some(mark(1, 1)), None));
        // Back to the previous version in A.
        add(&mut m, kind::SLOT_PENDING, mark(0, 0), 0);
        assert_eq!(fold(&mut m).pending, Some(mark(0, 0)));
    }

    #[test]
    fn rollback_after_trials() {
        let mut m = provisioned();
        add(&mut m, kind::SLOT_PENDING, mark(1, 1), 0);
        for _ in 0..3 {
            add(&mut m, kind::BOOT_ATTEMPT, mark(1, 1), 0);
        }
        assert_eq!(fold(&mut m).attempts, 3);
        add(&mut m, kind::ROLLBACK, mark(1, 1), reason::TRIAL_EXHAUSTED);
        let f = fold(&mut m);
        assert_eq!(f.confirmed, Some(mark(0, 0)));
        assert_eq!((f.pending, f.reverted), (None, Some(mark(1, 1))));
        assert_eq!(f.last_reason, reason::TRIAL_EXHAUSTED);
    }

    #[test]
    fn newer_face_wins_and_empty_log_has_no_cursor() {
        let mut m = Mem::new();
        assert_eq!(read(&mut m, APP_LOG, 0).unwrap(), (Fold::default(), None));
        assert!(append(&mut m, APP_LOG, kind::CONFIRMED, 0, mark(0, 0), 0).is_err());
        let mut m = provisioned();
        let b = mark(1, 5);
        let rec = Record {
            kind: kind::SNAPSHOT,
            target: target::APP,
            slot: 1,
            reason: 0,
            seq: 9,
            version: b.version,
            digest16: b.digest16,
        };
        face_bytes(2, &[rec], &mut m.0[APP_LOG[1] as usize..][..4096]);
        let (f, cur) = read(&mut m, APP_LOG, target::APP).unwrap();
        assert_eq!((f.confirmed, cur.unwrap().generation), (Some(b), 2));
    }

    #[test]
    fn other_target_is_ignored() {
        let mut m = provisioned();
        append(
            &mut m,
            APP_LOG,
            kind::SLOT_PENDING,
            target::BOOTCORE,
            mark(1, 1),
            0,
        )
        .unwrap();
        assert_eq!(fold(&mut m).pending, None);
    }
}
