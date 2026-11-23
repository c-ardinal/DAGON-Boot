//! Boot core A/B: stage0's plan from the boot core records and the trial
//! cell, what a boot core records about itself at start, and one log
//! face pair holding both targets through compaction.

use dagon_core::bootcore::{
    cell_holds, choose, plan, read_state, settle, settle_log, trial_cell, tried, Cell, MustReset,
    Plan, Settle, CLEARED,
};
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
    // Nothing pending: the confirmed core; an empty log: none.
    let only_a = core_fold(&[rec(kind::PROVISION, target::BOOTCORE, a, 0)]);
    assert_eq!(plan(&only_a, 0, MAX), Plan::Run(Some(a)));
    assert_eq!(plan(&LogState::default(), 0, MAX), Plan::Run(None));
}

/// Stage0 starts only a core a record names, as the trial rules allow:
/// the confirmed one, or the pending one while its starts last. A core no
/// record names, a merely written one, or a pending one whose starts ran
/// out never starts, whatever else fails (the caller then falls back to
/// the confirmed app).
#[test]
fn stage0_starts_only_a_recorded_core_under_the_trial_rules() {
    let (a, b) = (r(0, 0), r(1, 1));
    let only = |good: &'static [u8]| {
        move |s: u8, r: Option<&SlotRef>| {
            assert!(r.is_some_and(|r| r.slot == s), "checked against its record");
            if good.contains(&s) {
                Ok(s)
            } else {
                Err(s)
            }
        }
    };
    // Confirmed B is corrupt; A holds a good core no record names.
    let st = core_fold(&[rec(kind::PROVISION, target::BOOTCORE, b, 0)]);
    let e = choose(&st, CLEARED, MAX, 2, only(&[0])).unwrap_err();
    assert_eq!((e.named, e.why), (true, Some(1)));
    // An empty log: nothing is checked, nothing starts.
    let e = choose(&LogState::default(), CLEARED, MAX, 2, only(&[0, 1])).unwrap_err();
    assert_eq!((e.named, e.why), (false, None));
    // Pending B fails its check and confirmed A is corrupt: nothing starts.
    let st = core_fold(&[
        rec(kind::PROVISION, target::BOOTCORE, a, 0),
        rec(kind::SLOT_PENDING, target::BOOTCORE, b, 0),
    ]);
    let e = choose(&st, CLEARED, MAX, 2, only(&[])).unwrap_err();
    assert_eq!((e.named, e.why), (true, Some(0)));
    // Pending B ran out of starts and confirmed A is corrupt: B, which
    // checks out, does not start without its trial.
    let cell = trial_cell(&b, MAX);
    let e = choose(&st, cell, MAX, 2, only(&[1])).unwrap_err();
    assert_eq!((e.named, e.why), (true, Some(0)));
    // While its starts last, B starts on trial.
    let start = choose(&st, CLEARED, MAX, 2, only(&[1])).unwrap();
    assert_eq!((start.slot, start.trial), (1, true));
}

#[test]
fn the_trial_cell_counts_only_for_the_image_it_names() {
    let (b, other) = (r(1, 1), r(1, 2));
    for n in 0..=MAX {
        assert_eq!(tried(trial_cell(&b, n), &b), n);
    }
    assert_eq!(tried(trial_cell(&other, 2), &b), 0, "another image");
    assert_eq!(tried([0, 0], &b), 0, "cleared");
    let [w, _] = trial_cell(&b, 2);
    let [old, _] = trial_cell(&b, 1);
    // An update cut between its two stores never lowers the count.
    assert_eq!(tried([w, old], &b), 2, "new word, old word");
    assert_eq!(tried([w, 0], &b), 2, "new word, cleared word");
    assert_eq!(tried([w ^ 0x1, w ^ 0x1], &b), 0, "count check wrong");
    assert_eq!(tried([w ^ 0x100, w ^ 0x100], &b), 0, "digest bits differ");
}

/// The check refuses a core not signed by the key stage0's verifier holds.
#[test]
fn check_inline_verifies_the_signature() {
    use dagon_core::bootcore::{check_inline, CoreFail};
    use dagon_ed25519::Ed25519Small;
    use dagon_format::header::*;
    use ed25519_compact::{KeyPair, Seed};
    let body = vec![7u8; 600];
    let mut h = ImageHeader {
        image_kind: image_kind::BOOTCORE,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: 2,
        slot_binding: 0,
        placement: placement::INLINE,
        load_addr: 0x200,
        payload_size: body.len() as u32,
        version: Version::new(1, 0, 0),
        build_flags: build_flags::VERIFY_ENFORCED,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: dagon_format::sha256(&body),
        key_id: [0; 8],
        target_tag: [0; 16],
        proto_version: 1,
        capabilities: capabilities::SIG_VERIFIER,
        signature: [0; 64],
    };
    let signer = KeyPair::from_seed(Seed::new([3; 32]));
    let other = KeyPair::from_seed(Seed::new([4; 32]));
    h.key_id = dagon_format::key_id(&signer.pk);
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *signer.sk.sign(msg, None);
    let mut m = Mem(vec![0xFF; 0x1000]);
    m.write(0, &h.to_bytes()).unwrap();
    m.write(0x200, &body).unwrap();
    let slot = dagon_core::boot::Place::inline(0, 0x1000);
    let (mine, theirs) = (Ed25519Small::new(*signer.pk), Ed25519Small::new(*other.pk));
    assert_eq!(check_inline(&mut m, slot, 0, (None, &mine), 2), Ok(0x200));
    assert!(matches!(
        check_inline(&mut m, slot, 0, (None, &theirs), 2),
        Err(CoreFail::Signature)
    ));
}

