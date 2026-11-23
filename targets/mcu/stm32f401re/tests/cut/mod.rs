//! The F401's flash with a power cut at any flash operation, the signed
//! images and first install the power-cut tests start from, and the
//! inventory of what each cut flash operation was ([`Site`]), which the
//! power-cut tests check they cut all of and report ([`Coverage`]).
#![allow(dead_code)]

use dagon_core::log::{trial_room, Log, SlotRef};
use dagon_format::header::*;
use dagon_format::statelog::{kind, target, FaceHeader};
use dagon_format::statelog::{Record, FACE_HEADER_SIZE, RECORD_SIZE};
use dagon_format::{key_id, sha256, Version};
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::TRIAL_ATTEMPTS;
use dagon_port::{Flash, FlashError, FlashRegion};
use ed25519_compact::{KeyPair, Seed};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
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
    /// Every erase and write asked for, in order, the cut ones too.
    pub trace: Rc<RefCell<Vec<FlashOp>>>,
}

impl Cut {
    pub fn new() -> Self {
        Cut {
            mem: Rc::new(RefCell::new(vec![0xFF; MIN_FLASH_SIZE])),
            ops: Rc::default(),
            cut: None,
            trace: Rc::default(),
        }
    }
    /// A flash of its own holding what this one holds now, powered.
    pub fn copy(&self) -> Self {
        Cut {
            mem: Rc::new(RefCell::new(self.mem.borrow().clone())),
            ops: Rc::default(),
            cut: None,
            trace: Rc::default(),
        }
    }
    /// What flash operation number `n` was, when one was asked for.
    pub fn site(&self, n: usize) -> Option<Site> {
        sites(&self.trace.borrow()).get(n).copied()
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
        self.trace.borrow_mut().push(FlashOp::Erase { addr });
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
        self.trace.borrow_mut().push(FlashOp::Write {
            addr,
            len: data.len(),
            head: data[..data.len().min(RECORD_SIZE)].to_vec(),
        });
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
    let app = signed(image_kind::APP, 0, SLOTS.slots[0].header, 0, 2000);
    f.write(CORE_SLOTS[0].0, &core).unwrap();
    f.write(SLOTS.slots[0].header, &app).unwrap();
    f.write(LOG.faces[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let mut log = Log::load(&mut f, &LOG).unwrap();
    for k in [kind::PROVISION, kind::CONFIRMED] {
        log.append_core(&mut f, k, head(&core, 0), 0).unwrap();
        log.append(&mut f, k, head(&app, 0), 0).unwrap();
    }
    *f.ops.borrow_mut() = 0;
    f.trace.borrow_mut().clear();
    f
}

/// [`installed`] with the shared log's face so full (APP records) that
/// the store's reservation compacts it during the update: erase of the
/// other face, both targets' records carried, the face header last.
pub fn installed_full() -> Cut {
    let mut f = installed();
    let app = signed(image_kind::APP, 0, SLOTS.slots[0].header, 0, 2000);
    let mut log = Log::load(&mut f, &LOG).unwrap();
    while log.room() >= trial_room(TRIAL_ATTEMPTS) {
        log.append(&mut f, kind::CONFIRMED, head(&app, 0), 0)
            .unwrap();
    }
    *f.ops.borrow_mut() = 0;
    f.trace.borrow_mut().clear();
    f
}

/// One erase or write as the code under test asked for it (a write's
/// first record's worth of bytes kept).
#[derive(Clone, Debug)]
pub enum FlashOp {
    Erase {
        addr: u32,
    },
    Write {
        addr: u32,
        len: usize,
        head: Vec<u8>,
    },
}

/// The part of an image slot a flash operation goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    Erase,
    Body,
    Header,
}

/// What a flash operation was, by where it went and what it wrote: the
/// inventory the power-cut tests must cut all of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Site {
    /// An image slot of `t` (`target::APP`, `target::BOOTCORE`).
    Slot { t: u8, slot: u8, part: Part },
    /// A state log record of target `t`; `carried` when a compaction
    /// copies it into the new face.
    Record { t: u8, kind: u8, carried: bool },
    /// A compaction's erase of the face it copies into.
    FaceErase,
    /// A compaction's last step: the new face's header.
    FaceHeader,
    /// Anywhere else: no power-cut test expects one.
    Outside { addr: u32 },
}

fn slot_of(addr: u32) -> Option<(u8, u8, u32)> {
    for (i, p) in SLOTS.slots.iter().enumerate() {
        if (p.header..p.body + p.room).contains(&addr) {
            return Some((target::APP, i as u8, p.header));
        }
    }
    for (i, &(base, size)) in CORE_SLOTS.iter().enumerate() {
        if (base..base + size).contains(&addr) {
            return Some((target::BOOTCORE, i as u8, base));
        }
    }
    None
}

fn in_log(addr: u32) -> Option<u32> {
    LOG.faces
        .iter()
        .copied()
        .find(|&f| (f..f + LOG.face_len).contains(&addr))
}

/// The site of each operation of `trace`. A record is carried when it
/// follows a face erase whose face header has not been written yet.
pub fn sites(trace: &[FlashOp]) -> Vec<Site> {
    let mut compacting = false;
    trace
        .iter()
        .map(|op| match *op {
            FlashOp::Erase { addr } => match (slot_of(addr), in_log(addr)) {
                (Some((t, slot, _)), _) => Site::Slot {
                    t,
                    slot,
                    part: Part::Erase,
                },
                (_, Some(_)) => {
                    compacting = true;
                    Site::FaceErase
                }
                _ => Site::Outside { addr },
            },
            FlashOp::Write {
                addr,
                len,
                ref head,
            } => match (slot_of(addr), in_log(addr)) {
                (Some((t, slot, base)), _) => Site::Slot {
                    t,
                    slot,
                    part: if addr == base && len == HEADER_SIZE {
                        Part::Header
                    } else {
                        Part::Body
                    },
                },
                (_, Some(face)) if addr == face && len == FACE_HEADER_SIZE => {
                    compacting = false;
                    Site::FaceHeader
                }
                (_, Some(_)) => match head.as_slice().try_into().map(Record::parse) {
                    Ok(Ok(r)) if len == RECORD_SIZE => Site::Record {
                        t: r.target,
                        kind: r.kind,
                        carried: compacting,
                    },
                    _ => Site::Outside { addr },
                },
                _ => Site::Outside { addr },
            },
        })
        .collect()
}

fn slot_name(slot: u8) -> char {
    (b'A' + slot) as char
}

fn kind_name(k: u8) -> String {
    match k {
        kind::SLOT_PENDING => "SLOT_PENDING".into(),
        kind::BOOT_ATTEMPT => "BOOT_ATTEMPT".into(),
        kind::CONFIRMED => "CONFIRMED".into(),
        kind::ROLLBACK => "ROLLBACK".into(),
        kind::UPDATE_STAY => "UPDATE_STAY".into(),
        kind::PROVISION => "PROVISION".into(),
        kind::UPDATE_DONE => "UPDATE_DONE".into(),
        kind::SNAPSHOT => "SNAPSHOT".into(),
        k => format!("0x{k:02X}"),
    }
}

fn target_name(t: u8) -> &'static str {
    if t == target::BOOTCORE {
        "ブートローダ"
    } else {
        "APP"
    }
}

