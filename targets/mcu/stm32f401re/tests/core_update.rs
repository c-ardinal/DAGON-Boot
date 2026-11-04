//! Boot core A/B self-update over the F401 layout: the slot store
//! (`KindSStore` with the boot core slots, one log for both targets) takes
//! a new core into the idle core slot in the agent's order (erase, body,
//! header last, SLOT_PENDING); stage0's choice (`bootcore::choose` with
//! `check_inline`) and the started core's settle (`bootcore::settle_log`)
//! run over the same flash; a power cut lands at every flash operation of
//! the whole update. After any cut a boot core starts, the app's records
//! are intact, and the log ends with one confirmed core and nothing pending.

use dagon_agent::{SlotStore, Transition};
use dagon_core::bootcore::{self, MustReset, Settled, CLEARED};
use dagon_core::log::{Log, SlotRef};
use dagon_format::header::*;
use dagon_format::statelog::{kind, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::{KindSStore, TRIAL_ATTEMPTS};
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
    let body: Vec<u8> = (0..len as u32).map(|i| (i as u8) ^ minor).collect();
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
    let app = signed(image_kind::APP, 0, SLOTS.slots[0], 0, 2000);
    f.write(CORE_SLOTS[0].0, &core).unwrap();
    f.write(SLOTS.slots[0], &app).unwrap();
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

/// The agent's XFER and SET_PENDING with target BOOTCORE through the store.
fn agent_writes(f: &Cut, img: &[u8]) -> Option<()> {
    let t = Transition::for_store_tests();
    let mut s = KindSStore::load(f.clone(), &LOG, SLOTS)
        .ok()?
        .with_cores(CORE_SLOTS);
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
        let st = bootcore::read_state(f, &LOG).unwrap_or_default();
        let start = bootcore::choose(&st, *cell, TRIAL_ATTEMPTS, |r| {
            let s = r.map_or(0, |r| r.slot);
            bootcore::check_inline(&mut f.clone(), CORE_SLOTS[usize::from(s)], s, r, MCU_ID)
        })
        .expect("a boot core starts");
        *cell = start.cell;
        let ok = healthy(f, start.slot);
        match bootcore::settle_log(f, &LOG, start.slot, (*cell, TRIAL_ATTEMPTS), ok) {
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
    let mut f = installed();
    f.cut = cut;
    let img = signed(image_kind::BOOTCORE, 1, CORE_SLOTS[1].0, minor, 3000);
    let mut cell = CLEARED;
    if agent_writes(&f, &img).is_some() && f.powered() {
        boot(&mut f, &mut cell);
    }
    f.cut = None;
    *f.ops.borrow_mut() = 0;
    let mut cell = CLEARED;
    let running = boot(&mut f, &mut cell);
    (f, running)
}

#[test]
fn a_new_core_is_tried_confirms_itself_and_the_app_records_stay() {
    let (f, running) = update(1, None);
    assert_eq!(running, 1);
    let mut s = KindSStore::load(f.clone(), &LOG, SLOTS)
        .unwrap()
        .with_cores(CORE_SLOTS);
    let c = s.target_status(target::BOOTCORE).unwrap();
    assert_eq!((c.active, c.confirmed), (1, 1));
    assert_eq!(
        (c.slots[0].state, c.slots[1].state),
        (SlotState::Written, SlotState::Confirmed)
    );
    let a = s.status();
    assert_eq!((a.active, a.confirmed), (0, 0), "the app is untouched");
}

#[test]
fn a_core_that_fails_its_check_falls_back_to_the_old_one() {
    let (f, running) = update(BROKEN, None);
    assert_eq!(running, 0);
    let mut s = KindSStore::load(f, &LOG, SLOTS)
        .unwrap()
        .with_cores(CORE_SLOTS);
    let c = s.target_status(target::BOOTCORE).unwrap();
    assert_eq!((c.confirmed, c.slots[1].state), (0, SlotState::Reverted));
    assert_eq!(
        c.last_reason,
        dagon_format::statelog::reason::TRIAL_EXHAUSTED
    );
}

#[test]
fn a_power_cut_at_every_flash_step_leaves_a_bootable_core() {
    for minor in [1, BROKEN] {
        let (done, _) = {
            let mut f = installed();
            let img = signed(image_kind::BOOTCORE, 1, CORE_SLOTS[1].0, minor, 3000);
            agent_writes(&f, &img).unwrap();
            boot(&mut f, &mut CLEARED.clone());
            (f, 0)
        };
        let ops = *done.ops.borrow();
        assert!(ops > 10, "{ops}");
        for n in 0..ops {
            for half in [false, true] {
                let (f, running) = update(minor, Some((n, half)));
                let mut s = KindSStore::load(f, &LOG, SLOTS)
                    .unwrap()
                    .with_cores(CORE_SLOTS);
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
