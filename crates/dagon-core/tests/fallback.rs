//! Stage0's fallback (`bootcore::start`) when no boot core may start: the
//! app the log's confirmed APP record names starts, never a pending or
//! trial one, and only after every check of the boot core's own (the
//! record's digest, the signature under stage0's key, the image kind, MCU,
//! target tag, binding, placement, load address and size, the payload
//! digest) and an entry point inside the image. Stage0 halts only when no
//! such app exists, and reports a reason only then. One log holds the APP
//! and boot core records here, as on an MCU that keeps both in one face
//! pair.

mod bench;

use bench::*;
use dagon_core::bootcore::{start, Apps, Boot, CoreFail, Diag, NoCore, Stage0, CLEARED};
use dagon_ed25519::Ed25519Small;

/// One boot core slot, below the APP slots.
const CORE: [Place; 1] = [Place::inline(0x0801_0000, 0x1_0000)];
const MAX: u8 = 3;

/// The entry point of an image whose body starts with a Cortex-M vector
/// table: its reset vector, without the Thumb bit.
fn entry(head: [u8; 8], _body: u32) -> u32 {
    u32::from_le_bytes([head[4], head[5], head[6], head[7]]) & !1
}

const AT: Stage0<'static> = Stage0 {
    log: &LOG,
    slots: &CORE,
    max: MAX,
    mcu_id: McuId([0x02; 8]),
};
const APPS: Apps<'static> = Apps {
    log: &LOG,
    slots: SLOTS.slots,
    target_tag: TAG,
    entry,
};

fn stage0_verifier() -> Ed25519Small {
    Ed25519Small::new(*KeyPair::from_seed(Seed::new(SEED)).pk)
}

/// The header fields of a well-formed app for `slot`.
fn app_header(slot: u8, body: &[u8]) -> ImageHeader {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: McuId([0x02; 8]),
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: entry_of(slot),
        payload_size: body.len() as u32,
        version: Version::new(1, slot, 0),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    }
}

/// The body address of APP slot `slot`.
fn entry_of(slot: u8) -> u32 {
    SLOTS.slots[usize::from(slot)].body
}

/// An app body for `slot` whose vector table's reset vector is `reset`
/// (relative to the body).
fn app_body(slot: u8, reset: u32) -> Vec<u8> {
    let mut body: Vec<u8> = (0..2048u32).map(|i| (i ^ u32::from(slot)) as u8).collect();
    body[..4].copy_from_slice(&0x2001_0000u32.to_le_bytes());
    body[4..8].copy_from_slice(&(entry_of(slot).wrapping_add(reset) | 1).to_le_bytes());
    body
}

/// Signs `h` with `seed`.
fn signed(mut h: ImageHeader, seed: [u8; 32]) -> [u8; HEADER_SIZE] {
    let kp = KeyPair::from_seed(Seed::new(seed));
    h.key_id = key_id(&kp.pk);
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    h.to_bytes()
}

/// Puts `hdr` and `body` into APP slot `slot`; the header as a record.
fn put_app(m: &mut Mem, slot: u8, hdr: &[u8; HEADER_SIZE], body: &[u8]) -> SlotRef {
    let base = SLOTS.slots[usize::from(slot)].header;
    m.put(base, hdr);
    m.put(base + HEADER_SIZE as u32, body);
    slot_ref(slot, hdr)
}

/// A valid app in `slot`.
fn good_app(m: &mut Mem, slot: u8) -> SlotRef {
    let body = app_body(slot, 0x101);
    put_app(m, slot, &signed(app_header(slot, &body), SEED), &body)
}

