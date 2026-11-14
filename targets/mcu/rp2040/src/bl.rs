//! Boot core A/B on the hub (`dagon_core::bootcore` over the hub layout):
//! stage0's choice of the boot core slot, and what the started boot core
//! records about the trial before it starts the app. Only the boot core
//! writes the boot core log (stage0 keeps the trial count in the trial
//! cell, never in flash).

use dagon_core::bootcore::{self, CoreFail, Diag, NoCore, Stage0, Start};
pub use dagon_core::bootcore::{MustReset, Settled};
use dagon_format::SigVerifier;
use dagon_port::Flash;

use crate::layout::BL_LOG_LAYOUT;
use crate::TRIAL_ATTEMPTS;

/// What stage0 knows of this MCU (`dagon_core::bootcore::decide`): the boot
/// core slots `(base, size)`, slot A first, from the generated definition.
pub const STAGE0: Stage0<'static> = Stage0 {
    log: &BL_LOG_LAYOUT,
    slots: &crate::slots::BOOTCORE,
    max: TRIAL_ATTEMPTS,
    mcu_id: crate::MCU_WIRE_ID,
};

/// Stage0's choice from the boot core log and the trial cell `cell` read
/// at reset (`dagon_core::bootcore::decide`): every core it starts passes
/// the shared check under `verifier`; with none, the reason goes to
/// `diag`. Reads flash only.
pub fn choose<F: Flash, V: SigVerifier + ?Sized, D: Diag>(
    flash: &mut F,
    cell: [u32; 2],
    verifier: &V,
    diag: &mut D,
) -> Result<Start<u32>, NoCore<CoreFail>> {
    bootcore::decide(flash, &STAGE0, cell, verifier, diag)
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
    use dagon_core::bootcore::{NoDiag, CLEARED};
    use dagon_ed25519::Ed25519Small;
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
        let v = Ed25519Small::new([0; 32]);
        let e = choose(&mut m, CLEARED, &v, &mut NoDiag).unwrap_err();
        assert_eq!((e.named, e.why), (false, CoreFail::Header));
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
        assert!(!choose(&mut m, CLEARED, &v, &mut NoDiag).unwrap_err().named);
    }
}
