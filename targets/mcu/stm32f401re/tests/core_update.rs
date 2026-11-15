//! Boot core A/B self-update over the F401 layout: the slot store
//! (`KindSStore` with the boot core slots, one log for both targets) takes
//! a new core into the idle core slot in the agent's order (erase, body,
//! header last, SLOT_PENDING); stage0's choice (`bootcore::decide`, the
//! signature included) and the started core's settle (`bootcore::settle_log`)
//! run over the same flash; a power cut lands at every flash operation of
//! the whole update. After any cut a boot core starts, the app's records
//! are intact, and the log ends with one confirmed core and nothing pending.
//!
//! With one boot core slot the app writes that slot in place: a power cut
//! anywhere in the update, and a reset or power loss between the new
//! core's starts, ends with the old core, the new core confirmed through
//! its trial, or (no core left to start) stage0 starting the confirmed app,
//! which writes the core again.

use dagon_agent::{SlotStore, Transition};
use dagon_core::bootcore::{self, Boot, MustReset, Settled, CLEARED};
use dagon_core::log::{trial_room, Log, SlotRef};
use dagon_ed25519::Ed25519Small;
use dagon_format::header::*;
use dagon_format::statelog::{kind, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::{KindSStore, ERASE_REGIONS, TRIAL_ATTEMPTS};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::{target, SlotState};
use ed25519_compact::{KeyPair, Seed};
use std::cell::RefCell;
use std::rc::Rc;

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
/// A core with this minor version fails its own check.
const BROKEN: u8 = 9;

/// The F401's flash with a power cut: operations from number `cut` on do
/// nothing and fail (the one at `cut` lands half when `half`).
#[derive(Clone)]
struct Cut {
    mem: Rc<RefCell<Vec<u8>>>,
    ops: Rc<RefCell<usize>>,
    cut: Option<(usize, bool)>,
}

impl Cut {
    fn new() -> Self {
        Cut {
            mem: Rc::new(RefCell::new(vec![0xFF; MIN_FLASH_SIZE])),
            ops: Rc::default(),
            cut: None,
        }
    }
    fn powered(&self) -> bool {
        self.cut.is_none_or(|(n, _)| *self.ops.borrow() <= n)
    }
    /// Counts one operation: `Ok(true)` it lands whole, `Ok(false)` half (the
    /// cut lands inside it), `Err` not at all (the power is gone).
    fn op(&mut self) -> Result<bool, FlashError> {
        let n = *self.ops.borrow();
        *self.ops.borrow_mut() += 1;
        match self.cut {
            Some((c, _)) if n > c => Err(FlashError::Hw),
            Some((c, half)) if n == c => {
                if half {
                    Ok(false)
                } else {
                    Err(FlashError::Hw)
                }
            }
            _ => Ok(true),
        }
    }
    fn at(addr: u32) -> usize {
        (addr - STAGE0) as usize
    }
}

impl Flash for Cut {
    fn geometry(&self) -> &[FlashRegion] {
        &FLASH_REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let a = Self::at(addr);
        buf.copy_from_slice(&self.mem.borrow()[a..a + buf.len()]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let whole = self.op()?;
        let a = Self::at(addr);
        let n = if whole {
            len as usize
        } else {
            len as usize / 2
        };
        self.mem.borrow_mut()[a..a + n].fill(0xFF);
        if whole {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let whole = self.op()?;
        let a = Self::at(addr);
        let n = if whole {
            data.len()
        } else {
            data.len() / 2 / 4 * 4
        };
        self.mem.borrow_mut()[a..a + n].copy_from_slice(&data[..n]);
        if whole {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }
}

fn signed(kind_: u8, slot: u8, base: u32, minor: u8, len: usize) -> Vec<u8> {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let mut body: Vec<u8> = (0..len as u32).map(|i| (i as u8) ^ minor).collect();
    // A vector table: the stack top, then a reset vector inside the body.
    body[..4].copy_from_slice(&RAM.to_le_bytes());
    body[4..8].copy_from_slice(&((base + HEADER_LEN + 0x101) | 1).to_le_bytes());
    let (flags, caps) = if kind_ == image_kind::BOOTCORE {
        (build_flags::VERIFY_ENFORCED, capabilities::SIG_VERIFIER)
    } else {
        (0, capabilities::CONFIRM_BY_HUB)
    };
    let mut h = ImageHeader {
        image_kind: kind_,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: MCU_ID,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: base + HEADER_LEN,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: caps,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], &body].concat()
}

fn head(img: &[u8], slot: u8) -> SlotRef {
    let raw: [u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
    SlotRef {
        slot,
        version: ImageHeader::parse(&raw).unwrap().version,
        digest16: header_digest16(&raw),
    }
}

/// Core A and app A installed, the log provisioned for both, as the
/// first install writes them.
fn installed() -> Cut {
    let mut f = Cut::new();
    let core = signed(image_kind::BOOTCORE, 0, CORE_SLOTS[0].0, 0, 3000);
    let app = signed(image_kind::APP, 0, SLOTS.slots[0].0, 0, 2000);
    f.write(CORE_SLOTS[0].0, &core).unwrap();
    f.write(SLOTS.slots[0].0, &app).unwrap();
    f.write(LOG.faces[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let mut log = Log::load(&mut f, &LOG).unwrap();
    for k in [kind::PROVISION, kind::CONFIRMED] {
        log.append_core(&mut f, k, head(&core, 0), 0).unwrap();
        log.append(&mut f, k, head(&app, 0), 0).unwrap();
    }
    *f.ops.borrow_mut() = 0;
    f
}

/// [`installed`] with the shared log's face so full (APP records) that
/// the store's reservation compacts it during the update: erase of the
/// other face, both targets' records carried, the face header last.
fn installed_full() -> Cut {
    let mut f = installed();
    let app = signed(image_kind::APP, 0, SLOTS.slots[0].0, 0, 2000);
    let mut log = Log::load(&mut f, &LOG).unwrap();
    while log.room() >= trial_room(TRIAL_ATTEMPTS) {
        log.append(&mut f, kind::CONFIRMED, head(&app, 0), 0)
            .unwrap();
    }
    *f.ops.borrow_mut() = 0;
    f
}

/// What the app's agent does at start (`KindSStore::keep_room`).
fn app_start(f: &Cut) {
    if let Ok(s) = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS) {
        let _ = s.with_cores(&CORE_SLOTS, true).keep_room();
    }
}

fn pending(f: &mut Cut) -> bool {
    bootcore::read_state(f, &LOG).is_some_and(|c| c.pending.is_some())
}

/// Boots, starts the app (which restores the log's room) and reboots
/// until the boot core trial is settled (0xFF: the power went).
fn until_settled(f: &mut Cut, cell: &mut [u32; 2]) -> u8 {
    for _ in 0..4 {
        let s = boot(f, cell);
        if s == 0xFF {
            return s;
        }
        app_start(f);
        if !f.powered() {
            return 0xFF;
        }
        if !pending(f) {
            return s;
        }
    }
    panic!("the boot core trial stays open");
}

/// The agent's XFER and SET_PENDING with target BOOTCORE through the store.
fn agent_writes(f: &Cut, img: &[u8]) -> Option<()> {
    let t = Transition::for_store_tests();
    let mut s = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS)
        .ok()?
        .with_cores(&CORE_SLOTS, true);
    let st = s.target_status(target::BOOTCORE)?;
    assert_eq!((st.active, st.confirmed), (0, 0));
    let body = &img[HEADER_SIZE..];
    let hdr: &[u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
    s.erase(target::BOOTCORE, 1, body.len() as u32, &t).ok()?;
    for (i, c) in body.chunks(256).enumerate() {
        s.write(target::BOOTCORE, 1, (i * 256) as u32, c, &t).ok()?;
    }
    s.write_target_header(target::BOOTCORE, 1, hdr, &t).ok()?;
    s.set_target_pending(target::BOOTCORE, 1, hdr, &t).ok()?;
    Some(())
}

fn healthy(f: &mut Cut, slot: u8) -> bool {
    let mut raw = [0u8; HEADER_SIZE];
    f.read(CORE_SLOTS[usize::from(slot)].0, &mut raw).unwrap();
    ImageHeader::parse(&raw).unwrap().version.minor() != BROKEN
}

/// Resets until a core settles; the running core's slot (0xFF: the power
/// went).
fn boot(f: &mut Cut, cell: &mut [u32; 2]) -> u8 {
    for _ in 0..10 {
        if !f.powered() {
            return 0xFF;
        }
        let at = bootcore::Stage0 {
            log: &LOG,
            slots: &CORE_SLOTS,
            max: TRIAL_ATTEMPTS,
            mcu_id: MCU_ID,
        };
        let verifier = Ed25519Small::new(*KeyPair::from_seed(Seed::new(SEED)).pk);
        let start = bootcore::decide(&mut f.clone(), &at, *cell, &verifier, &mut bootcore::NoDiag)
            .expect("a boot core starts");
        *cell = start.cell;
        let ok = healthy(f, start.slot);
        match bootcore::settle_log(
            f,
            &LOG,
            start.slot,
            bootcore::Cell {
                value: *cell,
                max: TRIAL_ATTEMPTS,
                holds: true,
            },
            ok,
        ) {
            Ok(s) => {
                if s != Settled::Nothing {
                    *cell = CLEARED;
                }
                return start.slot;
            }
            Err(MustReset) => continue,
        }
    }
    panic!("no core settled in 10 resets");
}

fn update(minor: u8, cut: Option<(usize, bool)>) -> (Cut, u8) {
    update_from(installed(), minor, cut)
}

fn update_from(mut f: Cut, minor: u8, cut: Option<(usize, bool)>) -> (Cut, u8) {
    f.cut = cut;
    let img = signed(image_kind::BOOTCORE, 1, CORE_SLOTS[1].0, minor, 3000);
    let mut cell = CLEARED;
    if agent_writes(&f, &img).is_some() && f.powered() {
        until_settled(&mut f, &mut cell);
    }
    f.cut = None;
    *f.ops.borrow_mut() = 0;
    let mut cell = CLEARED;
    let running = until_settled(&mut f, &mut cell);
    (f, running)
}

#[test]
fn a_new_core_is_tried_confirms_itself_and_the_app_records_stay() {
    let (f, running) = update(1, None);
    assert_eq!(running, 1);
    let mut s = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, true);
    let c = s.target_status(target::BOOTCORE).unwrap();
    assert_eq!((c.active, c.confirmed), (1, 1));
    assert_eq!(
        (c.slots[0].state, c.slots[1].state),
        (SlotState::Written, SlotState::Confirmed)
    );
    let a = s.status();
    assert_eq!((a.active, a.confirmed), (0, 0), "the app is untouched");
}

/// A node whose trial cell does not hold takes no boot core: stage0 could
/// not count its starts.
#[test]
fn without_a_holding_trial_cell_no_boot_core_is_taken() {
    let f = installed();
    let s = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, false);
    assert_eq!(s.capacity(target::BOOTCORE, 1), 0);
    assert!(s.capacity(target::APP, 1) > 0);
    let s = KindSStore::load(f, &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, true);
    assert!(s.capacity(target::BOOTCORE, 1) > 0);
}

#[test]
fn a_core_that_fails_its_check_falls_back_to_the_old_one() {
    let (f, running) = update(BROKEN, None);
    assert_eq!(running, 0);
    let mut s = KindSStore::load(f, &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, true);
    let c = s.target_status(target::BOOTCORE).unwrap();
    assert_eq!((c.confirmed, c.slots[1].state), (0, SlotState::Reverted));
    assert_eq!(
        c.last_reason,
        dagon_format::statelog::reason::TRIAL_EXHAUSTED
    );
}

#[test]
fn a_power_cut_at_every_flash_step_leaves_a_bootable_core() {
    for (minor, full) in [(1, false), (BROKEN, false), (1, true), (BROKEN, true)] {
        let start = || if full { installed_full() } else { installed() };
        let ops = {
            let f = start();
            let img = signed(image_kind::BOOTCORE, 1, CORE_SLOTS[1].0, minor, 3000);
            agent_writes(&f, &img).unwrap();
            let gen = Log::load(&mut f.clone(), &LOG).unwrap().generation;
            assert_eq!(gen > 1, full, "the full log compacts during the update");
            let mut g = f.clone();
            until_settled(&mut g, &mut CLEARED.clone());
            let n = *g.ops.borrow();
            n
        };
        assert!(ops > 10, "{ops}");
        for n in 0..ops {
            for half in [false, true] {
                let (f, running) = update_from(start(), minor, Some((n, half)));
                let mut s = KindSStore::load(f, &LOG, SLOTS, &ERASE_REGIONS)
                    .unwrap()
                    .with_cores(&CORE_SLOTS, true);
                let c = s.target_status(target::BOOTCORE).unwrap();
                let open = c
                    .slots
                    .iter()
                    .any(|x| matches!(x.state, SlotState::Pending | SlotState::Trying));
                assert!(!open, "cut {n}/{half} 1.{minor}: trial left open");
                assert_eq!(c.confirmed, running, "cut {n}/{half}");
                assert!(minor != BROKEN || running == 0, "broken core confirmed");
                let a = s.status();
                assert_eq!(a.confirmed, 0, "cut {n}/{half}: app record lost");
            }
        }
    }
}

/// Every target and slot the store receives into has the definition's one
/// planner layout, with the store's capacity; the largest body of each is
/// one sector, one flash operation (two of which every watchdog period
/// covers, as the definition check requires).
#[test]
fn every_slot_the_store_takes_has_one_planner_layout() {
    use dagon_agent::erase_plan;
    let s = KindSStore::load(installed(), &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, true);
    let layout = s.erase_layout().unwrap();
    for t in 0..=u8::MAX {
        for slot in 0..=u8::MAX {
            let cap = s.capacity(t, slot);
            let layouts: Vec<_> = layout
                .slots
                .iter()
                .filter(|l| (l.target, l.slot) == (t, slot))
                .collect();
            match (cap, layouts.as_slice()) {
                (0, []) => {}
                (cap, [l]) => {
                    assert_eq!(l.capacity, cap, "{t}/{slot}");
                    let start = match t {
                        target::APP => SLOTS.slots[usize::from(slot)].0,
                        _ => CORE_SLOTS[usize::from(slot)].0,
                    };
                    let inline = dagon_port::timeout::HeaderPlace::Inline;
                    assert_eq!((l.start, l.header), (start, inline), "{t}/{slot}");
                    assert_eq!(erase_plan(&layout, t, slot, cap).unwrap().count(), 1);
                }
                _ => panic!("{t}/{slot}: capacity {cap}, layouts {layouts:?}"),
            }
        }
    }
}

/// Regions a board returns in place of the MCU's: one slower part erased
/// in 64 KiB units over the boot core and APP sectors.
static BOARD_REGIONS: [dagon_port::timeout::EraseRegion; 1] = [dagon_port::timeout::EraseRegion {
    start: 0x0801_0000,
    len: 0x7_0000,
    unit: 0x1_0000,
    unit_max_ms: 3_000,
}];

/// The store plans over the regions its board returns: another part
/// changes the plan it erases.
#[test]
fn the_store_plans_over_its_boards_regions() {
    use dagon_agent::erase_plan;
    use dagon_port::timeout::EraseOp;
    let plan = |regions: &'static [dagon_port::timeout::EraseRegion]| -> Vec<EraseOp> {
        let s = KindSStore::load(installed(), &LOG, SLOTS, regions).unwrap();
        let layout = s.erase_layout().unwrap();
        erase_plan(&layout, target::APP, 1, 0x1_0000)
            .unwrap()
            .collect()
    };
    let op = |addr, len, max_ms| EraseOp { addr, len, max_ms };
    assert_eq!(plan(&ERASE_REGIONS), [op(0x0804_0000, 0x2_0000, 4_000)]);
    assert_eq!(
        plan(&BOARD_REGIONS),
        [
            op(0x0804_0000, 0x1_0000, 3_000),
            op(0x0805_0000, 0x1_0000, 3_000)
        ]
    );
}

// One boot core slot, written in place by the app.

static ONE_CORE: [(u32, u32); 1] = [CORE_SLOTS[0]];
const TAG: [u8; 16] = target_tag("seg-f401-7seg");

/// What a stage0 built for a node with one boot core slot and this tag
/// knows (the package's, with its slot list and tag).
const ONE: bootcore::Stage0<'static> = bootcore::Stage0 {
    slots: &ONE_CORE,
    ..STAGE0_AT
};
const APPS: bootcore::Apps<'static> = bootcore::Apps {
    target_tag: TAG,
    ..STAGE0_APPS
};

