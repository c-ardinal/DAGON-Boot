//! Kani proofs of stage0's decision (`cargo kani -p dagon-core`).
//!
//! Stage0 is never updated in the field, so a panic in it (an index out of
//! range, an overflow, an `unwrap`) on some flash or retained content would
//! leave a node that only the ROM loader recovers. The proofs below show,
//! for every content of the flash and the trial cell, that stage0's fold
//! of the log and the assembly of its decision ([`bootcore::start`], with
//! the image checks replaced by any result they can give) do not panic,
//! and that the trial cell and the choice keep their rules:
//!
//! - the cell counts at most [`bootcore::MAX_COUNT`] starts, a cell
//!   update cut between its two stores never lowers the count, and a cell
//!   written for another image counts nothing;
//! - the trial count, the app request and the app result never read as
//!   each other, and a request cut half-way is none;
//! - stage0 tries a pending core only while its starts last, and starts
//!   no core that the log does not name, nor one in a slot the MCU
//!   definition does not give;
//! - a compaction of the log carries the whole folded state: the records
//!   it writes fold to the state they were made from.
//!
//! What is assumed: the flash reads back anything (every read returns
//! arbitrary bytes, even the same address twice); the signature verifier
//! accepts or refuses anything (its own correctness is the Ed25519 vector
//! and differential tests'); the CRC-32 of the log's records returns an
//! arbitrary value (stubbed: bit-level CRC over symbolic bytes is too
//! large for the solver, and its result only feeds a comparison, so any
//! value covers both outcomes). The image header and body checks over a
//! symbolic 512-byte header take the solver too long; the decision's proof
//! replaces them by any result, and the tests and the F401 package's node
//! search cover them. The log faces hold [`RECORDS`] records, so the loops
//! are bounded; the loop bodies are the same for every iteration, so the
//! bound does not hide a path.

use crate::boot::Place;
use crate::bootcore::{self, Apps, Boot, Diag, Plan, Settle, Stage0, CLEARED, MAX_COUNT};
use crate::log::{LogLayout, LogState, SlotRef};
use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{FACE_HEADER_SIZE, RECORD_SIZE};
use dagon_format::{SigVerifier, Version};
use dagon_port::{Flash, FlashError, FlashRegion};

/// Records each log face holds in the proofs.
const RECORDS: u32 = 2;
/// Body bytes of each slot in the proofs' layout.
const BODY: u32 = 256;

/// A flash whose every read returns arbitrary bytes and whose writes and
/// erases succeed or fail arbitrarily (stage0 never writes; the boot core
/// code the proofs reach does not either).
struct AnyFlash;

impl Flash for AnyFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, _: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        if kani::any() {
            return Err(FlashError::Hw);
        }
        // The longest read is a 512-byte header.
        let any: [u8; HEADER_SIZE] = kani::any();
        buf.copy_from_slice(&any[..buf.len()]);
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        if kani::any() {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        if kani::any() {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }
}

/// A verifier that accepts or refuses any signature, under any key id.
struct AnyVerifier;

impl SigVerifier for AnyVerifier {
    fn key_id(&self) -> [u8; 8] {
        kani::any()
    }
    fn verify(&self, _: &[u8], _: &[&[u8]], _: &[u8; 64]) -> bool {
        kani::any()
    }
}

struct NoDiag;

impl Diag for NoDiag {
    fn report(&mut self, code: u8) {
        assert!((1..=8).contains(&code), "a reason code stage0 defines");
    }
}

fn crc_stub(_: &[u8]) -> u32 {
    kani::any()
}

fn any_ref() -> SlotRef {
    SlotRef {
        slot: kani::any(),
        version: Version::from_raw(kani::any()),
        digest16: kani::any(),
    }
}

fn any_state() -> LogState {
    LogState {
        confirmed: kani::any::<bool>().then(any_ref),
        pending: kani::any::<bool>().then(any_ref),
        attempts: kani::any(),
        reverted: kani::any::<bool>().then(any_ref),
        last_reason: kani::any(),
    }
}