/// The log's only face holds `recs`: (target, kind, reference).
fn log(m: &mut Mem, recs: &[(u8, u8, SlotRef)]) {
    let mut out = FaceHeader { generation: 1 }.to_bytes().to_vec();
    for (i, (t, k, r)) in recs.iter().enumerate() {
        let rec = Record {
            kind: *k,
            target: *t,
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

/// A boot core record naming a core that is not in its slot (the slot was
/// erased for a new core when the power went).
fn gone_core() -> SlotRef {
    SlotRef {
        slot: 0,
        version: Version::new(1, 0, 0),
        digest16: [0x33; 16],
    }
}

#[derive(Default)]
struct Seen(Vec<u8>);

impl Diag for Seen {
    fn report(&mut self, code: u8) {
        self.0.push(code);
    }
}

/// Stage0's decision, with and without a diagnostic output: the same, and
/// a code is reported exactly when nothing starts.
fn decided(m: &mut Mem) -> Result<Boot, NoCore<CoreFail>> {
    let v = stage0_verifier();
    let mut seen = Seen::default();
    let told = start(m, &AT, &APPS, CLEARED, &v, &mut seen);
    let quiet = start(
        m,
        &AT,
        &APPS,
        CLEARED,
        &v,
        &mut dagon_core::bootcore::NoDiag,
    );
    assert_eq!(told, quiet);
    match &told {
        Ok(_) => assert!(seen.0.is_empty(), "reported {:?} yet started", seen.0),
        Err(no) => assert_eq!(seen.0, [no.why.code()]),
    }
    told
}

/// The confirmed app's log: the core gone, the app provisioned and
/// confirmed, then `more`.
fn confirmed_app_log(m: &mut Mem, a: SlotRef, more: &[(u8, u8, SlotRef)]) {
    let core = gone_core();
    let mut recs = vec![
        (target::BOOTCORE, kind::PROVISION, core),
        (target::BOOTCORE, kind::CONFIRMED, core),
        (target::APP, kind::PROVISION, a),
        (target::APP, kind::CONFIRMED, a),
    ];
    recs.extend_from_slice(more);
    log(m, &recs);
}

#[test]
fn with_no_core_to_start_the_confirmed_app_starts() {
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    confirmed_app_log(&mut m, a, &[]);
    assert_eq!(
        decided(&mut m),
        Ok(Boot::App {
            slot: A,
            body: entry_of(A)
        })
    );
}

#[test]
fn the_confirmed_app_in_slot_b_starts_while_a_pending_app_waits_untried() {
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    let b = good_app(&mut m, B);
    confirmed_app_log(&mut m, b, &[(target::APP, kind::SLOT_PENDING, a)]);
    assert_eq!(
        decided(&mut m),
        Ok(Boot::App {
            slot: B,
            body: entry_of(B)
        })
    );
}

#[test]
fn a_core_that_may_start_starts_and_the_app_does_not() {
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    let mut core = app_header(A, &[0; 64]);
    core.image_kind = image_kind::BOOTCORE;
    core.build_flags = build_flags::VERIFY_ENFORCED;
    core.capabilities = capabilities::SIG_VERIFIER;
    core.load_addr = CORE[0].body;
    core.payload_sha256 = sha256(&[0x5A; 64]);
    let body_at = core.load_addr;
    let hdr = signed(core, SEED);
    m.put(CORE[0].header, &hdr);
    m.put(body_at, &[0x5A; 64]);
    let c = slot_ref(A, &hdr);
    log(
        &mut m,
        &[
            (target::BOOTCORE, kind::PROVISION, c),
            (target::APP, kind::PROVISION, a),
        ],
    );
    match decided(&mut m) {
        Ok(Boot::Core(s)) => assert_eq!((s.slot, s.trial), (A, false)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pending_or_trial_app_never_starts() {
    // The pending image is the confirmed one itself.
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    confirmed_app_log(&mut m, a, &[(target::APP, kind::SLOT_PENDING, a)]);
    let e = decided(&mut m).unwrap_err();
    assert!(e.named);

    // Another app is on trial: the log says the trial image runs.
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    let b = good_app(&mut m, B);
    confirmed_app_log(
        &mut m,
        a,
        &[
            (target::APP, kind::SLOT_PENDING, b),
            (target::APP, kind::BOOT_ATTEMPT, b),
        ],
    );
    assert!(decided(&mut m).is_err(), "started while a trial is open");

    // Only a pending app, nothing confirmed.
    let mut m = Mem::new();
    let b = good_app(&mut m, B);
    log(
        &mut m,
        &[
            (target::BOOTCORE, kind::PROVISION, gone_core()),
            (target::APP, kind::SLOT_PENDING, b),
        ],
    );
    assert!(decided(&mut m).is_err(), "a pending app started");
}

#[test]
fn with_no_app_record_nothing_starts() {
    let mut m = Mem::new();
    good_app(&mut m, A);
    log(&mut m, &[(target::BOOTCORE, kind::CONFIRMED, gone_core())]);
    let e = decided(&mut m).unwrap_err();
    assert_eq!((e.named, e.why), (true, CoreFail::Header));
}

/// One of the checks the confirmed app must pass, broken.
type Breaks = fn(&mut ImageHeader, &mut Vec<u8>) -> [u8; 32];

#[test]
fn an_app_failing_any_check_never_starts() {
    let cases: [(&str, Breaks); 12] = [
        ("signed by another key", |_, _| [5u8; 32]),
        ("a boot core", |h, _| {
            h.image_kind = image_kind::BOOTCORE;
            SEED
        }),
        ("another MCU", |h, _| {
            h.mcu_id = McuId([0x01; 8]);
            SEED
        }),
        ("another node's tag", |h, _| {
            h.target_tag = target_tag("another-node");
            SEED
        }),
        ("bound to slot B", |h, _| {
            h.slot_binding = B;
            SEED
        }),
        ("not inline", |h, _| {
            h.placement = placement::TRAILER;
            SEED
        }),
        ("linked elsewhere", |h, _| {
            h.load_addr += 0x200;
            SEED
        }),
        ("larger than its slot", |h, _| {
            h.payload_size = SLOTS.slots[0].room + 1;
            SEED
        }),
        ("a body not the signed one", |_, b| {
            b[100] ^= 1;
            SEED
        }),
        ("its entry past its end", |h, b| {
            let end = entry_of(A) + h.payload_size;
            b[4..8].copy_from_slice(&(end | 1).to_le_bytes());
            h.payload_sha256 = sha256(b);
            SEED
        }),
        ("its entry before its start", |h, b| {
            b[4..8].copy_from_slice(&((entry_of(A) - 2) | 1).to_le_bytes());
            h.payload_sha256 = sha256(b);
            SEED
        }),
        ("no entry at all", |h, b| {
            b[4..8].fill(0xFF);
            h.payload_sha256 = sha256(b);
            SEED
        }),
    ];
    for (what, broken) in cases {
        let mut m = Mem::new();
        let mut body = app_body(A, 0x101);
        let mut h = app_header(A, &body);
        let seed = broken(&mut h, &mut body);
        let hdr = signed(h, seed);
        let a = put_app(&mut m, A, &hdr, &body);
        confirmed_app_log(&mut m, a, &[]);
        assert!(decided(&mut m).is_err(), "{what}: started");
    }

    // Not signed at all, every digest right.
    let mut m = Mem::new();
    let body = app_body(A, 0x101);
    let a = put_app(&mut m, A, &app_header(A, &body).to_bytes(), &body);
    confirmed_app_log(&mut m, a, &[]);
    assert!(decided(&mut m).is_err(), "unsigned: started");

    // The record names another header than the one in the slot.
    let mut m = Mem::new();
    let a = good_app(&mut m, A);
    let named = SlotRef {
        digest16: [0x44; 16],
        ..a
    };
    confirmed_app_log(&mut m, named, &[]);
    assert!(decided(&mut m).is_err(), "another header: started");
}

/// A stage0 built without a node tag starts no app, not even one signed
/// with an all-zero tag.
#[test]
fn without_a_node_tag_no_app_starts() {
    let mut m = Mem::new();
    let body = app_body(A, 0x101);
    let mut h = app_header(A, &body);
    h.target_tag = [0; 16];
    let a = put_app(&mut m, A, &signed(h, SEED), &body);
    confirmed_app_log(&mut m, a, &[]);
    let untagged = Apps {
        target_tag: dagon_core::bootcore::NO_TAG,
        ..APPS
    };
    let v = stage0_verifier();
    let e = start(
        &mut m,
        &AT,
        &untagged,
        CLEARED,
        &v,
        &mut dagon_core::bootcore::NoDiag,
    );
    assert!(e.is_err(), "{e:?}");
}

/// The hand-off by reset (`start_handed`): a request the boot core wrote
/// whole runs the app decision once; an app it chooses starts, on trial
/// when pending; otherwise the result is left in the cell and only the
/// confirmed boot core starts. Without a whole request it is `start`.
mod handed {
    use super::*;
    use dagon_core::bootcore::{
        app_result, start_handed, Handed, Refusal, Start, NO_TAG, START_APP,
    };
    use std::cell::Cell;

    /// A valid boot core in the boot core slot.
    fn good_core(m: &mut Mem) -> SlotRef {
        let body = [0x5A; 64];
        let mut core = app_header(A, &body);
        core.image_kind = image_kind::BOOTCORE;
        core.build_flags = build_flags::VERIFY_ENFORCED;
        core.capabilities = capabilities::SIG_VERIFIER;
        core.load_addr = CORE[0].body;
        let hdr = signed(core, SEED);
        m.put(CORE[0].header, &hdr);
        m.put(CORE[0].body, &body);
        slot_ref(A, &hdr)
    }

    /// Stage0's decision over `cell`, which it reads and writes.
    fn handed(m: &mut Mem, cell: &Cell<[u32; 2]>) -> Result<Handed, NoCore<CoreFail>> {
        let write = |i: usize, w: u32| {
            let mut v = cell.get();
            v[i] = w;
            cell.set(v);
        };
        start_handed(
            m,
            (&AT, &APPS),
            (|| cell.get(), write),
            &stage0_verifier(),
            &mut dagon_core::bootcore::NoDiag,
        )
    }

    /// A good core and the app records `apps` in one log.
    fn with_core(m: &mut Mem, apps: &[(u8, u8, SlotRef)]) -> SlotRef {
        let c = good_core(m);
        let mut recs = vec![(target::BOOTCORE, kind::PROVISION, c)];
        recs.extend_from_slice(apps);
        log(m, &recs);
        c
    }

    fn core_with(cell: [u32; 2]) -> Handed {
        Handed::Core(Start {
            slot: A,
            body: CORE[0].body,
            trial: false,
            cell,
        })
    }

    #[test]
    fn a_whole_request_starts_the_confirmed_app_once() {
        let mut m = Mem::new();
        let a = good_app(&mut m, A);
        with_core(&mut m, &[(target::APP, kind::PROVISION, a)]);
        let cell = Cell::new(START_APP);
        assert_eq!(
            handed(&mut m, &cell),
            Ok(Handed::App {
                slot: A,
                body: entry_of(A),
                trial: false
            })
        );
        assert_eq!(cell.get(), CLEARED);
        // The next reset has no request: the boot core starts.
        assert_eq!(handed(&mut m, &cell), Ok(core_with(CLEARED)));
    }

    #[test]
    fn a_pending_app_starts_on_trial_and_its_attempt_is_recorded() {
        let mut m = Mem::new();
        let a = good_app(&mut m, A);
        let b = good_app(&mut m, B);
        with_core(
            &mut m,
            &[
                (target::APP, kind::PROVISION, a),
                (target::APP, kind::SLOT_PENDING, b),
            ],
        );
        let cell = Cell::new(START_APP);
        assert_eq!(
            handed(&mut m, &cell),
            Ok(Handed::App {
                slot: B,
                body: entry_of(B),
                trial: true
            })
        );
        let st = Log::load(&mut m, &LOG).unwrap().state;
        assert_eq!((st.pending, st.attempts), (Some(b), 1));
    }

    #[test]
    fn an_app_that_fails_its_checks_leaves_the_result_and_the_confirmed_core_starts() {
        let mut m = Mem::new();
        let a = good_app(&mut m, A);
        with_core(&mut m, &[(target::APP, kind::PROVISION, a)]);
        // One body byte changed.
        let at = entry_of(A) + 9;
        let mut byte = [0u8];
        m.read(at, &mut byte).unwrap();
        m.put(at, &[byte[0] ^ 1]);
        let cell = Cell::new(START_APP);
        let left = app_result(Refusal::Rejected);
        assert_eq!(handed(&mut m, &cell), Ok(core_with(left)));
        assert_eq!(cell.get(), left);
    }

    #[test]
    fn with_no_app_record_the_result_says_no_image() {
        let mut m = Mem::new();
        with_core(&mut m, &[]);
        let cell = Cell::new(START_APP);
        let left = app_result(Refusal::NoImage);
        assert_eq!(handed(&mut m, &cell), Ok(core_with(left)));
        assert_eq!(cell.get(), left);
    }

    #[test]
    fn without_a_node_tag_a_request_starts_no_app() {
        let mut m = Mem::new();
        let mut h = app_header(A, &app_body(A, 0x101));
        h.target_tag = NO_TAG;
        let body = app_body(A, 0x101);
        let a = put_app(&mut m, A, &signed(h, SEED), &body);
        with_core(&mut m, &[(target::APP, kind::PROVISION, a)]);
        let cell = Cell::new(START_APP);
        let untagged = Apps {
            target_tag: NO_TAG,
            ..APPS
        };
        let write = |i: usize, w: u32| {
            let mut v = cell.get();
            v[i] = w;
            cell.set(v);
        };
        let got = start_handed(
            &mut m,
            (&AT, &untagged),
            (|| cell.get(), write),
            &stage0_verifier(),
            &mut dagon_core::bootcore::NoDiag,
        );
        let left = app_result(Refusal::NoImage);
        assert_eq!(got, Ok(core_with(left)));
        assert_eq!(cell.get(), left);
    }

    #[test]
    fn half_a_request_runs_no_decision() {
        let mut m = Mem::new();
        let a = good_app(&mut m, A);
        with_core(&mut m, &[(target::APP, kind::PROVISION, a)]);
        for half in [[START_APP[0], 0], [0, START_APP[1]]] {
            let cell = Cell::new(half);
            assert_eq!(handed(&mut m, &cell), Ok(core_with(CLEARED)));
            assert_eq!(cell.get(), half, "left for the boot core");
        }
    }

    #[test]
    fn a_refused_request_without_a_core_starts_nothing() {
        let mut m = Mem::new();
        let a = good_app(&mut m, A);
        let core = gone_core();
        log(
            &mut m,
            &[
                (target::BOOTCORE, kind::PROVISION, core),
                (target::APP, kind::PROVISION, a),
            ],
        );
        m.put(entry_of(A), &[0; 4]);
        let cell = Cell::new(START_APP);
        let no = handed(&mut m, &cell).unwrap_err();
        assert!(no.named);
        assert_eq!(cell.get(), app_result(Refusal::Rejected));
    }
}