/// What runs once the resets end: the core whose minor version is given
/// (it settled its trial, or there was none), or the app stage0 started
/// for want of a core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ran {
    Core(u8),
    App,
}

/// The agent's XFER and SET_PENDING with target BOOTCORE into the only
/// slot, through the store.
fn write_only_core(f: &Cut, img: &[u8]) -> Option<()> {
    let t = Transition::for_store_tests();
    let mut s = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS)
        .ok()?
        .with_cores(&ONE_CORE, true);
    assert_eq!(s.capacity(target::BOOTCORE, 1), 0, "one boot core slot");
    let body = &img[HEADER_SIZE..];
    let hdr: &[u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
    s.erase(target::BOOTCORE, 0, body.len() as u32, &t).ok()?;
    for (i, c) in body.chunks(256).enumerate() {
        s.write(target::BOOTCORE, 0, (i * 256) as u32, c, &t).ok()?;
    }
    s.write_target_header(target::BOOTCORE, 0, hdr, &t).ok()?;
    s.set_target_pending(target::BOOTCORE, 0, hdr, &t).ok()?;
    Some(())
}

/// What the app's agent does at start, over the one-slot store.
fn one_core_app_start(f: &Cut) {
    if let Ok(s) = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS) {
        let _ = s.with_cores(&ONE_CORE, true).keep_room();
    }
}