/// The cell check: a register that holds values passes and keeps its
/// value; a stuck or absent one fails.
#[test]
fn the_cell_check_tells_a_holding_cell_from_a_stuck_one() {
    use std::cell::Cell as C;
    let b = r(1, 1);
    let set = |reg: &C<[u32; 2]>, i: usize, w: u32| {
        let mut v = reg.get();
        v[i] = w;
        reg.set(v);
    };
    for start in [trial_cell(&b, 2), CLEARED, [0x1234_5678, 0x9ABC_DEF0]] {
        let reg = C::new(start);
        assert!(cell_holds(|| reg.get(), |i, w| set(&reg, i, w)));
        assert_eq!(reg.get(), start, "the value it had is back");
    }
    let stuck = C::new([0u32, 0]);
    assert!(!cell_holds(|| stuck.get(), |_, _| {}));
    assert!(!cell_holds(|| [u32::MAX; 2], |_, _| {}));
}

/// The cell check stores one word at a time, as both MCUs do. A reset
/// after any of its stores leaves a cell that counts no fewer starts for
/// the pending image than before (else stage0 would start a broken core
/// again and again).
#[test]
fn a_reset_inside_the_cell_check_never_lowers_the_count() {
    use std::cell::RefCell;
    let b = r(1, 1);
    let [w1, _] = trial_cell(&b, 1);
    let [w3, _] = trial_cell(&b, 3);
    let starts = [
        trial_cell(&b, 2),
        [w3, w1],
        [w1, 0],
        [0, w3],
        trial_cell(&r(1, 2), 4),
        CLEARED,
        [0x1234_5678, 0x9ABC_DEF0],
    ];
    for start in starts {
        let reg = RefCell::new(start);
        let stores = RefCell::new(Vec::new());
        cell_holds(
            || *reg.borrow(),
            |i, w| {
                reg.borrow_mut()[i] = w;
                stores.borrow_mut().push((i, w));
            },
        );
        let mut cut = start;
        for &(i, w) in stores.borrow().iter() {
            cut[i] = w;
            assert!(
                tried(cut, &b) >= tried(start, &b),
                "{start:08x?}: a reset here leaves {cut:08x?}"
            );
        }
    }
}

