//! Fixed scenarios: the failure classes the formats must exclude, cuts at
//! every byte of one run, the negative control, and the cases the model's
//! shrinker found while the formats were iterated (kept as regressions).

use dagon_retained_model::boot::{self, Behaviour, Manifest, Target, VerifierReplacement};
use dagon_retained_model::fuse::{self, fuse_id, CallSite, Cell, FuseV1, FuseV2, LAYER_APP};
use dagon_retained_model::harness::{check, run_many, shrink, World};
use dagon_retained_model::identity::{self, identity, LoadError};
use dagon_retained_model::mem::{Mem, Survives};
use dagon_retained_model::record::{
    self, Geometry, Ring, Writer, RECORD_MAX, V1 as RecV1, V1_SIZE,
};
use dagon_retained_model::topology::{self, BodyCodec, BodyV1, Store};
use dagon_retained_model::world_fuse::{self as wf, FuseWorld};
use dagon_retained_model::world_record::{
    self as wr, Config, End::*, Op, RecordWorld, Stage, Ver::*,
};
use dagon_retained_model::world_topology::{self as wt, TopologyWorld};
use dagon_retained_model::world_trial::{self as wtr, TrialWorld};

const SEED: u64 = 0x5EED_F1C5;

fn ok<W: World>(cfg: &W::Config, ops: &[W::Op]) {
    if let Err(v) = check::<W>(cfg, ops, SEED) {
        panic!("{v:?}\n{cfg:#?}\n{ops:#?}");
    }
}

fn cfg(survives: Survives, stage0: bool, s0: usize, bc: usize) -> Config {
    Config {
        survives,
        stage0,
        s0_bytes: s0 * V1_SIZE,
        bc_bytes: bc * V1_SIZE,
        more: vec![],
        gap: 4,
        weak: false,
    }
}

fn stage(ver: wr::Ver, events: u8, end: wr::End) -> Stage {
    Stage { ver, events, end }
}

fn boot_op(s0: Stage, bc: Stage) -> Op {
    Op::Boot {
        s0,
        bc,
        more: vec![],
        app: V1,
        cut: None,
        power: false,
    }
}

fn plain() -> Op {
    boot_op(stage(V1, 2, Next), stage(V1, 3, Next))
}

// --- Boot record ---------------------------------------------------------

/// P2 (torn writes, also over a valid entry): a boot cut after every byte,
/// on a ring already wrapped so every append overwrites a valid entry.
#[test]
fn record_cut_after_every_byte() {
    let c = cfg(Survives::Power, true, 3, 4);
    let mut ops: Vec<Op> = (0..4).map(|_| plain()).collect();
    for k in 0..(5 * 22) {
        ops.push(Op::Boot {
            s0: stage(V1, 2, Next),
            bc: stage(V1, 3, Next),
            more: vec![],
            app: V1,
            cut: Some(k),
            power: false,
        });
        ops.push(plain());
        ok::<RecordWorld>(&c, &ops);
        ops.truncate(4);
    }
}

/// Negative control: the payload-first/CRC-last shape without a
/// commit byte is caught (P2), and the shrunk case passes with layout 1.
#[test]
fn weak_layout_is_caught() {
    #[derive(Debug)]
    struct WeakWorld;
    impl World for WeakWorld {
        type Config = Config;
        type Op = Op;
        fn generate(rng: &mut dagon_retained_model::rng::Rng) -> (Config, Vec<Op>) {
            let (c, ops) = RecordWorld::generate(rng);
            (Config { weak: true, ..c }, ops)
        }
        fn run(
            c: &Config,
            ops: &[Op],
            seed: u64,
        ) -> Result<(), dagon_retained_model::harness::Violation> {
            RecordWorld::run(c, ops, seed)
        }
        fn simpler(op: &Op) -> Vec<Op> {
            RecordWorld::simpler(op)
        }
    }
    let f = run_many::<WeakWorld>(SEED, 200_000).expect_err("the weak layout must fail");
    assert!(
        ["P2", "P1", "P4", "P6"].contains(&f.violation.property),
        "{f:?}"
    );
    let v1 = Config {
        weak: false,
        ..f.config.clone()
    };
    if let Err(v) = check::<RecordWorld>(&v1, &f.ops, f.seed) {
        panic!("layout 1 fails the shrunk case: {v:?}\n{f:?}");
    }
}