/// Stage0 and the boot core through the resets a failing core causes
/// (each keeps the trial cell; after `lose` of them the power goes and
/// comes back, clearing it). `None`: the flash lost its power.
fn one_core_boot(f: &mut Cut, cell: &mut [u32; 2], lose: Option<usize>) -> Option<Ran> {
    let verifier = Ed25519Small::new(*KeyPair::from_seed(Seed::new(SEED)).pk);
    for resets in 0..12 {
        if !f.powered() {
            return None;
        }
        if lose == Some(resets) {
            *cell = CLEARED;
        }
        let before = *cell;
        match bootcore::start(
            &mut f.clone(),
            &ONE,
            &APPS,
            *cell,
            &verifier,
            &mut bootcore::NoDiag,
        ) {
            Ok(Boot::App { slot, body }) => {
                assert_eq!((slot, body), (0, SLOTS.slots[0].0 + HEADER_LEN));
                assert_eq!(*cell, before, "the trial cell is left as it is");
                return Some(Ran::App);
            }
            Ok(Boot::Core(start)) => {
                *cell = start.cell;
                let ok = healthy(f, start.slot);
                let settled = bootcore::settle_log(
                    f,
                    &LOG,
                    start.slot,
                    bootcore::Cell {
                        value: *cell,
                        max: TRIAL_ATTEMPTS,
                        holds: true,
                    },
                    ok,
                );
                match settled {
                    Ok(s) => {
                        if s != Settled::Nothing {
                            *cell = CLEARED;
                        }
                        return Some(Ran::Core(core_minor(f)));
                    }
                    Err(MustReset) => continue,
                }
            }
            Err(no) => panic!("stage0 halted: {no:?}"),
        }
    }
    panic!("no core settled and no app started in 12 resets");
}