impl Site {
    /// The report's name for the site.
    pub fn label(self) -> String {
        match self {
            Site::Slot { t, slot, part } => {
                let what = match part {
                    Part::Erase => "消去",
                    Part::Body => "本体の書込",
                    Part::Header => "ヘッダの書込（最終操作）",
                };
                format!("{}スロット{}: {what}", target_name(t), slot_name(slot))
            }
            Site::Record { t, kind, carried } => format!(
                "{}{}記録 {}",
                if carried {
                    "ログ圧縮: 継承する"
                } else {
                    "ログ: "
                },
                target_name(t),
                kind_name(kind)
            ),
            Site::FaceErase => "ログ圧縮: 移行先の面の消去".into(),
            Site::FaceHeader => "ログ圧縮: 面ヘッダの書込（最終操作）".into(),
            Site::Outside { addr } => format!("範囲外 0x{addr:08X}"),
        }
    }
}

/// How a power cut met a flash operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Way {
    /// Half of it landed (else none of it).
    pub half: bool,
    /// A reset that kept retained memory (else a power loss).
    pub keep: bool,
}

/// The cuts a test made, by site and way; every one of them recovered
/// (a test fails at the first that does not).
#[derive(Clone, Debug, Default)]
pub struct Coverage(pub BTreeMap<Site, BTreeMap<Way, usize>>);