/// An old stage0 that wrote nothing, then its upgrade starting on an
/// empty ring (P4: identities stay unique, the boot core's own boots stay
/// apart from stage0's).
#[test]
fn old_stage0_then_upgrade() {
    let c = cfg(Survives::ResetRandom, true, 3, 8);
    let mut ops: Vec<Op> = (0..5)
        .map(|_| boot_op(stage(Old, 0, Next), stage(V1, 2, Next)))
        .collect();
    ops.extend((0..5).map(|_| plain()));
    ops.push(boot_op(stage(V1, 1, Halt), stage(V1, 1, Next)));
    ops.push(plain());
    ok::<RecordWorld>(&c, &ops);
}

/// Sequence wrap and long silence: stage0-only boots past 2^15, 2^31 and up to a
/// full 2^32 wrap while the boot core is silent, then both run (P5).
#[test]
fn silent_boot_core_across_wrap() {
    for boots in [40_000u64, (1 << 30) + 7, (1 << 31) + 3, (1 << 31) - 70] {
        for stage0 in [true, false] {
            let c = cfg(Survives::Power, stage0, 4, 8);
            let ops = vec![
                plain(),
                plain(),
                Op::FastForward {
                    boots,
                    linked: false,
                },
                plain(),
                Op::FastForward {
                    boots,
                    linked: true,
                },
                plain(),
            ];
            ok::<RecordWorld>(&c, &ops);
        }
    }
}

/// A layout change with surviving bytes, both directions, readers of
/// both layouts (P9).
#[test]
fn layout_skew_both_ways() {
    let c = cfg(Survives::Power, true, 4, 8);
    for (a, b) in [(V1, V2), (V2, V1)] {
        let ops = vec![
            boot_op(stage(a, 2, Next), stage(a, 2, Next)),
            boot_op(stage(b, 2, Next), stage(a, 2, Next)),
            boot_op(stage(b, 2, Next), stage(b, 2, Next)),
            Op::Boot {
                s0: stage(a, 1, Next),
                bc: stage(b, 1, Next),
                more: vec![],
                app: V2,
                cut: None,
                power: false,
            },
            boot_op(stage(a, 2, Next), stage(a, 2, Next)),
        ];
        ok::<RecordWorld>(&c, &ops);
    }
}

/// A boot that overflows its ring and halts; the next app accounts
/// the loss (P7) and still sees the halt (P1).
#[test]
fn overflow_then_halt() {
    let c = cfg(Survives::ResetCleared, true, 3, 4);
    let ops = vec![
        plain(),
        Op::Boot {
            s0: stage(V1, 2, Next),
            bc: stage(V1, 30, Halt),
            more: vec![],
            app: V1,
            cut: None,
            power: false,
        },
        plain(),
    ];
    ok::<RecordWorld>(&c, &ops);
}

/// A hung boot is not the current one: stage0 hangs, then a boot whose boot core must not
/// join the hung boot (P6).
#[test]
fn hang_is_not_the_current_boot() {
    let c = cfg(Survives::Power, true, 4, 8);
    let ops = vec![
        plain(),
        boot_op(stage(V1, 2, Hang), stage(V1, 2, Next)),
        boot_op(stage(V1, 2, Next), stage(V1, 2, Hang)),
        plain(),
    ];
    ok::<RecordWorld>(&c, &ops);
}

/// P3: foreign bytes over both rings never move a write out of a ring.
#[test]
fn fuzzed_rings_keep_writes_inside() {
    let c = cfg(Survives::Power, true, 4, 8);
    let mut ops = vec![plain(), plain()];
    for w in 0..2 {
        for at in (0..200).step_by(13) {
            ops.push(Op::Fuzz {
                writer: w,
                at,
                len: 23,
            });
            ops.push(plain());
        }
    }
    ok::<RecordWorld>(&c, &ops);
}