fn core_minor(f: &mut Cut) -> u8 {
    let mut raw = [0u8; HEADER_SIZE];
    f.read(ONE_CORE[0].0, &mut raw).unwrap();
    ImageHeader::parse(&raw).unwrap().version.minor()
}

/// Boots, starts the app and reboots until the boot core trial is settled
/// or stage0 started the app.
fn one_core_settled(f: &mut Cut, lose: Option<usize>) -> Option<Ran> {
    let mut cell = CLEARED;
    for _ in 0..4 {
        let ran = one_core_boot(f, &mut cell, lose)?;
        one_core_app_start(f);
        if !f.powered() {
            return None;
        }
        if ran == Ran::App || !pending(f) {
            return Some(ran);
        }
    }
    panic!("the boot core trial stays open");
}

/// The update to core 1.`minor` in the only slot with the power cut at
/// operation `cut` (or a power loss after `lose` starts); then the power
/// comes back and the node boots. What runs then.
fn one_core_update(
    full: bool,
    minor: u8,
    cut: Option<(usize, bool)>,
    lose: Option<usize>,
) -> (Cut, Ran) {
    let mut f = if full { installed_full() } else { installed() };
    f.cut = cut;
    let img = signed(image_kind::BOOTCORE, 0, ONE_CORE[0].0, minor, 3000);
    if write_only_core(&f, &img).is_some() && f.powered() {
        one_core_settled(&mut f, lose);
    }
    f.cut = None;
    *f.ops.borrow_mut() = 0;
    let ran = one_core_settled(&mut f, None).expect("powered");
    (f, ran)
}

