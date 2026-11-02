//! First-install flash contents: the F401 ST-LINK binary and the hub
//! rescue UF2, each with a state log that starts with the slot A image
//! provisioned and confirmed.

use std::collections::BTreeMap;

use dagon_format::header::{self, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{self, kind, reason, slot, FaceHeader, Record};

use crate::image::Slot;

/// State log face: header (generation 1), PROVISION and CONFIRMED for
/// slot A of `target`.
pub fn initial_log(target: u8, image: &[u8]) -> Vec<u8> {
    let raw: [u8; HEADER_SIZE] = image[..HEADER_SIZE].try_into().expect("signed image");
    let h = ImageHeader::parse(&raw).expect("signed image");
    let rec = |k, seq| Record {
        kind: k,
        target,
        slot: slot::A,
        reason: reason::NONE,
        seq,
        version: h.version,
        digest16: header::header_digest16(&raw),
    };
    let mut out = FaceHeader { generation: 1 }.to_bytes().to_vec();
    out.extend_from_slice(&rec(kind::PROVISION, 1).to_bytes());
    out.extend_from_slice(&rec(kind::CONFIRMED, 2).to_bytes());
    out
}

const PAGE: usize = 256;

/// Sparse flash contents in 256-byte pages; unwritten bytes read 0xFF.
#[derive(Default)]
pub struct FlashImage {
    pages: BTreeMap<u32, [u8; PAGE]>,
}

impl FlashImage {
    pub fn put(&mut self, addr: u32, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() {
            let a = addr + i as u32;
            let page = self
                .pages
                .entry(a & !(PAGE as u32 - 1))
                .or_insert([0xFF; PAGE]);
            page[a as usize % PAGE] = b;
        }
    }

    /// Covers `len` bytes from `addr` with 0xFF so the region is rewritten.
    pub fn blank(&mut self, addr: u32, len: u32) {
        self.put(addr, &vec![0xFF; len as usize]);
    }

    /// Header and payload of a signed image at their slot addresses.
    pub fn put_image(&mut self, slot: &Slot, image: &[u8]) {
        self.put(slot.header_addr, &image[..HEADER_SIZE]);
        self.put(slot.load_addr, &image[HEADER_SIZE..]);
    }

    /// Contiguous bytes from `base` to the last written page.
    pub fn to_bin(&self, base: u32) -> Vec<u8> {
        let end = self
            .pages
            .keys()
            .next_back()
            .map_or(base, |&a| a + PAGE as u32);
        let mut out = vec![0xFF; (end - base) as usize];
        for (&a, page) in &self.pages {
            let off = (a - base) as usize;
            out[off..off + PAGE].copy_from_slice(page);
        }
        out
    }

    /// UF2 file, one 256-byte block per written page, ascending.
    pub fn to_uf2(&self, family_id: u32) -> Vec<u8> {
        let n = self.pages.len() as u32;
        let mut out = Vec::with_capacity(self.pages.len() * 512);
        for (i, (&addr, page)) in self.pages.iter().enumerate() {
            let mut block = [0u8; 512];
            let words = [
                UF2_MAGIC0,
                UF2_MAGIC1,
                UF2_FLAG_FAMILY,
                addr,
                PAGE as u32,
                i as u32,
                n,
                family_id,
            ];
            for (j, w) in words.iter().enumerate() {
                block[j * 4..j * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            block[32..32 + PAGE].copy_from_slice(page);
            block[508..].copy_from_slice(&UF2_MAGIC_END.to_le_bytes());
            out.extend_from_slice(&block);
        }
        out
    }
}

const UF2_MAGIC0: u32 = 0x0A32_4655;
const UF2_MAGIC1: u32 = 0x9E5D_5157;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;
const UF2_FLAG_FAMILY: u32 = 0x0000_2000;
pub const UF2_FAMILY_RP2040: u32 = 0xE48B_FF56;

pub const F401_FLASH_BASE: u32 = 0x0800_0000;
const F401_LOG_A: u32 = 0x0800_8000;

/// F401 first install, written at [`F401_FLASH_BASE`] after a full chip
/// erase: boot core (payload at 0x08000000, header in the trailer), APP state
/// log A, APP-A. Log B and the reserved sector stay erased.
pub fn f401_flash(boot: (&Slot, &[u8]), app: (&Slot, &[u8])) -> Vec<u8> {
    let mut f = FlashImage::default();
    f.put_image(boot.0, boot.1);
    f.put(F401_LOG_A, &initial_log(statelog::target::APP, app.1));
    f.put_image(app.0, app.1);
    f.to_bin(F401_FLASH_BASE)
}

const RP2040_XIP: u32 = 0x1000_0000;
pub const RP2040_STAGE0_MAX: u32 = 0x4000;
const RP2040_BL_LOG_A: u32 = RP2040_XIP + 0x4000;
const RP2040_BL_LOG_B: u32 = RP2040_XIP + 0x5000;
const RP2040_APP_LOG_A: u32 = RP2040_XIP + 0x6000;
const RP2040_APP_LOG_B: u32 = RP2040_XIP + 0x7000;
const RP2040_LOG_FACE: u32 = 0x1000;

/// Hub rescue UF2: boot2 + stage0 (raw, from the start of flash), boot core
/// log and app log (face A provisioned, face B erased), BL-A and APP-A.
pub fn hub_rescue_uf2(stage0: &[u8], boot: (&Slot, &[u8]), app: (&Slot, &[u8])) -> Vec<u8> {
    let mut f = FlashImage::default();
    f.put(RP2040_XIP, stage0);
    f.blank(RP2040_BL_LOG_A, RP2040_LOG_FACE);
    f.put(
        RP2040_BL_LOG_A,
        &initial_log(statelog::target::BOOTCORE, boot.1),
    );
    f.blank(RP2040_BL_LOG_B, RP2040_LOG_FACE);
    f.blank(RP2040_APP_LOG_A, RP2040_LOG_FACE);
    f.put(RP2040_APP_LOG_A, &initial_log(statelog::target::APP, app.1));
    f.blank(RP2040_APP_LOG_B, RP2040_LOG_FACE);
    f.put_image(boot.0, boot.1);
    f.put_image(app.0, app.1);
    f.to_uf2(UF2_FAMILY_RP2040)
}