/// Found by the shrinker: a foreign write destroying stage0's newest entry;
/// stage0 continues past it, so no boot number or identity is used twice.
#[test]
fn regression_destroyed_newest_entry() {
    let c = Config {
        survives: Survives::ResetCleared,
        stage0: true,
        s0_bytes: 3 * V1_SIZE,
        bc_bytes: 4 * V1_SIZE,
        more: vec![],
        gap: 12,
        weak: false,
    };
    let one = || boot_op(stage(V1, 1, Next), stage(V1, 1, Next));
    let ops = vec![
        one(),
        one(),
        one(),
        Op::Fuzz {
            writer: 0,
            at: 113,
            len: 1,
        },
        one(),
    ];
    ok::<RecordWorld>(&c, &ops);
}

/// Found by the shrinker: a later stage's link to a boot whose opener entry
/// was destroyed by another layout's writer is ordered by its number.
#[test]
fn regression_stale_link_after_skew() {
    let c = Config {
        survives: Survives::ResetCleared,
        stage0: true,
        s0_bytes: 8 * V1_SIZE,
        bc_bytes: 16 * V1_SIZE,
        more: vec![],
        gap: 4,
        weak: false,
    };
    let ops = vec![
        boot_op(stage(V1, 2, Next), stage(V1, 1, Next)),
        Op::FastForward {
            boots: 1_073_741_811,
            linked: false,
        },
        boot_op(stage(V1, 1, Next), stage(V1, 1, Next)),
        boot_op(stage(V2, 1, Next), stage(V1, 1, Next)),
    ];
    ok::<RecordWorld>(&c, &ops);
}

/// Found by the shrinker: a writer silent for more than half the sequence
/// space; the loss statement is by append order, not by serial distance.
#[test]
fn regression_window_after_half_space() {
    let c = Config {
        survives: Survives::Power,
        stage0: true,
        s0_bytes: 8 * V1_SIZE,
        bc_bytes: 8 * V1_SIZE,
        more: vec![],
        gap: 12,
        weak: false,
    };
    let ops = vec![
        boot_op(stage(V1, 1, Halt), stage(V1, 1, Next)),
        Op::FastForward {
            boots: 1_073_741_842,
            linked: false,
        },
        boot_op(stage(V1, 1, Next), stage(V1, 1, Next)),
    ];
    ok::<RecordWorld>(&c, &ops);
}

/// Found by the shrinker: a writer restarting on an empty ring after its
/// entries were overwritten by another layout must not restart at the same
/// number (CRC-32 over a region of CRC-32-protected entries is nearly
/// constant; the fresh start uses FNV-1a). P11.
#[test]
fn regression_fresh_start_differs() {
    let c = Config {
        survives: Survives::ResetRandom,
        stage0: true,
        s0_bytes: 3 * V1_SIZE,
        bc_bytes: 8 * V1_SIZE,
        more: vec![],
        gap: 0,
        weak: false,
    };
    let b = |s0v, s0e, bce, app| Op::Boot {
        s0: stage(s0v, s0e, Next),
        bc: stage(V1, bce, Next),
        more: vec![],
        app,
        cut: None,
        power: false,
    };
    let ops = vec![
        b(V1, 1, 39, V1),
        b(V1, 4, 1, V1),
        b(V2, 3, 1, V2),
        b(V2, 4, 1, V1),
        Op::FastForward {
            boots: 8_388_608,
            linked: true,
        },
        b(V2, 4, 1, V2),
    ];
    assert!(check::<RecordWorld>(&c, &ops, 0x61c5_8a7d_26ae_79af).is_ok());
}

/// P10: the largest region pages exactly at every link size, and the 0x20
/// summary fits one page with the most writers.
#[test]
fn record_size_and_paging() {
    use dagon_proto::max_payload;
    let region: Vec<u8> = (0..RECORD_MAX).map(|i| i as u8).collect();
    for link in [max_payload::I2C, max_payload::UART, max_payload::USB] {
        assert_eq!(
            record::paged_read(&region, record::record_page(link)),
            region
        );
    }
    let geo = Geometry {
        region: 0..16 * 4 * V1_SIZE,
        rings: (0..16)
            .map(|i| Ring {
                base: i * 4 * V1_SIZE,
                slots: 4,
            })
            .collect(),
    };
    geo.validate(V1_SIZE).unwrap();
    let mut m = Mem::new(geo.region.end, 0, 1);
    let mut h = None;
    for w in 0..16u8 {
        let mut wr = Writer::open(&RecV1, &m, &geo, w, h, 0);
        wr.append(&mut m, 1, 1, [0; 3]).unwrap();
        h = Some(wr.handoff());
    }
    let r = record::read(&RecV1, m.bytes(), &geo);
    assert_eq!(r.boots.len(), 1);
    assert!(record::summary(1, &r).len() <= record::record_page(max_payload::I2C));
    let big = Geometry {
        region: 0..RECORD_MAX + 1,
        rings: vec![Ring { base: 0, slots: 2 }],
    };
    assert!(big.validate(V1_SIZE).is_err());
}