/// The confirmed boot core and app records.
fn confirmed(f: &mut Cut) -> (Option<SlotRef>, Option<SlotRef>) {
    let l = Log::load(f, &LOG).unwrap();
    (l.core.confirmed, l.state.confirmed)
}

/// What every way the update ends must hold: the old core runs, or the new
/// one confirmed through its trial, or stage0 started the confirmed app
/// and that app writes a core that then runs; the app's record stays.
fn ends_well(mut f: Cut, ran: Ran, minor: u8, what: &str) {
    let app = signed(image_kind::APP, 0, SLOTS.slots[0].0, 0, 2000);
    assert_eq!(
        confirmed(&mut f).1,
        Some(head(&app, 0)),
        "{what}: app record"
    );
    match ran {
        Ran::Core(0) => {}
        Ran::Core(m) => {
            assert_eq!(m, minor, "{what}");
            assert_ne!(m, BROKEN, "{what}: a failing core was confirmed");
            let new = signed(image_kind::BOOTCORE, 0, ONE_CORE[0].0, m, 3000);
            assert_eq!(confirmed(&mut f).0, Some(head(&new, 0)), "{what}");
            assert!(!pending(&mut f), "{what}: trial left open");
        }
        Ran::App => {
            let again = signed(image_kind::BOOTCORE, 0, ONE_CORE[0].0, 2, 3000);
            write_only_core(&f, &again).expect("the app writes the core again");
            let ran = one_core_settled(&mut f, None).unwrap();
            assert_eq!(ran, Ran::Core(2), "{what}: the rewritten core runs");
            assert_eq!(confirmed(&mut f).0, Some(head(&again, 0)), "{what}");
        }
    }
}

