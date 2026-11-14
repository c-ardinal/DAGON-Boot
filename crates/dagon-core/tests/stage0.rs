//! Stage0's decision (`bootcore::decide`) over real boot core images: every
//! path that may start a core (the confirmed core, the pending core on
//! trial, an empty log, the other slots no record names, the pending core
//! that ran out of starts) with a validly signed core, a core signed by
//! another key and a well-formed core with no signature; and no core at
//! all. Only a validly signed core ever starts, and the decision is the
//! same with and without a diagnostic output.

mod bench;

use bench::*;
use dagon_core::bootcore::{
    check_inline, choose, decide, read_state, trial_cell, CoreFail, Diag, NoCore, NoDiag, Stage0,
    Start, CLEARED,
};
use dagon_ed25519::Ed25519Small;

const CORE_SLOTS: [(u32, u32); 2] = [(0x0802_0000, 0x2_0000), (0x0804_0000, 0x2_0000)];
const MAX: u8 = 3;
const AT: Stage0<'static> = Stage0 {
    log: &LOG,
    slots: &CORE_SLOTS,
    max: MAX,
    mcu_id: mcu_id::STM32F401XE,
};

/// How the core in a slot is signed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sig {
    /// By the key stage0 holds.
    Valid,
    /// By another key (the header names that key).
    Bad,
    /// Not at all: a zero signature, every digest right.
    Unsigned,
}

const SIGS: [Sig; 3] = [Sig::Valid, Sig::Bad, Sig::Unsigned];

/// Stage0's verifier, under the key [`Sig::Valid`] signs with.
fn stage0_verifier() -> Ed25519Small {
    Ed25519Small::new(*KeyPair::from_seed(Seed::new(SEED)).pk)
}

/// A boot core for `slot`, signed as `sig`.
fn core(slot: u8, minor: u8, sig: Sig) -> ([u8; HEADER_SIZE], Vec<u8>) {
    let seed = if sig == Sig::Bad { [5; 32] } else { SEED };
    let kp = KeyPair::from_seed(Seed::new(seed));
    let body: Vec<u8> = (0..1500u32).map(|i| (i as u8) ^ minor).collect();
    let mut h = ImageHeader {
        image_kind: image_kind::BOOTCORE,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: CORE_SLOTS[usize::from(slot)].0 + HEADER_SIZE as u32,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: build_flags::VERIFY_ENFORCED,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: capabilities::SIG_VERIFIER,
        signature: [0; 64],
    };
    if sig != Sig::Unsigned {
        let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
        h.signature = *kp.sk.sign(msg, None);
    }
    (h.to_bytes(), body)
}

fn put_core(m: &mut Mem, slot: u8, minor: u8, sig: Sig) -> SlotRef {
    let (hdr, body) = core(slot, minor, sig);
    let base = CORE_SLOTS[usize::from(slot)].0;
    m.put(base, &hdr);
    m.put(base + HEADER_SIZE as u32, &body);
    slot_ref(slot, &hdr)
}

/// The boot core records `recs` as the log's only face.
fn core_log(m: &mut Mem, recs: &[(u8, SlotRef)]) {
    let mut out = FaceHeader { generation: 1 }.to_bytes().to_vec();
    for (i, (k, r)) in recs.iter().enumerate() {
        let rec = Record {
            kind: *k,
            target: target::BOOTCORE,
            slot: r.slot,
            reason: reason::NONE,
            seq: i as u32 + 1,
            version: r.version,
            digest16: r.digest16,
        };
        out.extend_from_slice(&rec.to_bytes());
    }
    m.put(LOG.faces[0], &out);
}

/// A diagnostic output that keeps what it was told.
#[derive(Default)]
struct Seen(Vec<u8>);

impl Diag for Seen {
    fn report(&mut self, code: u8) {
        self.0.push(code);
    }
}

type Chosen = Result<Start<u32>, NoCore<CoreFail>>;