impl Coverage {
    pub fn add(&mut self, site: Site, way: Way) {
        *self.0.entry(site).or_default().entry(way).or_default() += 1;
    }

    pub fn merge(&mut self, other: &Coverage) {
        for (s, ways) in &other.0 {
            for (w, n) in ways {
                *self.0.entry(*s).or_default().entry(*w).or_default() += n;
            }
        }
    }

    pub fn cuts(&self) -> usize {
        self.0.values().flat_map(|w| w.values()).sum()
    }

    /// Fails naming each site of `required` not cut in every one of
    /// `ways`, and each site cut that is not in `inventory`.
    pub fn check(&self, required: &[Site], inventory: &[Site], ways: &[Way]) {
        let mut wrong = Vec::new();
        for s in required {
            for w in ways {
                if self.0.get(s).and_then(|m| m.get(w)).is_none() {
                    wrong.push(format!("not cut ({w:?}): {}", s.label()));
                }
            }
        }
        for s in self.0.keys() {
            if !inventory.contains(s) {
                wrong.push(format!("cut but not in the inventory: {}", s.label()));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// The report's table: one row per site, the cuts of each way.
    pub fn table(&self, ways: &[Way]) -> String {
        let name = |w: &Way| {
            format!(
                "{}・{}",
                if w.half {
                    "半分が反映"
                } else {
                    "未反映"
                },
                if w.keep { "リセット" } else { "電源断" }
            )
        };
        let mut t = String::from("| フラッシュ操作 |");
        for w in ways {
            let _ = write!(t, " {} |", name(w));
        }
        t.push_str(" 復旧失敗 |\n|---|");
        for _ in ways {
            t.push_str("--:|");
        }
        t.push_str("--:|\n");
        for (s, m) in &self.0 {
            let _ = write!(t, "| {} |", s.label());
            for w in ways {
                let _ = write!(t, " {} |", m.get(w).copied().unwrap_or(0));
            }
            t.push_str(" 0 |\n");
        }
        t
    }

    /// Writes `body` as `<name>.md` into the folder `DAGON_POWERCUT_REPORT`
    /// names, when it names one.
    pub fn report(name: &str, body: &str) {
        if let Ok(dir) = std::env::var("DAGON_POWERCUT_REPORT") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{name}.md"), body).unwrap();
        }
    }
}

/// Both ways a cut meets an operation with retained memory lost.
pub const POWER_LOSS: [Way; 2] = [
    Way {
        half: false,
        keep: false,
    },
    Way {
        half: true,
        keep: false,
    },
];

/// All four ways.
pub const ALL_WAYS: [Way; 4] = [
    Way {
        half: false,
        keep: false,
    },
    Way {
        half: true,
        keep: false,
    },
    Way {
        half: false,
        keep: true,
    },
    Way {
        half: true,
        keep: true,
    },
];

/// The sites a compaction makes: the erase, the records it carries for
/// both targets, the face header.
pub fn compaction_sites(carried: &[(u8, u8)]) -> Vec<Site> {
    let mut v = vec![Site::FaceErase, Site::FaceHeader];
    v.extend(carried.iter().map(|&(t, kind)| Site::Record {
        t,
        kind,
        carried: true,
    }));
    v
}

/// The three sites of writing slot `slot` of `t`.
pub fn slot_sites(t: u8, slot: u8) -> [Site; 3] {
    [Part::Erase, Part::Body, Part::Header].map(|part| Site::Slot { t, slot, part })
}

pub fn record(t: u8, kind: u8) -> Site {
    Site::Record {
        t,
        kind,
        carried: false,
    }
}