// --- Lockup fuse ---------------------------------------------------------

fn fuse_cfg(trips: usize, boot_exts: u8, app_exts: u8) -> wf::Config {
    wf::Config {
        survives: Survives::Power,
        trips,
        boot_exts,
        app_exts,
        manifest: Manifest {
            budget_us: 100,
            windows: vec![0],
            devices: vec![(0x20, 1)],
        },
        trial_ms: 2000,
        window_ms: 100,
        verifier: None,
        image_ok: [true, true],
    }
}

fn run(boot: Vec<wf::Call>, app: Vec<wf::Call>, cut: Option<u32>) -> wf::Op {
    wf::Op::Run {
        boot,
        app,
        cut,
        power: false,
    }
}

fn call(ext: u8, step: u8, b: Behaviour) -> wf::Call {
    wf::Call { ext, step, b }
}

/// P22 (a reset between a call's return and its disarm): a reset after every fuse
/// store of a run, including between a call's return and its disarm.
#[test]
fn fuse_cut_after_every_byte() {
    let c = fuse_cfg(3, 2, 3);
    let calls = vec![
        call(0, 0, Behaviour::Return { us: 5 }),
        call(1, 1, Behaviour::Overrun),
    ];
    for k in 0..400 {
        let ops = vec![
            run(vec![], calls.clone(), None),
            run(vec![], calls.clone(), Some(k)),
            run(vec![], calls.clone(), None),
        ];
        ok::<FuseWorld>(&c, &ops);
    }
}

/// P23: an app lockup, then the boot core's start (which must neither clear
/// nor act on the app's fuse), then the app's start (extension disabled,
/// serving). P23.
#[test]
fn app_lockup_hands_off_to_the_app() {
    let c = fuse_cfg(2, 2, 2);
    let ops = vec![
        run(vec![], vec![call(1, 0, Behaviour::Lockup)], None),
        run(vec![], vec![call(1, 0, Behaviour::Lockup)], None),
        run(vec![], vec![], Some(30)),
        run(vec![], vec![call(1, 0, Behaviour::Lockup)], None),
    ];
    ok::<FuseWorld>(&c, &ops);
}

/// Found by the shrinker: a clear request that outlives its trip must not
/// clear the same extension's next trip (one request, one clear).
#[test]
fn regression_clear_request_clears_once() {
    let c = wf::Config {
        image_ok: [false, true],
        ..fuse_cfg(2, 1, 2)
    };
    let ops = vec![
        run(vec![call(0, 2, Behaviour::Lockup)], vec![], None),
        run(vec![], vec![], None),
        wf::Op::HostClear { pick: 136 },
        run(vec![call(0, 3, Behaviour::Lockup)], vec![], None),
        run(vec![], vec![], None),
    ];
    ok::<FuseWorld>(&c, &ops);
}

/// P14-P17: every behaviour at every boot step reaches the decision the
/// state log gives, and a handler is not called after its fault limit.
#[test]
fn every_behaviour_at_every_step() {
    let bs = [
        Behaviour::Return { us: 500 },
        Behaviour::Overrun,
        Behaviour::Access(Target::FlashController),
        Behaviour::Access(Target::StateLog),
        Behaviour::Access(Target::Watchdog),
        Behaviour::Access(Target::BootRecord),
        Behaviour::Access(Target::Fuse),
        Behaviour::Access(Target::OtherExtension),
        Behaviour::Access(Target::Window(5)),
        Behaviour::BusWrite {
            device: 0x21,
            register: 0,
        },
        Behaviour::Lockup,
    ];
    for b in bs {
        for step in 0..boot::STEPS as u8 {
            let c = fuse_cfg(2, 2, 1);
            let ops = vec![
                run(
                    (0..boot::STEPS as u8).map(|s| call(0, s, b)).collect(),
                    vec![],
                    None,
                ),
                run(vec![call(1, step, b)], vec![], None),
                run(vec![], vec![], None),
            ];
            ok::<FuseWorld>(&c, &ops);
        }
    }
}