/// The F401's layout, shrunk so each face holds [`RECORDS`] records and
/// each slot one body chunk: the addresses keep their place in the map,
/// so the address arithmetic is the board's. The unwind bounds cover
/// the longest loop: a 512-byte header read, a 16-byte digest compare.
const LOG: LogLayout = LogLayout {
    faces: [0x0800_4000, 0x0800_8000],
    face_len: FACE_HEADER_SIZE as u32 + RECORDS * RECORD_SIZE as u32,
};
const SLOT_LEN: u32 = HEADER_SIZE as u32 + BODY;
const CORE_SLOTS: [Place; 2] = [
    Place::inline(0x0801_0000, SLOT_LEN),
    Place::inline(0x0806_0000, SLOT_LEN),
];
const APP_SLOTS: [Place; 2] = [
    Place::inline(0x0802_0000, SLOT_LEN),
    Place::inline(0x0804_0000, SLOT_LEN),
];

/// The Cortex-M entry point: the reset vector, the second word of the body.
fn entry(head: [u8; 8], _: u32) -> u32 {
    u32::from_le_bytes([head[4], head[5], head[6], head[7]]) & !1
}

/// The trial cell never counts more than `MAX_COUNT` starts, whatever
/// retained memory holds after a power-up.
#[kani::proof]
#[kani::unwind(3)]
fn a_cell_counts_at_most_max_count() {
    let cell: [u32; 2] = kani::any();
    let p = any_ref();
    assert!(bootcore::tried(cell, &p) <= MAX_COUNT);
}

/// The cell written for start `n` counts `n` (up to `MAX_COUNT`); while
/// it moves from start `a` to start `b >= a`, cut after its first store,
/// it counts `b`, and never fewer than `a`.
#[kani::proof]
#[kani::unwind(3)]
fn a_cut_cell_update_never_lowers_the_count() {
    let p = any_ref();
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    kani::assume(a <= b);
    let (old, new) = (bootcore::trial_cell(&p, a), bootcore::trial_cell(&p, b));
    let (a, b) = (a.min(MAX_COUNT), b.min(MAX_COUNT));
    assert_eq!(bootcore::tried(old, &p), a);
    assert_eq!(bootcore::tried(new, &p), b);
    // Word 0 is stored first.
    assert_eq!(bootcore::tried([new[0], old[1]], &p), b);
    assert!(bootcore::tried([new[0], old[1]], &p) >= a);
}

/// A cell written for an image whose header digest starts otherwise
/// counts nothing for this one, and a cleared cell counts nothing.
#[kani::proof]
#[kani::unwind(3)]
fn a_cell_for_another_image_counts_nothing() {
    let p = any_ref();
    let q = any_ref();
    kani::assume(p.digest16[..2] != q.digest16[..2]);
    let n: u8 = kani::any();
    assert_eq!(bootcore::tried(bootcore::trial_cell(&q, n), &p), 0);
    assert_eq!(bootcore::tried(CLEARED, &p), 0);
}

/// The cell's tagged values never read as each other: whatever retained
/// memory holds, a cell holding the app request or a result counts no
/// trial start, the request is no result, and a cut write or take of the
/// request (one word new, one old) is no request.
#[kani::proof]
// The array comparisons run memcmp over the cell's 8 bytes.
#[kani::unwind(9)]
fn the_cell_values_never_read_as_each_other() {
    let p = any_ref();
    let other: u32 = kani::any();
    let refusal = if kani::any() {
        bootcore::Refusal::NoImage
    } else {
        bootcore::Refusal::Rejected
    };
    let res = bootcore::app_result(refusal);
    assert_eq!(bootcore::tried(bootcore::START_APP, &p), 0);
    assert_eq!(bootcore::tried(res, &p), 0);
    assert_eq!(bootcore::result(bootcore::START_APP), None);
    assert_eq!(bootcore::result(res), Some(refusal));
    // One word of a result still counts it (the other word holding no
    // result, or this one); one word of a request does not.
    let codes = [bootcore::Refusal::NoImage, bootcore::Refusal::Rejected];
    if codes
        .iter()
        .all(|&r| r == refusal || bootcore::app_result(r)[0] != other)
    {
        assert_eq!(bootcore::result([res[0], other]), Some(refusal));
    }
    kani::assume(other != bootcore::START_APP[0]);
    let cut = [bootcore::START_APP[0], other];
    assert_eq!(
        bootcore::hand_off(cut) == bootcore::HandOff::Request,
        bootcore::result(cut).is_none()
    );
    assert!(!bootcore::take_request(|| cut, |_, _| {}));
    assert!(!bootcore::take_request(
        || [other, bootcore::START_APP[1]],
        |_, _| {}
    ));
}

