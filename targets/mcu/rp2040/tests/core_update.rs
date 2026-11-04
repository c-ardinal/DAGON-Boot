//! Boot core A/B self-update over the hub's real layout: the agent's writes
//! (idle boot core slot, header last, SLOT_PENDING), stage0's choice
//! (`bl::choose`) with the trial cell, the started core's settle
//! (`bl::settle_core`), and a power cut at every flash operation of the
//! whole update, for a new core that works and one that fails its own
//! check. After any cut a boot core starts, and the log ends with one
//! confirmed core and nothing pending.

use dagon_core::bootcore::CLEARED;
use dagon_core::log::{trial_room, Log, SlotRef};
use dagon_format::header::*;
use dagon_format::statelog::{kind, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_rp2040::bl::{self, Settled};
use dagon_mcu_rp2040::image;
use dagon_mcu_rp2040::layout::*;
use dagon_port::{Flash, FlashRegion};
use dagon_sim::{CutPoint, SimFlash};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
/// A core with this minor version fails its own check (stands for a core
/// that cannot start the app, or crashes before it settles).
const BROKEN: u8 = 9;

fn flash() -> SimFlash {
    SimFlash::new(
        &[FlashRegion {
            start: XIP_BASE,
            len: MIN_FLASH_SIZE as u32,
            erase_size: SECTOR,
            write_size: 1,
            erased_value: 0xFF,
        }],
        7,
    )
}

fn bootcore(slot: u8, minor: u8) -> Vec<u8> {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let body: Vec<u8> = (0..3000u32).map(|i| (i as u8) ^ minor).collect();
    let mut h = ImageHeader {
        image_kind: image_kind::BOOTCORE,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::RP2040,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: BL_SLOT[slot as usize] + HEADER_LEN,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: build_flags::VERIFY_ENFORCED,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: target_tag("hub-rp2040-lcd"),
        proto_version: 1,
        capabilities: capabilities::SIG_VERIFIER,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], &body].concat()
}

/// Core A installed and recorded, as the install UF2 leaves the flash.
fn installed() -> SimFlash {
    installed_with(0)
}

/// [`installed`] with the boot core log's face so full that the agent's
/// reservation compacts it into the other face during the update.
fn installed_full() -> SimFlash {
    let room = trial_room(dagon_mcu_rp2040::TRIAL_ATTEMPTS);
    installed_with(LOG_FACE_SIZE / 32 - 1 - 2 - room + 1)
}