/// P19: a verifier replacement decides only when listed, hash-matched and
/// carried by a signed image.
#[test]
fn verifier_replacement_needs_all_three() {
    for bits in 0..16u8 {
        let v = VerifierReplacement {
            declared: bits & 1 != 0,
            hash_matches: bits & 2 != 0,
            image_signed: bits & 4 != 0,
            verdict: bits & 8 != 0,
        };
        let c = wf::Config {
            verifier: Some(v),
            image_ok: [true, false],
            ..fuse_cfg(2, 1, 1)
        };
        ok::<FuseWorld>(
            &c,
            &[
                wf::Op::ReplaceImage { app: true },
                run(vec![], vec![], None),
            ],
        );
    }
}

/// P21 (near-collision identities, more than 16 extensions and more
/// than 4 slots): every field change gives a new fuse id.
#[test]
fn fuse_ids_distinct_over_large_sets() {
    let mut seen = std::collections::BTreeSet::new();
    for layer in [1u8, 2, 3] {
        for slot in 0..6u32 {
            for chain in 0..20u32 {
                for flip in 0..4usize {
                    let mut artifact = [7u8; 32];
                    artifact[31 - flip] ^= 1;
                    let mut header = [9u8; 32];
                    header[flip] ^= (slot as u8) << 1;
                    let id = fuse_id(&CallSite {
                        layer,
                        slot,
                        header,
                        chain,
                        artifact,
                    });
                    assert!(seen.insert(id));
                }
            }
        }
    }
}

/// P25: bytes of another fuse layout naming an installed call site are
/// never acted on.
#[test]
fn other_fuse_layout_is_ignored() {
    let cells = [Cell { base: 0, trips: 2 }];
    let id = fuse_id(&CallSite {
        layer: LAYER_APP,
        slot: 0,
        header: [1; 32],
        chain: 0,
        artifact: [2; 32],
    });
    let mut m = Mem::new(200, 0, 1);
    fuse::arm(&FuseV2, &mut m, cells[0], LAYER_APP, id).unwrap();
    let s = fuse::start(&FuseV1, &mut m, &cells, 0, true, &[id]).unwrap();
    assert!(s.disabled.is_empty());
}

// --- MCU identity and topology --------------------------------------------