/// A pending core on a node whose cell does not hold rolls itself back
/// (stage0 could not count its starts) and resets; the old core runs.
#[test]
fn a_pending_core_without_a_holding_cell_rolls_itself_back() {
    let layout = LogLayout {
        faces: [0, 0x400],
        face_len: 0x400,
    };
    let mut m = Mem(vec![0xFF; 0x800]);
    m.write(0, &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let (a, b) = (r(0, 0), r(1, 1));
    let mut log = Log::load(&mut m, &layout).unwrap();
    log.append_core(&mut m, kind::PROVISION, a, 0).unwrap();
    log.append_core(&mut m, kind::SLOT_PENDING, b, 0).unwrap();
    let cell = Cell {
        value: CLEARED,
        max: MAX,
        holds: false,
    };
    assert_eq!(settle_log(&mut m, &layout, 1, cell, true), Err(MustReset));
    let st = Log::load(&mut m, &layout).unwrap().core;
    assert_eq!((st.confirmed, st.pending), (Some(a), None));
    assert_eq!(st.reverted, Some(b));
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
        // A word carries 28 checked bits: 2^-28 per word to match by chance,
        // and a match only makes stage0 give up on the core sooner.
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

mod hand_off {
    use dagon_core::bootcore::{
        app_result, hand_off, result, take_request, trial_cell, tried, HandOff, Refusal, CLEARED,
        START_APP,
    };
    use std::cell::Cell as C;

    /// A two-word cell; `stuck` keeps it from taking writes.
    struct Words {
        v: C<[u32; 2]>,
        stuck: bool,
        stores: C<u8>,
    }

    impl Words {
        fn new(v: [u32; 2]) -> Self {
            Words {
                v: C::new(v),
                stuck: false,
                stores: C::new(0),
            }
        }
        fn take(&self) -> bool {
            take_request(
                || self.v.get(),
                |i, w| {
                    self.stores.set(self.stores.get() + 1);
                    if !self.stuck {
                        let mut v = self.v.get();
                        v[i] = w;
                        self.v.set(v);
                    }
                },
            )
        }
    }

    #[test]
    fn stage0_takes_a_whole_request_once_and_clears_it() {
        let w = Words::new(START_APP);
        assert!(w.take());
        assert_eq!(w.v.get(), CLEARED);
        assert!(!w.take(), "taken once");
    }

    #[test]
    fn half_a_request_is_none_and_is_left_alone() {
        // A reset after the boot core's first store, or after stage0's
        // first clearing store.
        for cut in [[START_APP[0], 0], [0, START_APP[1]]] {
            let w = Words::new(cut);
            assert!(!w.take(), "{cut:x?}");
            assert_eq!(w.v.get(), cut);
            assert_eq!(w.stores.get(), 0);
        }
    }

    #[test]
    fn a_cell_that_takes_no_write_runs_no_decision() {
        let w = Words {
            stuck: true,
            ..Words::new(START_APP)
        };
        assert!(!w.take());
        assert!(!w.take());
    }

    #[test]
    fn the_boot_core_stays_on_a_result_from_either_word_and_asks_otherwise() {
        for r in [Refusal::NoImage, Refusal::Rejected] {
            let v = app_result(r);
            assert_eq!(hand_off(v), HandOff::Stay(r));
            assert_eq!(hand_off([v[0], 0]), HandOff::Stay(r));
            assert_eq!(hand_off([0, v[1]]), HandOff::Stay(r));
            // A result with a flipped code check is none.
            assert_eq!(result([v[0] ^ 1, v[1] ^ 1]), None);
        }
        assert_eq!(hand_off(CLEARED), HandOff::Request);
        assert_eq!(hand_off(START_APP), HandOff::Request);
        let p = super::r(0, 1);
        assert_eq!(hand_off(trial_cell(&p, 2)), HandOff::Request);
    }

    #[test]
    fn requests_and_results_count_no_trial_start() {
        let p = super::r(0, 1);
        assert_eq!(tried(START_APP, &p), 0);
        assert_eq!(tried(app_result(Refusal::NoImage), &p), 0);
        assert_eq!(tried(app_result(Refusal::Rejected), &p), 0);
        assert!(!Words::new(trial_cell(&p, 1)).take());
    }
}

/// A boot core whose MCU definition detaches its header: stage0 reads the
/// header from its own sector and hashes the body where the definition
/// puts it, and refuses a header made for another placement or linked
/// for another body.
#[test]
fn check_inline_takes_a_detached_header_where_the_definition_puts_it() {
    use dagon_core::boot::Place;
    use dagon_core::bootcore::{check_inline, CoreFail};
    use dagon_ed25519::Ed25519Small;
    use dagon_format::header::*;
    use ed25519_compact::{KeyPair, Seed};
    let kp = KeyPair::from_seed(Seed::new([3; 32]));
    let body = vec![9u8; 700];
    // Body at 0x1000 (the whole region), header in the sector at 0x3000.
    let place = Place::detached(0x3000, 0x1000, 0x2000);
    let signed = |f: &dyn Fn(&mut ImageHeader)| {
        let mut h = ImageHeader {
            image_kind: image_kind::BOOTCORE,
            port_kind: port_kind::SELF_BOOT,
            mcu_id: 2,
            slot_binding: 0,
            placement: placement::DETACHED,
            load_addr: 0x1000,
            payload_size: body.len() as u32,
            version: Version::new(1, 0, 0),
            build_flags: build_flags::VERIFY_ENFORCED,
            release_seq: 1,
            min_bootcore_version: Version::default(),
            payload_sha256: dagon_format::sha256(&body),
            key_id: dagon_format::key_id(&kp.pk),
            target_tag: [0; 16],
            proto_version: 1,
            capabilities: capabilities::SIG_VERIFIER,
            signature: [0; 64],
        };
        f(&mut h);
        let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
        h.signature = *kp.sk.sign(msg, None);
        let mut m = Mem(vec![0xFF; 0x4000]);
        m.write(0x3000, &h.to_bytes()).unwrap();
        m.write(0x1000, &body).unwrap();
        m
    };
    let v = Ed25519Small::new(*kp.pk);
    let check = |mut m: Mem| check_inline(&mut m, place, 0, (None, &v), 2);
    assert_eq!(check(signed(&|_| {})), Ok(0x1000));
    // The same image read as if its header were inline: no header there.
    let mut m = signed(&|_| {});
    assert_eq!(
        check_inline(&mut m, Place::inline(0x1000, 0x3000), 0, (None, &v), 2),
        Err(CoreFail::Header)
    );
    for (what, f) in [
        (
            "made inline",
            &(|h: &mut ImageHeader| h.placement = placement::INLINE) as &dyn Fn(&mut ImageHeader),
        ),
        ("linked past its header", &|h: &mut ImageHeader| {
            h.load_addr = 0x1200
        }),
    ] {
        assert_eq!(check(signed(f)), Err(CoreFail::LoadAddr), "{what}");
    }
    // The body fills the region (no header room taken from it).
    let mut big = signed(&|h| h.payload_size = 0x2000);
    assert_eq!(
        check_inline(&mut big, place, 0, (None, &v), 2),
        Err(CoreFail::Body)
    );
    assert_eq!(
        check(signed(&|h| h.payload_size = 0x2001)),
        Err(CoreFail::Size)
    );
}
