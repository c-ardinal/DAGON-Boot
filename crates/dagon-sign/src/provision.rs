//! First-install flash contents: the F401 ST-LINK binary and the hub
//! rescue UF2, each with a state log that starts with the slot A image
//! provisioned and confirmed.

use std::collections::{BTreeMap, BTreeSet};

use dagon_format::header::{self, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{self, kind, reason, slot, FaceHeader, Record};

use dagon_topology::defs::{requirements, McuDef, Writer};

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

/// The page the definitions are checked against (`requirements::PAGE`).
const PAGE: usize = requirements::PAGE as usize;

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
    /// UF2 file covering every touched 4 KiB sector whole: its 16 pages in
    /// ascending order, unwritten bytes 0xFF. The RP2040 boot ROM erases a
    /// sector per 4 KiB write it receives; a sector split across two writes
    /// loses the part written first, so sectors must never straddle them.
    pub fn to_uf2(&self, family_id: u32) -> Vec<u8> {
        self.to_uf2_with(family_id, DEFAULT_UF2_SECTOR)
    }

    /// [`to_uf2`](Self::to_uf2) with the writer's erase `sector`, which
    /// must meet [`requirements::uf2_sector_ok`] and end below 2^32 (as
    /// definitions are checked to on load).
    pub fn to_uf2_with(&self, family_id: u32, sector: u32) -> Vec<u8> {
        assert!(
            requirements::uf2_sector_ok(sector),
            "UF2 sector {sector:#x} breaks the UF2 builder's requirement"
        );
        let sectors: BTreeSet<u32> = self.pages.keys().map(|a| a & !(sector - 1)).collect();
        assert!(
            sectors.iter().all(|s| s.checked_add(sector).is_some()),
            "a UF2 sector ends at 2^32"
        );
        let pages: Vec<(u32, [u8; PAGE])> = sectors
            .iter()
            .flat_map(|&s| (s..s + sector).step_by(PAGE))
            .map(|a| (a, self.pages.get(&a).copied().unwrap_or([0xFF; PAGE])))
            .collect();
        let n = pages.len() as u32;
        let mut out = Vec::with_capacity(pages.len() * 512);
        for (i, (addr, page)) in pages.iter().enumerate() {
            let addr = *addr;
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

/// Erase sector [`FlashImage::to_uf2`] rounds to; a UF2 writer's
/// definition (`sector`) gives its own to `to_uf2_with`.
const DEFAULT_UF2_SECTOR: u32 = 0x1000;
// The UF2 format's own words (not an MCU's).
const UF2_MAGIC0: u32 = 0x0A32_4655;
const UF2_MAGIC1: u32 = 0x9E5D_5157;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;
const UF2_FLAG_FAMILY: u32 = 0x0000_2000;

/// First-install contents of `mcu` from its definition: stage0 at its
/// region (exactly when the layout has one), each state log with face A
/// provisioned (PROVISION + CONFIRMED for slot A) and face B erased, and
/// the boot core and app images in their slots. Callers check sizes.
pub fn first_install(
    mcu: &McuDef,
    stage0: Option<&[u8]>,
    boot: (&Slot, &[u8]),
    app: (&Slot, &[u8]),
) -> Result<FlashImage, String> {
    let mut f = FlashImage::default();
    match (mcu.layout.stage0, stage0) {
        (Some(r), Some(bytes)) => f.put(r.at, bytes),
        (None, None) => {}
        (Some(_), None) => return Err(format!("{} needs stage0", mcu.id)),
        (None, Some(_)) => return Err(format!("{} has no stage0 region", mcu.id)),
    }
    let logs = [
        (mcu.layout.bootcore_log, statelog::target::BOOTCORE, boot.1),
        (mcu.layout.app_log, statelog::target::APP, app.1),
    ];
    for (log, target, image) in logs {
        let Some(log) = log else { continue };
        for face in log.faces {
            f.blank(face, log.face_size);
        }
        let face = initial_log(target, image);
        assert!(
            face.len() as u32 <= log.face_size,
            "{}: a provisioned face needs {} bytes (requirements::LOG_FACE_MIN)",
            mcu.id,
            face.len()
        );
        f.put(log.faces[0], &face);
    }
    f.put_image(boot.0, boot.1);
    f.put_image(app.0, app.1);
    Ok(f)
}

/// The file `mcu`'s writer takes for a first install: a UF2 (`uf2-drive`)
/// or a binary from the flash base (`probe-rs`, after a chip erase). An
/// `espflash` writer takes partitions one by one (dagon-cli).
pub fn first_install_file(
    mcu: &McuDef,
    stage0: Option<&[u8]>,
    boot: (&Slot, &[u8]),
    app: (&Slot, &[u8]),
) -> Result<Vec<u8>, String> {
    let f = first_install(mcu, stage0, boot, app)?;
    match &mcu.writer {
        Writer::Uf2Drive { family, sector, .. } => Ok(f.to_uf2_with(*family, *sector)),
        Writer::ProbeRs { .. } => Ok(f.to_bin(mcu.default_flash_map().base)),
        Writer::Espflash { .. } => Err(format!(
            "{} is written partition by partition, not as one file",
            mcu.id
        )),
    }
}
