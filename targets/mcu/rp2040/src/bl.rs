//! Read-only view of the boot core state log for stage0: which boot core
//! slot the log confirms. dagon-core's log only folds APP records, so this
//! adapter reads BOOTCORE records the same way (newest valid face, records up
//! to the first erased or corrupt one). Boot core updates are not supported,
//! so only PROVISION and CONFIRMED matter here.

use dagon_core::log::SlotRef;
use dagon_format::statelog::{kind, target, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE};
use dagon_port::Flash;

use crate::layout::LOG_FACE_SIZE;

pub fn confirmed<F: Flash>(flash: &mut F, faces: [u32; 2]) -> Option<SlotRef> {
    let mut best: Option<(u32, u32)> = None;
    for face in faces {
        let mut h = [0u8; FACE_HEADER_SIZE];
        flash.read(face, &mut h).ok()?;
        if let Ok(fh) = FaceHeader::parse(&h) {
            if best.is_none_or(|(_, g)| fh.generation > g) {
                best = Some((face, fh.generation));
            }
        }
    }
    let (face, _) = best?;
    let mut found = None;
    let mut at = face + FACE_HEADER_SIZE as u32;
    while at + RECORD_SIZE as u32 <= face + LOG_FACE_SIZE {
        let mut b = [0u8; RECORD_SIZE];
        flash.read(at, &mut b).ok()?;
        if Record::is_erased(&b) {
            break;
        }
        let Ok(r) = Record::parse(&b) else { break };
        if r.target == target::BOOTCORE && matches!(r.kind, kind::PROVISION | kind::CONFIRMED) {
            found = Some(SlotRef {
                slot: r.slot,
                version: r.version,
                digest16: r.digest16,
            });
        }
        at += RECORD_SIZE as u32;
    }
    found
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use super::*;
    use crate::layout::BL_LOG;
    use dagon_format::Version;
    use dagon_port::{FlashError, FlashRegion};
    use std::vec;
    use std::vec::Vec;

    pub struct Mem(pub Vec<u8>);

    impl Flash for Mem {
        fn geometry(&self) -> &[FlashRegion] {
            &[]
        }
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            let a = (addr - crate::layout::XIP_BASE) as usize;
            buf.copy_from_slice(self.0.get(a..a + buf.len()).ok_or(FlashError::Range)?);
            Ok(())
        }
        fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
            Err(FlashError::Hw)
        }
        fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
            Err(FlashError::Hw)
        }
    }

    fn rec(kind: u8, target: u8, slot: u8, seq: u32) -> Record {
        Record {
            kind,
            target,
            slot,
            reason: 0,
            seq,
            version: Version::new(1, 0, 0),
            digest16: [7; 16],
        }
    }

    fn face(m: &mut Mem, at: u32, generation: u32, recs: &[Record]) {
        let a = (at - crate::layout::XIP_BASE) as usize;
        m.0[a..a + 32].copy_from_slice(&FaceHeader { generation }.to_bytes());
        for (i, r) in recs.iter().enumerate() {
            m.0[a + 32 + i * 32..a + 64 + i * 32].copy_from_slice(&r.to_bytes());
        }
    }

    #[test]
    fn install_log_names_slot_a_and_empty_log_names_none() {
        let mut m = Mem(vec![0xFF; 0x8000]);
        assert_eq!(confirmed(&mut m, BL_LOG), None);
        // As written by the install UF2: PROVISION + CONFIRMED for slot A.
        let t = target::BOOTCORE;
        face(
            &mut m,
            BL_LOG[0],
            1,
            &[rec(kind::PROVISION, t, 0, 1), rec(kind::CONFIRMED, t, 0, 2)],
        );
        assert_eq!(confirmed(&mut m, BL_LOG).map(|r| r.slot), Some(0));
        // APP records and older faces do not count.
        face(
            &mut m,
            BL_LOG[1],
            2,
            &[rec(kind::CONFIRMED, target::APP, 1, 3)],
        );
        assert_eq!(confirmed(&mut m, BL_LOG), None);
    }
}