/// Stage0's decision on `m` with the trial cell `cell`, run without a
/// diagnostic output and with one: both decide the same, and the output
/// hears one code, the reason's, exactly when no core starts.
fn decided(m: &mut Mem, cell: [u32; 2]) -> Chosen {
    let v = stage0_verifier();
    let quiet = decide(m, &AT, cell, &v, &mut NoDiag);
    let mut seen = Seen::default();
    let told = decide(m, &AT, cell, &v, &mut seen);
    assert_eq!(quiet, told, "a diagnostic output changes nothing");
    match &told {
        Ok(_) => assert!(seen.0.is_empty(), "nothing to report: {:?}", seen.0),
        Err(no) => assert_eq!(seen.0, [no.why.code()]),
    }
    told
}

/// What stage0 must do for a core signed as `sig` on a path where it is
/// the only startable candidate: start it (in `slot`, on trial or not)
/// only when it is validly signed, else start nothing for the reason `why`
/// (the first core it checked outside a trial).
fn only_valid_starts(chosen: Chosen, sig: Sig, (slot, trial): (u8, bool), why: CoreFail) {
    match (sig, chosen) {
        (Sig::Valid, Ok(s)) => {
            assert_eq!((s.slot, s.trial), (slot, trial));
            assert_eq!(s.body, CORE_SLOTS[usize::from(slot)].0 + HEADER_SIZE as u32);
        }
        (Sig::Valid, Err(e)) => panic!("a validly signed core did not start: {e:?}"),
        (_, Ok(s)) => panic!("a {sig:?} core started: {s:?}"),
        (_, Err(e)) => assert_eq!(e.why, why, "{sig:?}"),
    }
}

#[test]
fn the_confirmed_core_starts_only_when_validly_signed() {
    for sig in SIGS {
        let mut m = Mem::new();
        let a = put_core(&mut m, A, 0, sig);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
        only_valid_starts(
            decided(&mut m, CLEARED),
            sig,
            (A, false),
            CoreFail::Signature,
        );
    }
}

#[test]
fn the_pending_core_starts_on_trial_only_when_validly_signed() {
    for sig in SIGS {
        let mut m = Mem::new();
        // The confirmed core's slot is blank: nothing to fall back to.
        let a = slot_ref(A, &core(A, 0, Sig::Valid).0);
        let b = put_core(&mut m, B, 1, sig);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::SLOT_PENDING, b)]);
        // Refused, it leaves the confirmed core's blank slot as the reason.
        only_valid_starts(decided(&mut m, CLEARED), sig, (B, true), CoreFail::Header);
    }
}

#[test]
fn a_bad_pending_core_falls_back_to_the_valid_confirmed_one() {
    for sig in [Sig::Bad, Sig::Unsigned] {
        let mut m = Mem::new();
        let a = put_core(&mut m, A, 0, Sig::Valid);
        let b = put_core(&mut m, B, 1, sig);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::SLOT_PENDING, b)]);
        let s = decided(&mut m, CLEARED).unwrap();
        assert_eq!((s.slot, s.trial), (A, false), "{sig:?}");
    }
}

#[test]
fn with_an_empty_log_slot_a_starts_only_when_validly_signed() {
    for sig in SIGS {
        let mut m = Mem::new();
        put_core(&mut m, A, 0, sig);
        only_valid_starts(
            decided(&mut m, CLEARED),
            sig,
            (A, false),
            CoreFail::Signature,
        );
    }
}

#[test]
fn a_slot_no_record_names_starts_only_when_validly_signed() {
    for sig in SIGS {
        let mut m = Mem::new();
        // The log names A, whose slot is blank; B holds a core no record
        // names.
        let a = slot_ref(A, &core(A, 0, Sig::Valid).0);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
        put_core(&mut m, B, 1, sig);
        match decided(&mut m, CLEARED) {
            Err(e) if sig != Sig::Valid => {
                // The reason is the named core's (a blank slot A).
                assert_eq!((e.named, e.why), (true, CoreFail::Header));
            }
            chosen => only_valid_starts(chosen, sig, (B, false), CoreFail::Header),
        }
    }
}

