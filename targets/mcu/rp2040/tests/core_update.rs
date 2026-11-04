//! Boot core A/B self-update over the hub's real layout: the agent's writes
//! (idle boot core slot, header last, SLOT_PENDING), stage0's choice
//! (`bl::choose`) with the trial cell, the started core's settle
//! (`bl::settle_core`), and a power cut at every flash operation of the
//! whole update, for a new core that works and one that fails its own
//! check. After any cut a boot core starts, and the log ends with one
//! confirmed core and nothing pending.

use dagon_core::bootcore::CLEARED;
use dagon_core::log::{Log, SlotRef};
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
    let mut f = flash();
    let a = bootcore(0, 0);
    f.write(BL_SLOT[0], &a).unwrap();
    f.write(BL_LOG[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let r = image::head_ref(&mut f, BL_SLOT[0], 0).unwrap();
    let mut log = Log::load(&mut f, &BL_LOG_LAYOUT).unwrap();
    log.append_core(&mut f, kind::PROVISION, r, 0).unwrap();
    log.append_core(&mut f, kind::CONFIRMED, r, 0).unwrap();
    f.clear_ops();
    f
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
        match bl::settle_core(f, start.slot, *cell, ok) {
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
        bl::settle_core(&mut f, 1, start.cell, false),
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
        assert_eq!(bl::settle_core(&mut f, 1, cell, false), Err(bl::MustReset));
    }
    assert_eq!(tries, dagon_mcu_rp2040::TRIAL_ATTEMPTS);
    assert_eq!(
        bl::settle_core(&mut f, 0, cell, true),
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
        bl::settle_core(&mut f, 0, cell, true),
        Ok(Settled::RolledBack)
    );
    assert_eq!(
        Log::load(&mut f, &BL_LOG_LAYOUT).unwrap().core.last_reason,
        dagon_format::statelog::reason::HASH_MISMATCH
    );
}

/// Runs the whole update (agent writes, then boots until settled) with a
/// power cut armed at `cut`; then power returns (the trial cell is lost)
/// and the hub boots until settled.
fn update_with_cut(minor: u8, cut: Option<CutPoint>) -> (SimFlash, u8, bool) {
    let mut f = installed();
    if let Some(c) = cut {
        f.arm_cut(c);
    }
    let img = bootcore(1, minor);
    let mut cell = CLEARED;
    let wrote = agent_writes(&mut f, 1, &img).is_some() && f.is_powered();
    if f.is_powered() {
        boot(&mut f, &mut cell);
    }
    let lost = !f.is_powered();
    f.power_on();
    let mut cell = CLEARED;
    let running = boot(&mut f, &mut cell);
    (f, running, wrote && !lost)
}

#[test]
fn a_power_cut_at_every_flash_step_leaves_a_bootable_core() {
    for minor in [1, BROKEN] {
        let (mut done, running, _) = update_with_cut(minor, None);
        let ops = {
            let n = done.ops().len();
            n
        };
        let expect = if minor == BROKEN { 0 } else { 1 };
        assert_eq!(running, expect, "uncut update, core 1.{minor}");
        assert_eq!(core_log(&mut done).0.map(|c| c.slot), Some(expect));
        assert!(ops > 4, "{ops} operations");
        for i in 0..ops {
            for cut in [CutPoint::During(i), CutPoint::After(i)] {
                let (mut f, running, _) = update_with_cut(minor, Some(cut));
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
