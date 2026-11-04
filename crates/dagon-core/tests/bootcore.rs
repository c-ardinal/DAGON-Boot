//! Boot core A/B: stage0's plan from the boot core records and the trial
//! cell, what a boot core records about itself at start, and one log
//! face pair holding both targets through compaction.

use dagon_core::bootcore::{plan, read_state, settle, trial_cell, tried, Plan, Settle};
use dagon_core::log::{Log, LogLayout, LogState, SlotRef};
use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
use dagon_format::Version;
use dagon_port::{Flash, FlashError, FlashRegion};

const MAX: u8 = 3;

fn r(slot: u8, minor: u8) -> SlotRef {
    SlotRef {
        slot,
        version: Version::new(1, minor, 0),
        digest16: [minor ^ (slot << 4); 16],
    }
}

fn rec(k: u8, t: u8, at: SlotRef, why: u8) -> Record {
    Record {
        kind: k,
        target: t,
        slot: at.slot,
        reason: why,
        seq: 0,
        version: at.version,
        digest16: at.digest16,
    }
}

fn core_fold(rs: &[Record]) -> LogState {
    rs.iter().fold(LogState::default(), |s, r| s.apply_core(r))
}

#[test]
fn core_records_fold_apart_from_app_records() {
    let (a, b) = (r(0, 0), r(1, 1));
    let rs = [
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::PROVISION, target::APP, b, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
    ];
    let core = core_fold(&rs);
    assert_eq!((core.confirmed, core.pending), (Some(a), Some(b)));
    let app = rs.iter().fold(LogState::default(), |s, r| s.apply(r));
    assert_eq!((app.confirmed, app.pending), (Some(b), None));
}

#[test]
fn stage0_tries_the_pending_core_until_the_attempts_run_out() {
    let (a, b) = (r(0, 0), r(1, 1));
    let st = core_fold(&[
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
    ]);
    for n in 0..MAX {
        assert_eq!(
            plan(&st, n, MAX),
            Plan::Try {
                pending: b,
                attempt: n + 1,
                fallback: Some(a)
            }
        );
    }
    assert_eq!(plan(&st, MAX, MAX), Plan::Run(Some(a)));
    // Nothing pending: the confirmed core; an empty log: slot A unchecked.
    let only_a = core_fold(&[rec(kind::PROVISION, target::BOOTCORE, a, 0)]);
    assert_eq!(plan(&only_a, 0, MAX), Plan::Run(Some(a)));
    assert_eq!(plan(&LogState::default(), 0, MAX), Plan::Run(None));
}

#[test]
fn the_trial_cell_counts_only_for_the_image_it_names() {
    let (b, other) = (r(1, 1), r(1, 2));
    for n in 0..=MAX {
        assert_eq!(tried(trial_cell(&b, n), &b), n);
    }
    assert_eq!(tried(trial_cell(&other, 2), &b), 0, "another image");
    assert_eq!(tried([0, 0], &b), 0, "cleared");
    let [w, c] = trial_cell(&b, 2);
    assert_eq!(tried([w, c ^ 1], &b), 0, "check word wrong");
    assert_eq!(
        tried([w ^ 0x100, !(w ^ 0x100)], &b),
        0,
        "digest bits differ"
    );
}

/// Retained memory holds arbitrary values after power-up: no value but the
/// cell written for that image reads as a count for it.
#[test]
fn random_retained_words_never_read_as_a_count() {
    let b = r(1, 1);
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..1_000_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let cell = [x as u32, (x >> 32) as u32];
        if cell[1] == !cell[0] {
            continue;
        }
        assert_eq!(tried(cell, &b), 0, "{cell:08x?}");
    }
}