#[test]
fn the_app_writes_the_only_core_slot_and_the_new_core_is_tried() {
    let (f, ran) = one_core_update(false, 1, None, None);
    assert_eq!(ran, Ran::Core(1));
    ends_well(f, ran, 1, "uncut");
    let (f, ran) = one_core_update(false, BROKEN, None, None);
    assert_eq!(ran, Ran::App, "no core left: stage0 starts the app");
    ends_well(f, ran, BROKEN, "failing core");
}

#[test]
fn a_power_cut_anywhere_in_a_one_slot_core_update_ends_in_a_core_or_the_app() {
    for full in [false, true] {
        for minor in [1, BROKEN] {
            let ops = {
                let mut f = if full { installed_full() } else { installed() };
                let img = signed(image_kind::BOOTCORE, 0, ONE_CORE[0].0, minor, 3000);
                write_only_core(&f, &img).unwrap();
                one_core_settled(&mut f, None).unwrap();
                let n = *f.ops.borrow();
                n
            };
            assert!(ops > 10, "{ops}");
            let mut seen = Vec::new();
            for n in 0..=ops {
                for half in [false, true] {
                    let (f, ran) = one_core_update(full, minor, Some((n, half)), None);
                    ends_well(
                        f,
                        ran,
                        minor,
                        &format!("full {full} 1.{minor} cut {n}/{half}"),
                    );
                    seen.push(ran);
                }
            }
            // The cuts reach every way the update can end.
            assert!(seen.contains(&Ran::App), "full {full} 1.{minor}");
            assert!(seen.contains(&Ran::Core(0)), "full {full} 1.{minor}");
            if minor != BROKEN {
                assert!(seen.contains(&Ran::Core(minor)), "full {full}");
            }
        }
    }
}

/// A power loss after each start of the new core (the trial cell lost)
/// starts its trial over; it still ends in the new core or the app.
#[test]
fn a_power_loss_between_the_new_cores_starts_ends_in_a_core_or_the_app() {
    for minor in [1, BROKEN] {
        for lose in 1..=usize::from(TRIAL_ATTEMPTS) {
            let (f, ran) = one_core_update(false, minor, None, Some(lose));
            ends_well(f, ran, minor, &format!("1.{minor} lost after {lose}"));
        }
    }
}