#[test]
fn a_pending_core_out_of_starts_starts_only_when_validly_signed() {
    for sig in SIGS {
        let mut m = Mem::new();
        let a = slot_ref(A, &core(A, 0, Sig::Valid).0);
        let b = put_core(&mut m, B, 1, sig);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::SLOT_PENDING, b)]);
        let cell = trial_cell(&b, MAX);
        match decided(&mut m, cell) {
            Ok(s) => {
                assert_eq!(sig, Sig::Valid, "a {sig:?} core started");
                assert_eq!((s.slot, s.trial, s.cell), (B, false, cell));
            }
            Err(e) => {
                assert_ne!(sig, Sig::Valid, "a validly signed core did not start");
                assert_eq!((e.named, e.why), (true, CoreFail::Header));
            }
        }
    }
}

/// `choose`'s last resort (the pending core that ran out of starts, checked
/// against its record after every other slot refused) reached with stage0's
/// own check, here behind a check that refuses any core no record names:
/// it starts only a validly signed core.
#[test]
fn the_last_resort_starts_only_a_validly_signed_core() {
    for sig in SIGS {
        let mut m = Mem::new();
        let a = slot_ref(A, &core(A, 0, Sig::Valid).0);
        let b = put_core(&mut m, B, 1, sig);
        core_log(&mut m, &[(kind::PROVISION, a), (kind::SLOT_PENDING, b)]);
        let st = read_state(&mut m, &LOG).unwrap();
        let v = stage0_verifier();
        let mut checked = Vec::new();
        let chosen = choose(&st, trial_cell(&b, MAX), MAX, 2, |s, r| {
            checked.push((s, r.is_some()));
            if r.is_none() {
                return Err(CoreFail::Digest);
            }
            let slot = CORE_SLOTS[usize::from(s)];
            check_inline(&mut m, slot, s, (r, &v), mcu_id::STM32F401XE)
        });
        assert_eq!(
            checked,
            [(A, true), (B, false), (B, true)],
            "the last resort ran"
        );
        match chosen {
            Ok(s) => assert_eq!((sig, s.slot, s.trial), (Sig::Valid, B, false)),
            Err(_) => assert_ne!(sig, Sig::Valid),
        }
    }
}

#[test]
fn with_no_core_at_all_nothing_starts_and_the_reason_is_reported() {
    let mut m = Mem::new();
    let e = decided(&mut m, CLEARED).unwrap_err();
    assert_eq!((e.named, e.why), (false, CoreFail::Header));
}

/// An indicator shows the code as that many blinks five times faster than
/// the board's halt blink, then one dark period.
#[test]
fn an_indicator_blinks_the_code_then_pauses() {
    use dagon_core::bootcore::blink_code;
    use std::cell::RefCell;
    let seen = RefCell::new(Vec::new());
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        blink_code(
            2,
            1000,
            |on| {
                seen.borrow_mut()
                    .push(if on { "on" } else { "off" }.to_string())
            },
            |ms| {
                seen.borrow_mut().push(ms.to_string());
                assert!(seen.borrow().len() < 9, "stop");
            },
        )
    }));
    assert!(r.is_err());
    assert_eq!(
        seen.into_inner(),
        ["on", "100", "off", "100", "on", "100", "off", "100", "1000"]
    );
}

/// The reason codes a diagnostic output shows: one per reason, from 1,
/// the signature's added after the others.
#[test]
fn every_reason_has_its_own_code() {
    let all = [
        CoreFail::Header,
        CoreFail::Digest,
        CoreFail::Size,
        CoreFail::Kind,
        CoreFail::LoadAddr,
        CoreFail::Body,
        CoreFail::Signature,
    ];
    let codes: Vec<u8> = all.iter().map(|f| f.code()).collect();
    assert_eq!(codes, [1, 2, 3, 4, 5, 6, 7]);
}