/// Stage0's plan tries the pending core only while the cell counts fewer
/// than its starts, as the start after the counted ones, and otherwise
/// runs the confirmed core.
#[kani::proof]
#[kani::unwind(17)]
fn the_plan_tries_a_pending_core_only_while_its_starts_last() {
    let st = any_state();
    let tried: u8 = kani::any();
    let max: u8 = kani::any();
    match bootcore::plan(&st, tried, max) {
        Plan::Try {
            pending,
            attempt,
            fallback,
        } => {
            assert_eq!(Some(pending), st.pending);
            assert!(tried < max.min(MAX_COUNT));
            assert_eq!(attempt, tried + 1);
            assert!(attempt <= MAX_COUNT);
            assert_eq!(fallback, st.confirmed);
        }
        Plan::Run(c) => {
            assert_eq!(c, st.confirmed);
            assert!(st.pending.is_none() || tried >= max.min(MAX_COUNT));
        }
    }
}

/// The running core settles only what the log holds: it confirms itself
/// when it is the pending one, and records the rollback to its own slot
/// otherwise.
#[kani::proof]
#[kani::unwind(17)]
fn a_core_settles_only_the_pending_trial() {
    let st = any_state();
    let running: u8 = kani::any();
    let tried: u8 = kani::any();
    let max: u8 = kani::any();
    match bootcore::settle(&st, running, tried, max) {
        Settle::Nothing => assert!(st.pending.is_none()),
        Settle::Confirm(p) => {
            assert_eq!(Some(p), st.pending);
            assert_eq!(p.slot, running);
        }
        Settle::Rollback { back, .. } => {
            let p = st.pending.unwrap();
            assert_ne!(p.slot, running);
            assert_eq!((back.slot, back.digest16), (running, p.digest16));
        }
    }
}

/// Stage0's choice, for any log fold, cell, limit, slot count and image
/// check: a core starts only from a slot the MCU gives, named by the log,
/// on trial only as the pending core within its starts (the cell left for
/// the next reset counts that start), otherwise as the confirmed core.
#[kani::proof]
#[kani::unwind(17)]
fn stage0_starts_only_a_named_core_within_its_starts() {
    let st = any_state();
    let cell: [u32; 2] = kani::any();
    let max: u8 = kani::any();
    let slots: u8 = kani::any();
    let chosen = bootcore::choose(&st, cell, max, slots, |_, r| {
        assert!(r.is_some(), "every core checked is one the log names");
        if kani::any() {
            Ok(())
        } else {
            Err(())
        }
    });
    if let Ok(s) = chosen {
        assert!(s.slot < slots);
        if s.trial {
            let p = st.pending.unwrap();
            assert_eq!(s.slot, p.slot);
            let n = bootcore::tried(cell, &p);
            assert!(n < max.min(MAX_COUNT));
            assert_eq!(bootcore::tried(s.cell, &p), n + 1);
        } else {
            assert_eq!(Some(s.slot), st.confirmed.map(|c| c.slot));
        }
    }
}

/// Stage0's fold of the boot core log ([`bootcore::read_state`]) does
/// not panic on any flash content.
#[kani::proof]
#[kani::stub(dagon_format::crc32, crc_stub)]
#[kani::unwind(33)]
fn the_log_fold_never_panics() {
    let _ = bootcore::read_state(&mut AnyFlash, &LOG);
}

fn check_inline_stub<F: Flash, V: SigVerifier + ?Sized>(
    _: &mut F,
    at: Place,
    _: u8,
    _: (Option<&SlotRef>, &V),
    _: u16,
) -> Result<u32, bootcore::CoreFail> {
    if kani::any() {
        Ok(at.body)
    } else if kani::any() {
        Err(bootcore::CoreFail::Header)
    } else {
        Err(bootcore::CoreFail::Signature)
    }
}