fn installed_with(extra: u32) -> SimFlash {
    let mut f = flash();
    let a = bootcore(0, 0);
    f.write(BL_SLOT[0], &a).unwrap();
    f.write(BL_LOG[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let r = image::head_ref(&mut f, BL_SLOT[0], 0).unwrap();
    let mut log = Log::load(&mut f, &BL_LOG_LAYOUT).unwrap();
    log.append_core(&mut f, kind::PROVISION, r, 0).unwrap();
    log.append_core(&mut f, kind::CONFIRMED, r, 0).unwrap();
    for _ in 0..extra {
        log.append_core(&mut f, kind::CONFIRMED, r, 0).unwrap();
    }
    f.clear_ops();
    f
}

/// What the app's agent does at start (`AppStore::keep_room`): the boot
/// core log gets its trial room back, outside any trial.
fn app_start(f: &mut SimFlash) {
    if let Ok(mut log) = Log::load(f, &BL_LOG_LAYOUT) {
        let _ = log.reserve(f, trial_room(dagon_mcu_rp2040::TRIAL_ATTEMPTS));
    }
}

/// What the agent does for XFER + SET_PENDING with target BOOTCORE, in its
/// order: erase what the image spans, body, header last, then SLOT_PENDING.
fn agent_writes(f: &mut SimFlash, slot: u8, img: &[u8]) -> Option<()> {
    let base = BL_SLOT[slot as usize];
    let mut at = base;
    while at < base + sectors(img.len() as u32) {
        f.erase(at, SECTOR).ok()?;
        at += SECTOR;
    }
    f.write(base + HEADER_LEN, &img[HEADER_LEN as usize..])
        .ok()?;
    f.write(base, &img[..HEADER_LEN as usize]).ok()?;
    let r = image::head_ref(f, base, slot)?;
    let mut log = Log::load(f, &BL_LOG_LAYOUT).ok()?;
    log.reserve(f, trial_room(dagon_mcu_rp2040::TRIAL_ATTEMPTS))
        .ok()?;
    log.append_core(f, kind::SLOT_PENDING, r, 0).ok()
}

/// Whether the core stage0 started passes its own check: what the boot
/// core main computes from its app decision and its verifier.
fn healthy(f: &mut SimFlash, slot: u8) -> bool {
    let h = image::head_ref(f, BL_SLOT[slot as usize], slot).unwrap();
    h.version.minor() != BROKEN
}

/// Resets until a core settles and would start the app (each reset keeps
/// the trial cell, as a watchdog reset does); the running core's slot.
/// Stops early (returning slot 0xFF) when the power goes.
fn boot(f: &mut SimFlash, cell: &mut [u32; 2]) -> u8 {
    for _ in 0..10 {
        if !f.is_powered() {
            return 0xFF;
        }
        let start = bl::choose(f, *cell).expect("a boot core starts");
        *cell = start.cell;
        let ok = healthy(f, start.slot);
        match bl::settle_core(f, start.slot, (*cell, true), ok) {
            Ok(s) => {
                if s != Settled::Nothing {
                    *cell = CLEARED;
                }
                return start.slot;
            }
            // The core resets; stage0 counts the start.
            Err(bl::MustReset) => continue,
        }
    }
    panic!("no core settled in 10 resets");
}

/// Boots, starts the app (which restores the log's room) and reboots
/// until the boot core trial is settled; the running core's slot (0xFF:
/// the power went).
fn until_settled(f: &mut SimFlash, cell: &mut [u32; 2]) -> u8 {
    for _ in 0..4 {
        let s = boot(f, cell);
        if s == 0xFF {
            return s;
        }
        app_start(f);
        if f.is_powered() && core_log(f).1.is_none() {
            return s;
        }
        if !f.is_powered() {
            return 0xFF;
        }
    }
    panic!("the boot core trial stays open");
}

fn core_log(f: &mut SimFlash) -> (Option<SlotRef>, Option<SlotRef>) {
    let l = Log::load(f, &BL_LOG_LAYOUT).unwrap().core;
    (l.confirmed, l.pending)
}

#[test]
fn a_new_core_is_tried_and_confirms_itself() {
    let mut f = installed();
    agent_writes(&mut f, 1, &bootcore(1, 1)).unwrap();
    assert_eq!(
        core_log(&mut f).1.map(|p| p.slot),
        Some(1),
        "recorded pending"
    );
    let mut cell = CLEARED;
    let start = bl::choose(&mut f, cell).unwrap();
    assert_eq!((start.slot, start.trial), (1, true), "stage0 tries it");
    assert_eq!(
        bl::settle_core(&mut f, 1, (start.cell, true), false),
        Err(bl::MustReset),
        "no confirmation before its own check passes"
    );
    assert_eq!(boot(&mut f, &mut cell), 1);
    let (c, p) = core_log(&mut f);
    assert_eq!((c.map(|c| c.slot), p), (Some(1), None));
    let again = bl::choose(&mut f, cell).unwrap();
    assert_eq!((again.slot, again.trial), (1, false));
}

#[test]
fn a_core_that_fails_its_check_falls_back_after_the_trial_starts() {
    let mut f = installed();
    agent_writes(&mut f, 1, &bootcore(1, BROKEN)).unwrap();
    let mut cell = CLEARED;
    let mut tries = 0;
    loop {
        let start = bl::choose(&mut f, cell).unwrap();
        cell = start.cell;
        if !start.trial {
            assert_eq!(start.slot, 0);
            break;
        }
        tries += 1;
        assert_eq!(
            bl::settle_core(&mut f, 1, (cell, true), false),
            Err(bl::MustReset)
        );
    }
    assert_eq!(tries, dagon_mcu_rp2040::TRIAL_ATTEMPTS);
    assert_eq!(
        bl::settle_core(&mut f, 0, (cell, true), true),
        Ok(Settled::RolledBack)
    );
    let l = Log::load(&mut f, &BL_LOG_LAYOUT).unwrap().core;
    assert_eq!(l.confirmed.map(|c| c.slot), Some(0));
    assert_eq!(l.reverted.map(|r| r.slot), Some(1));
    assert_eq!(
        l.last_reason,
        dagon_format::statelog::reason::TRIAL_EXHAUSTED
    );
}

#[test]
fn a_core_whose_image_does_not_check_out_is_never_started() {
    let mut f = installed();
    // The body in flash differs from the one its header names (a flash
    // fault after the agent verified it).
    let mut b = bootcore(1, 1);
    let last = b.len() - 1;
    b[last] ^= 1;
    agent_writes(&mut f, 1, &b).unwrap();
    let mut cell = CLEARED;
    let start = bl::choose(&mut f, cell).unwrap();
    assert_eq!((start.slot, start.trial), (0, false));
    cell = start.cell;
    assert_eq!(
        bl::settle_core(&mut f, 0, (cell, true), true),
        Ok(Settled::RolledBack)
    );
    assert_eq!(
        Log::load(&mut f, &BL_LOG_LAYOUT).unwrap().core.last_reason,
        dagon_format::statelog::reason::HASH_MISMATCH
    );
}

/// A face with a higher generation whose valid records name slot B, where
/// no core (or a corrupt one) is: stage0 still starts core A.
#[test]
fn a_forged_newer_face_naming_a_bad_slot_does_not_strand_the_hub() {
    use dagon_format::statelog::{target, Record};
    for corrupt_b in [false, true] {
        let mut f = installed();
        if corrupt_b {
            let mut b = bootcore(1, 1);
            let last = b.len() - 1;
            b[last] ^= 1;
            f.write(BL_SLOT[1], &b).unwrap();
        }
        let forged = |kind_: u8, seq: u32| Record {
            kind: kind_,
            target: target::BOOTCORE,
            slot: 1,
            reason: 0,
            seq,
            version: Version::new(9, 9, 9),
            digest16: [0x5A; 16],
        };
        f.erase(BL_LOG[1], SECTOR).unwrap();
        f.write(BL_LOG[1] + 32, &forged(kind::PROVISION, 1).to_bytes())
            .unwrap();
        f.write(BL_LOG[1] + 64, &forged(kind::CONFIRMED, 2).to_bytes())
            .unwrap();
        f.write(BL_LOG[1], &FaceHeader { generation: 99 }.to_bytes())
            .unwrap();
        let start = bl::choose(&mut f, CLEARED).expect("core A starts");
        assert_eq!(
            (start.slot, start.trial),
            (0, false),
            "corrupt B {corrupt_b}"
        );
    }
    // Nothing anywhere: stage0 has nothing to start (it halts).
    let mut empty = flash();
    assert!(bl::choose(&mut empty, CLEARED).is_err());
}

/// Stage0 and the boot core never erase, so no erase runs under a trial
/// watchdog: with the log's room used up by a torn record, the trial ends
/// by resets and the old core, the app's start makes room again, and the
/// trial then settles.
#[test]
fn the_boot_stage_never_erases() {
    use dagon_sim::OpKind;
    for torn in [false, true] {
        let mut f = installed();
        agent_writes(&mut f, 1, &bootcore(1, 1)).unwrap();
        if torn {
            // A record cut half-way at the face's next place.
            let log = Log::load(&mut f, &BL_LOG_LAYOUT).unwrap();
            let at = BL_LOG[0] + 32 + 32 * (127 - log.room());
            f.write(at, &[0x01, 0x01, 0x01]).unwrap();
        }
        f.clear_ops();
        let mut cell = CLEARED;
        let running = boot(&mut f, &mut cell);
        assert!(
            f.ops().iter().all(|o| o.kind == OpKind::Write),
            "torn {torn}: {:?}",
            f.ops()
        );
        if torn {
            assert_eq!(running, 0, "the trial could not be confirmed");
            assert!(core_log(&mut f).1.is_some(), "still open");
            app_start(&mut f);
            let mut cell = CLEARED;
            assert_eq!(until_settled(&mut f, &mut cell), 1);
        } else {
            assert_eq!(running, 1);
        }
    }
}

/// Runs the whole update (agent writes, then boots until settled) with a
/// power cut armed at `cut`; then power returns (the trial cell is lost)
/// and the hub boots until settled.
fn update_with_cut(minor: u8, full: bool, cut: Option<CutPoint>) -> (SimFlash, u8) {
    let mut f = if full { installed_full() } else { installed() };
    if let Some(c) = cut {
        f.arm_cut(c);
    }
    let img = bootcore(1, minor);
    let mut cell = CLEARED;
    if agent_writes(&mut f, 1, &img).is_some() && f.is_powered() {
        until_settled(&mut f, &mut cell);
    }
    f.power_on();
    let mut cell = CLEARED;
    let running = until_settled(&mut f, &mut cell);
    (f, running)
}

#[test]
fn a_power_cut_at_every_flash_step_leaves_a_bootable_core() {
    for (minor, full) in [(1, false), (BROKEN, false), (1, true), (BROKEN, true)] {
        let (mut done, running) = update_with_cut(minor, full, None);
        let ops = {
            let n = done.ops().len();
            n
        };
        let expect = if minor == BROKEN { 0 } else { 1 };
        assert_eq!(running, expect, "uncut update, core 1.{minor}");
        assert_eq!(core_log(&mut done).0.map(|c| c.slot), Some(expect));
        assert!(ops > 4, "{ops} operations");
        if full {
            let erases = done.ops().iter().filter(|o| o.addr == BL_LOG[1]).count();
            assert!(erases > 0, "the update compacts the boot core log");
        }
        for i in 0..ops {
            for cut in [CutPoint::During(i), CutPoint::After(i)] {
                let (mut f, running) = update_with_cut(minor, full, Some(cut));
                let (c, p) = core_log(&mut f);
                assert_eq!(p, None, "{cut:?} 1.{minor}: trial left open");
                let c = c.unwrap_or_else(|| panic!("{cut:?} 1.{minor}: no confirmed core"));
                assert_eq!(c.slot, running, "{cut:?} 1.{minor}");
                assert!(
                    minor != BROKEN || running == 0,
                    "{cut:?}: a broken core was confirmed"
                );
                let start = bl::choose(&mut f, CLEARED).unwrap();
                assert_eq!((start.slot, start.trial), (running, false), "{cut:?}");
            }
        }
    }
}