/// P12: refusal at every carrier width with both ids named, unknown values
/// never matched, round trip through the topology node.
#[test]
fn identity_load_and_match() {
    let ids: Vec<String> = (0..500).map(|i| format!("mcu-{i}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let t = identity::load(&refs, identity).unwrap();
    for id in &refs {
        assert_eq!(t.lookup(identity(id)), Some(*id));
    }
    for v in [
        0u64,
        u64::MAX,
        identity("not-loaded"),
        identity(refs[0]) ^ 1,
    ] {
        assert_eq!(t.lookup(v), None);
    }
    fn low_byte(id: &str) -> u64 {
        identity(id) & 0xFF | 0x100
    }
    match identity::load(&refs, low_byte) {
        Err(LoadError::Collision { a, b, .. }) => assert_ne!(a, b),
        other => panic!("{other:?}"),
    }
}

/// P13: a replacement interrupted after every byte of erase and program,
/// on top of a previous copy; then a completed one.
#[test]
fn topology_cut_after_every_byte() {
    let c = wt::Config {
        bank: 1024,
        random_start: false,
    };
    let body = BodyV1.encode(&wt::topology(2)).unwrap();
    let store = Store {
        base: 16,
        bank: 1024,
    };
    let last = store.commit_step(body.len()) as u32 + 2;
    for k in 0..=last {
        let ops = vec![
            wt::Op::Push { topo: 1, cut: None },
            wt::Op::Push {
                topo: 2,
                cut: Some(k),
            },
            wt::Op::Push {
                topo: 2,
                cut: Some(k / 2),
            },
            wt::Op::Push { topo: 3, cut: None },
        ];
        ok::<TopologyWorld>(&c, &ops);
    }
}

/// Bounds: a topology over the bank is refused before any byte.
#[test]
fn topology_too_large_is_refused() {
    let store = Store { base: 0, bank: 128 };
    let body = BodyV1.encode(&wt::topology(5)).unwrap();
    assert!(body.len() > store.capacity());
    assert_eq!(
        topology::host_plan(&store, &body),
        Err(topology::Refused::TooLarge)
    );
}

/// The shrinker reduces a failure to the operations it needs.
#[test]
fn shrinker_minimises() {
    struct Toy;
    impl World for Toy {
        type Config = ();
        type Op = u8;
        fn generate(_: &mut dagon_retained_model::rng::Rng) -> ((), Vec<u8>) {
            ((), vec![])
        }
        fn run(_: &(), ops: &[u8], _: u64) -> Result<(), dagon_retained_model::harness::Violation> {
            let bad = ops.contains(&3) && ops.contains(&7);
            match bad {
                true => Err(dagon_retained_model::harness::Violation::new("P0", 0, "")),
                false => Ok(()),
            }
        }
        fn simpler(_: &u8) -> Vec<u8> {
            vec![]
        }
    }
    let ops = shrink::<Toy>(&(), vec![1, 3, 5, 9, 7, 2], 0, "P0");
    assert_eq!(ops, vec![3, 7]);
}

/// Found by the shrinker: with no room for a verdict the watchdog flag
/// stays set, so the layer must not arm a call that a later reset could
/// leave armed and the flag would misread as a lockup (P22).
#[test]
fn regression_unstored_verdict_blocks_arming() {
    let c = wf::Config {
        window_ms: 0,
        ..fuse_cfg(2, 3, 2)
    };
    let ops = vec![
        run(vec![call(0, 3, Behaviour::Lockup)], vec![], None),
        run(vec![call(2, 2, Behaviour::Lockup)], vec![], None),
        run(vec![], vec![call(0, 2, Behaviour::Lockup)], None),
        run(vec![], vec![], Some(167)),
        wf::Op::HostClear { pick: 71 },
        run(vec![], vec![], None),
        run(vec![], vec![], None),
        run(vec![], vec![], None),
    ];
    ok::<FuseWorld>(&c, &ops);
}

/// Found by the shrinker: with one record slot, a verdict whose call was
/// already resolved must be freed before the next verdict is stored, or
/// the layer stops arming for nothing (P22, start order: free, classify,
/// act).
#[test]
fn regression_free_before_classify() {
    let c = wf::Config {
        survives: Survives::ResetCleared,
        window_ms: 100,
        image_ok: [true, false],
        ..fuse_cfg(1, 3, 5)
    };
    let ops = vec![
        run(vec![], vec![call(2, 3, Behaviour::Lockup)], None),
        run(vec![], vec![call(4, 1, Behaviour::Lockup)], None),
        wf::Op::HostClear { pick: 62 },
        run(vec![], vec![], Some(42)),
        run(vec![call(0, 1, Behaviour::Lockup)], vec![], None),
        run(vec![], vec![], None),
        wf::Op::HostClear { pick: 141 },
        run(vec![], vec![], None),
    ];
    ok::<FuseWorld>(&c, &ops);
}

/// Found by the shrinker: foreign bytes destroy the clean verdict of an
/// interrupted app call; a later watchdog reset may then trip that call
/// (it never returned). Stated in the limits; never a returned call.
#[test]
fn regression_lost_clean_verdict() {
    let c = wf::Config {
        survives: Survives::ResetCleared,
        window_ms: 0,
        ..fuse_cfg(3, 1, 3)
    };
    let ops = vec![
        run(
            vec![],
            vec![call(0, 4, Behaviour::Access(Target::Window(1)))],
            Some(293),
        ),
        run(vec![call(0, 4, Behaviour::Lockup)], vec![], None),
        wf::Op::Skew {
            cell: 0,
            slot: 0,
            tripped: false,
            pick: 106,
        },
        run(vec![], vec![], None),
    ];
    assert!(check::<FuseWorld>(&c, &ops, 0x8fa6_2ebc_b7e3_7e5f).is_ok());
}

/// Found by the shrinker: a compressed run after another layout destroyed
/// stage0's newest entries continues below them (order not claimed while
/// the older entries are held).
#[test]
fn regression_regression_in_fast_forward() {
    let c = Config {
        survives: Survives::ResetRandom,
        stage0: true,
        s0_bytes: 8 * V1_SIZE,
        bc_bytes: 8 * V1_SIZE,
        more: vec![],
        gap: 12,
        weak: false,
    };
    let b = |v, e, end| boot_op(stage(v, e, end), stage(V1, 1, Next));
    let ops = vec![
        b(V1, 4, Next),
        b(V1, 1, Next),
        b(V1, 3, Next),
        b(V1, 1, Hang),
        b(V1, 1, Next),
        b(V2, 1, Next),
        Op::FastForward {
            boots: 1,
            linked: false,
        },
        b(V1, 1, Next),
    ];
    assert!(check::<RecordWorld>(&c, &ops, 0xaf34_6dd5_6cb7_46ae).is_ok());
}

/// A writer whose newest entries another layout of the same stage destroyed
/// (layout 1, then 2, then 1) continues past them: no identity a consumer
/// saw comes back with other content, and the jump is stated as a gap.
#[test]
fn layout_switch_never_reuses_identities() {
    let geo = Geometry {
        region: 0..112,
        rings: vec![Ring { base: 0, slots: 4 }],
    };
    let geo2 = Geometry {
        region: 0..112,
        rings: vec![Ring { base: 0, slots: 3 }],
    };
    let mut m = Mem::new(112, 0, 3);
    let mut w = Writer::open(&RecV1, &m, &geo, 0, None, 0);
    for k in 0..6 {
        w.append(&mut m, 1, k, [0; 3]).unwrap();
    }
    let seen: Vec<(u32, u8)> = record::read(&RecV1, m.bytes(), &geo)
        .events()
        .map(|e| (e.entry.eseq, e.entry.kind))
        .collect();
    let mut w2 = Writer::open(&dagon_retained_model::skew::V2, &m, &geo2, 0, None, 0);
    w2.append(&mut m, 2, 0x20, [0; 3]).unwrap();
    let mut w = Writer::open(&RecV1, &m, &geo, 0, None, 0);
    w.append(&mut m, 1, 0x30, [0; 3]).unwrap();
    let r = record::read(&RecV1, m.bytes(), &geo);
    for e in r.events() {
        let clash = seen
            .iter()
            .any(|&(q, k)| q == e.entry.eseq && k != e.entry.kind);
        assert!(!clash, "identity {} reused", e.entry.eseq);
    }
    // Stated: a gap over the destroyed numbers, or a new sequence (the
    // other layout's fresh start cleared the whole ring).
    let newest_seen = seen.iter().map(|x| x.0).max().unwrap();
    let stated = r.losses.iter().any(|l| match *l {
        record::Loss::Gap { from, to, .. } => from <= newest_seen && newest_seen <= to,
        record::Loss::Window { epoch_start, .. } => epoch_start,
        _ => false,
    });
    assert!(stated, "{:?}", r.losses);
}

/// Three writers: boots of one opener that two different later writers
/// recorded, after the opener's ring wrapped, are still ordered (by the
/// opener's numbers), never reported newest first.
#[test]
fn three_writers_order_by_opener_numbers() {
    let g = Geometry {
        region: 0..504,
        rings: vec![
            Ring { base: 0, slots: 2 },
            Ring { base: 56, slots: 8 },
            Ring {
                base: 280,
                slots: 8,
            },
        ],
    };
    let mut m = Mem::new(504, 0, 7);
    let mut s = Writer::open(&RecV1, &m, &g, 0, None, 0);
    s.append(&mut m, 1, 0xA, [0; 3]).unwrap();
    let w1 = Writer::open(&RecV1, &m, &g, 1, Some(s.handoff()), 0);
    let mut w2 = Writer::open(&RecV1, &m, &g, 2, Some(w1.handoff()), 0);
    w2.append(&mut m, 1, 0xA, [0; 3]).unwrap();
    let mut s = Writer::open(&RecV1, &m, &g, 0, None, 0);
    s.append(&mut m, 1, 0xB, [0; 3]).unwrap();
    let mut w1 = Writer::open(&RecV1, &m, &g, 1, Some(s.handoff()), 0);
    w1.append(&mut m, 1, 0xB, [0; 3]).unwrap();
    for t in 0xC..=0xE {
        let mut s = Writer::open(&RecV1, &m, &g, 0, None, 0);
        s.append(&mut m, 1, t, [0; 3]).unwrap();
    }
    let r = record::read(&RecV1, m.bytes(), &g);
    let kinds: Vec<u8> = r.boots.iter().map(|b| b.events[0].entry.kind).collect();
    // 0xC's only entry was stage0's, overwritten by its two newer boots.
    assert_eq!(kinds, vec![0xA, 0xB, 0xD, 0xE]);
    assert!(r.boots.iter().all(|b| !b.uncertain));
}

/// Up to the writer limit: sixteen rings, every stage recording.
#[test]
fn sixteen_writers() {
    let c = Config {
        more: vec![3 * V1_SIZE; 14],
        ..cfg(Survives::Power, true, 3, 4)
    };
    let all = |end| Op::Boot {
        s0: stage(V1, 1, Next),
        bc: stage(V1, 2, Next),
        more: (0..14).map(|i| stage(V1, (i % 3) as u8, end)).collect(),
        app: V1,
        cut: None,
        power: false,
    };
    let ops: Vec<Op> = (0..12)
        .map(|k| all(if k % 4 == 3 { Halt } else { Next }))
        .collect();
    ok::<RecordWorld>(&c, &ops);
}

/// One carrier list: the specification's carrier line names exactly the
/// model's `identity::CARRIERS`.
#[test]
fn carrier_list_matches_the_spec() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/retained-formats.md"
    ))
    .unwrap();
    let text = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = text
        .split("Carriers: ")
        .nth(1)
        .and_then(|t| t.split(" (the model").next())
        .expect("carrier line");
    let named: Vec<&str> = line.split(", ").map(|c| c.trim_matches('`')).collect();
    let model: Vec<&str> = identity::CARRIERS.iter().map(|c| c.0).collect();
    assert_eq!(named, model);
}

