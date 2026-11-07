//! Boot core A/B on the hub (`dagon_core::bootcore` over the hub layout):
//! stage0's choice of the boot core slot, and what the started boot core
//! records about the trial before it starts the app. Only the boot core
//! writes the boot core log (stage0 keeps the trial count in the trial
//! cell, never in flash).

use dagon_core::bootcore::{self, NoCore, Start};
pub use dagon_core::bootcore::{MustReset, Settled};
use dagon_core::log::SlotRef;
use dagon_format::header::image_kind;
use dagon_port::Flash;

use crate::image::{self, Fail};
use crate::layout::{BL_LOG_LAYOUT, BL_SLOT, BL_SLOT_SIZE};
use crate::TRIAL_ATTEMPTS;

/// Stage0's choice from the boot core log and the trial cell `cell` read
/// at reset (`dagon_core::bootcore::choose`). Reads flash only.
pub fn choose<F: Flash>(flash: &mut F, cell: [u32; 2]) -> Result<Start<u32>, NoCore<Fail>> {
    let st = bootcore::read_state(flash, &BL_LOG_LAYOUT).unwrap_or_default();
    bootcore::choose(&st, cell, TRIAL_ATTEMPTS, |s, r| check(flash, s, r))
}

/// The core image in slot `s`, against the record `r` when one names it.
fn check<F: Flash>(flash: &mut F, s: u8, r: Option<&SlotRef>) -> Result<u32, Fail> {
    let base = *BL_SLOT.get(usize::from(s)).ok_or(Fail::Kind)?;
    image::check(flash, base, BL_SLOT_SIZE, s, r, image_kind::BOOTCORE)
}

/// The boot core running from slot `running` settles the boot core trial
/// before it starts the app (`dagon_core::bootcore::settle_log`); `cell`
/// the trial cell's value and whether it holds.
pub fn settle_core<F: Flash>(
    flash: &mut F,
    running: u8,
    (cell, holds): ([u32; 2], bool),
    healthy: bool,
) -> Result<Settled, MustReset> {
    let cell = bootcore::Cell {
        value: cell,
        max: TRIAL_ATTEMPTS,
        holds,
    };
    bootcore::settle_log(flash, &BL_LOG_LAYOUT, running, cell, healthy)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::layout::BL_LOG;
    use dagon_core::bootcore::CLEARED;
    use dagon_format::statelog::{kind, target, FaceHeader, Record};
    use dagon_format::Version;
    use dagon_port::{FlashError, FlashRegion};
    use std::vec;
    use std::vec::Vec;

    struct Mem(Vec<u8>);

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

    /// An empty boot core log names nothing: stage0 checks slot A on its
    /// own; APP records in the boot core log do not count.
    #[test]
    fn an_empty_or_app_only_log_names_no_core() {
        let mut m = Mem(vec![0xFF; 0x4_8000]);
        let e = choose(&mut m, CLEARED).unwrap_err();
        assert_eq!((e.named, e.why), (false, Fail::Header));
        let a = (BL_LOG[0] - crate::layout::XIP_BASE) as usize;
        m.0[a..a + 32].copy_from_slice(&FaceHeader { generation: 1 }.to_bytes());
        let r = Record {
            kind: kind::CONFIRMED,
            target: target::APP,
            slot: 1,
            reason: 0,
            seq: 1,
            version: Version::new(1, 0, 0),
            digest16: [7; 16],
        };
        m.0[a + 32..a + 64].copy_from_slice(&r.to_bytes());
        assert!(!choose(&mut m, CLEARED).unwrap_err().named);
    }
}
