//! The F401's flash with a power cut at any flash operation, and the signed
//! images and first install the power-cut tests start from.
#![allow(dead_code)]

use dagon_core::log::{trial_room, Log, SlotRef};
use dagon_format::header::*;
use dagon_format::statelog::{kind, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::TRIAL_ATTEMPTS;
use dagon_port::{Flash, FlashError, FlashRegion};
use ed25519_compact::{KeyPair, Seed};
use std::cell::RefCell;
use std::rc::Rc;

/// RFC 8032 section 7.1 TEST 1 key (public test data).
pub const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

/// The F401's flash with a power cut: operations from number `cut` on do
/// nothing and fail (the one at `cut` lands half when `half`).
#[derive(Clone)]
pub struct Cut {
    pub mem: Rc<RefCell<Vec<u8>>>,
    pub ops: Rc<RefCell<usize>>,
    pub cut: Option<(usize, bool)>,
}

impl Cut {
    pub fn new() -> Self {
        Cut {
            mem: Rc::new(RefCell::new(vec![0xFF; MIN_FLASH_SIZE])),
            ops: Rc::default(),
            cut: None,
        }
    }
    /// A flash of its own holding what this one holds now, powered.
    pub fn copy(&self) -> Self {
        Cut {
            mem: Rc::new(RefCell::new(self.mem.borrow().clone())),
            ops: Rc::default(),
            cut: None,
        }
    }
    pub fn powered(&self) -> bool {
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

pub fn signed(kind_: u8, slot: u8, base: u32, minor: u8, len: usize) -> Vec<u8> {
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

pub fn head(img: &[u8], slot: u8) -> SlotRef {
    let raw: [u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
    SlotRef {
        slot,
        version: ImageHeader::parse(&raw).unwrap().version,
        digest16: header_digest16(&raw),
    }
}

/// Core A and app A installed, the log provisioned for both, as the
/// first install writes them.
pub fn installed() -> Cut {
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
pub fn installed_full() -> Cut {
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