fn read_state_stub<F: Flash>(_: &mut F, _: &LogLayout) -> Option<LogState> {
    kani::any::<bool>().then(any_state)
}

fn confirmed_app_stub<F: Flash, V: SigVerifier + ?Sized>(
    _: &mut F,
    _: &Apps<'_>,
    _: u16,
    _: &V,
) -> Option<(u8, u32)> {
    let slot: u8 = kani::any();
    kani::assume(usize::from(slot) < APP_SLOTS.len());
    kani::any::<bool>().then(|| (slot, APP_SLOTS[usize::from(slot)].body))
}

/// The whole of stage0's decision ([`bootcore::start`]) with its parts
/// replaced by any result they can give (the log fold proved above; the
/// image checks, whose header and body checks take the solver too long
/// over a symbolic 512-byte header, are covered by the tests and by the
/// node search of the F401 package):
/// it does not panic for any log fold, trial cell, limit and slot count,
/// and a core it starts lies in a boot core slot of the layout, at that
/// slot's body.
#[kani::proof]
#[kani::stub(bootcore::read_state, read_state_stub)]
#[kani::stub(bootcore::check_inline, check_inline_stub)]
#[kani::stub(bootcore::confirmed_app, confirmed_app_stub)]
#[kani::unwind(17)]
fn stage0_decision_never_panics() {
    let at = Stage0 {
        log: &LOG,
        slots: if kani::any() {
            &CORE_SLOTS
        } else {
            &CORE_SLOTS[..1]
        },
        max: kani::any(),
        mcu_id: kani::any(),
    };
    let apps = Apps {
        log: &LOG,
        slots: &APP_SLOTS,
        target_tag: kani::any(),
        entry,
    };
    let cell: [u32; 2] = kani::any();
    match bootcore::start(&mut AnyFlash, &at, &apps, cell, &AnyVerifier, &mut NoDiag) {
        Ok(Boot::Core(s)) => {
            assert_eq!(s.body, at.slots[usize::from(s.slot)].body);
        }
        Ok(Boot::App { slot, body }) => {
            assert_eq!(body, APP_SLOTS[usize::from(slot)].body);
        }
        Err(_) => {}
    }
}

/// Any record of target APP: any kind, slot, reason and image.
fn any_record() -> dagon_format::statelog::Record {
    let at = any_ref();
    dagon_format::statelog::Record {
        kind: kani::any(),
        target: dagon_format::statelog::target::APP,
        slot: at.slot,
        reason: kani::any(),
        seq: 0,
        version: at.version,
        digest16: at.digest16,
    }
}

/// Records folded in [`compaction_carries_the_whole_state`].
const FOLDED: usize = 4;

/// Attempts the proof's states hold at most: one BOOT_ATTEMPT is carried
/// per attempt, and 255 unrolled folds exhaust the solver's memory. The
/// loop body is the same at every attempt; the exhaustive test
/// `carry_is_exact` covers every count up to 255.
const ATTEMPTS: u8 = 15;

/// A compaction loses nothing: for every state a fold of [`FOLDED`]
/// records of any kind, slot, reason and image reaches (with at most
/// [`ATTEMPTS`] attempts), the records [`LogState::carry`] writes fold
/// from an empty log to that state again, every field of it (the
/// exhaustive test `carry_is_exact` covers longer folds over a small
/// alphabet).
#[kani::proof]
// The SlotRef comparisons run memcmp over the 16-byte digest.
#[kani::unwind(17)]
fn compaction_carries_the_whole_state() {
    let mut s = LogState::default();
    for _ in 0..FOLDED {
        s = s.apply(&any_record());
    }
    kani::assume(s.attempts <= ATTEMPTS);
    let again = s
        .carry()
        .fold(LogState::default(), |acc, (kind, at, reason)| {
            acc.apply(&dagon_format::statelog::Record {
                kind,
                target: dagon_format::statelog::target::APP,
                slot: at.slot,
                reason,
                seq: 0,
                version: at.version,
                digest16: at.digest16,
            })
        });
    assert!(again == s);
}