/// Found by the shrinker: run order takes precedence over numbers; two
/// boots of one opener more than 2^31 of its appends apart, whose entries
/// the opener no longer holds, are still ordered by a later writer's runs.
#[test]
fn regression_run_order_before_numbers() {
    let c = Config {
        survives: Survives::ResetRandom,
        stage0: false,
        s0_bytes: 8 * V1_SIZE,
        bc_bytes: 8 * V1_SIZE,
        more: vec![4 * V1_SIZE, 3 * V1_SIZE],
        gap: 0,
        weak: false,
    };
    let b = |w1, w1e, w2e, w2end| Op::Boot {
        s0: stage(V1, 1, Next),
        bc: stage(Old, 1, Next),
        more: vec![stage(w1, w1e, Next), stage(V1, w2e, w2end)],
        app: V1,
        cut: None,
        power: false,
    };
    let ops = vec![
        b(V1, 2, 3, Hang),
        Op::FastForward {
            boots: 1_073_741_832,
            linked: true,
        },
        b(V1, 3, 1, Next),
        b(V2, 2, 1, Next),
    ];
    assert!(check::<RecordWorld>(&c, &ops, 0x61bb_21c6_ccfb_6d86).is_ok());
}

/// Found by the trial world: a reset between the two stores of a trial
/// cell update left a pair that counted 0, so the start went uncounted and
/// a broken core could be tried for ever (C1). The cell now repeats one
/// self-checking word and counts the larger of the two.
#[test]
fn regression_trial_cell_update_cut_between_its_words() {
    let c = wtr::Config { max: 5 };
    let ops = [
        wtr::Op::SetPending { good: false },
        wtr::Op::Boot {
            fault: Some(wtr::Fault::Reset { at: 0 }),
        },
    ];
    ok::<TrialWorld>(&c, &ops);
}