#[test]
fn a_started_core_confirms_itself_or_records_the_rollback() {
    let (a, b) = (r(0, 0), r(1, 1));
    let st = core_fold(&[
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
    ]);
    assert_eq!(settle(&st, 1, 1, MAX), Settle::Confirm(b));
    // The old core runs while B is pending: stage0 gave up on B.
    assert_eq!(
        settle(&st, 0, MAX, MAX),
        Settle::Rollback {
            back: SlotRef { slot: 0, ..b },
            reason: reason::TRIAL_EXHAUSTED
        }
    );
    assert_eq!(
        settle(&st, 0, 0, MAX),
        Settle::Rollback {
            back: SlotRef { slot: 0, ..b },
            reason: reason::HASH_MISMATCH
        },
        "never tried: B failed stage0's check"
    );
    let done = core_fold(&[
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
        rec(kind::CONFIRMED, target::BOOTCORE, b, 0),
    ]);
    assert_eq!(settle(&done, 1, 0, MAX), Settle::Nothing);
    assert_eq!(done.confirmed, Some(b));
    let back = core_fold(&[
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
        rec(
            kind::ROLLBACK,
            target::BOOTCORE,
            SlotRef { slot: 0, ..b },
            3,
        ),
    ]);
    assert_eq!(
        (back.confirmed, back.pending, back.reverted),
        (Some(a), None, Some(b))
    );
}

struct Mem(Vec<u8>);

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
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

/// Stage0's read-only fold reads what `Log` writes, through compactions.
#[test]
fn the_read_only_fold_equals_the_logs() {
    let layout = LogLayout {
        faces: [0, 0x400],
        face_len: 0x400,
    };
    let mut m = Mem(vec![0xFF; 0x800]);
    m.write(0, &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let mut log = Log::load(&mut m, &layout).unwrap();
    let mut x = 7u32;
    for i in 0..400u32 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let at = r((x >> 16) as u8 & 1, (i % 200) as u8);
        let k = [
            kind::PROVISION,
            kind::SLOT_PENDING,
            kind::CONFIRMED,
            kind::ROLLBACK,
        ][(x >> 24) as usize % 4];
        if x & 1 == 0 {
            log.append_core(&mut m, k, at, (x % 4) as u8).unwrap();
        } else {
            log.append(&mut m, k, at, 0).unwrap();
        }
        let want = Log::load(&mut m, &layout).unwrap().core;
        assert_eq!(read_state(&mut m, &layout), Some(want), "{i}");
    }
    assert!(log.generation > 1);
}

/// One face pair for both targets (a small MCU): compaction carries the
/// boot core state as well as the app state.
#[test]
fn one_log_holds_both_targets_through_compaction() {
    let layout = LogLayout {
        faces: [0, 0x200],
        face_len: 0x200,
    };
    let mut m = Mem(vec![0xFF; 0x400]);
    m.write(0, &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let (ca, cb, aa, ab) = (r(0, 0), r(1, 1), r(0, 5), r(1, 6));
    let mut log = Log::load(&mut m, &layout).unwrap();
    log.append_core(&mut m, kind::PROVISION, ca, 0).unwrap();
    log.append(&mut m, kind::PROVISION, aa, 0).unwrap();
    for i in 0..40u32 {
        let to = if i % 2 == 0 { ab } else { aa };
        log.append(&mut m, kind::SLOT_PENDING, to, 0).unwrap();
        log.append(&mut m, kind::CONFIRMED, to, 0).unwrap();
    }
    log.append_core(&mut m, kind::SLOT_PENDING, cb, 0).unwrap();
    for _ in 0..20 {
        log.append(&mut m, kind::SLOT_PENDING, ab, 0).unwrap();
    }
    let back = Log::load(&mut m, &layout).unwrap();
    assert!(back.generation > 1, "the faces compacted");
    assert_eq!(
        (back.core.confirmed, back.core.pending),
        (Some(ca), Some(cb))
    );
    assert_eq!(back.state.pending, Some(ab));
    assert_eq!(back.state.confirmed, Some(aa));
    assert_eq!(back, log);
}
