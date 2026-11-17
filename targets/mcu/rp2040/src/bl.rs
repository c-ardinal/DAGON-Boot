//! Boot core A/B on the hub (`dagon_core::bootcore` over the hub layout):
//! stage0's choice of the boot core slot (or, with no core to start, of
//! the confirmed app), and what the started boot core records about the
//! trial before it starts the app. Only the boot core writes the boot core
//! log (stage0 keeps the trial count in the trial cell, never in flash).

use dagon_core::bootcore::{self, Apps, Boot, CoreFail, Diag, NoCore, Stage0, Start};
pub use dagon_core::bootcore::{MustReset, Settled};
use dagon_core::log::{trial_room, Log, LogError};
use dagon_format::SigVerifier;
use dagon_port::Flash;

use crate::layout::{APP_LOG_LAYOUT, BL_LOG_LAYOUT};
use crate::TRIAL_ATTEMPTS;

/// What stage0 knows of this MCU (`dagon_core::bootcore::decide`): the boot
/// core slots `(base, size)`, slot A first, from the generated definition.
pub const STAGE0: Stage0<'static> = Stage0 {
    log: &BL_LOG_LAYOUT,
    slots: &crate::slots::BOOTCORE,
    max: TRIAL_ATTEMPTS,
    mcu_id: crate::MCU_WIRE_ID,
};

/// The apps stage0 starts the confirmed one of when no boot core may
/// (`dagon_core::bootcore::start`): the app log, the APP slots, this
/// node's tag (`DAGON_TARGET_TAG` at build time) and the vector table's
/// entry point.
pub const APPS: Apps<'static> = Apps {
    log: &APP_LOG_LAYOUT,
    slots: &crate::slots::APP,
    target_tag: crate::tag::stage0_tag(crate::TARGET_TAG),
    entry: dagon_mcu_common::vector::entry,
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

/// What stage0 starts (`dagon_core::bootcore::start`): the core
/// [`choose`] gives; with none, the confirmed app of [`APPS`] that passes
/// the same check and more; with neither, the reason goes to `diag`.
/// Reads flash only.
pub fn start<F: Flash, V: SigVerifier + ?Sized, D: Diag>(
    flash: &mut F,
    cell: [u32; 2],
    verifier: &V,
    diag: &mut D,
) -> Result<Boot, NoCore<CoreFail>> {
    bootcore::start(flash, &STAGE0, &APPS, cell, verifier, diag)
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

/// Why the app does not hand over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandOverError {
    /// The boot core receives on no link: nothing would serve update mode.
    Unsupported,
    /// The state log could not take the record.
    Log(LogError),
}

/// The app's hand-over to the boot core's update mode (REBOOT mode 1):
/// UPDATE_STAY in the app log, after room for the boot core's UPDATE_DONE
/// and the trials (`dagon_core::log::Log::request_stay`). Refused, writing
/// nothing, when the boot core receives on no link (`boot_core_receives`).
pub fn request_update_mode<F: Flash>(
    flash: &mut F,
    boot_core_receives: bool,
) -> Result<(), HandOverError> {
    if !boot_core_receives {
        return Err(HandOverError::Unsupported);
    }
    let mut log = Log::load(flash, &APP_LOG_LAYOUT).map_err(HandOverError::Log)?;
    log.request_stay(flash, trial_room(TRIAL_ATTEMPTS))
        .map_err(HandOverError::Log)
}

/// The boot core's start: whether the app handed over, consuming its
/// UPDATE_STAY with UPDATE_DONE in the room the app made (no erase). An
/// unreadable log is no hand-over.
pub fn handed_over<F: Flash>(flash: &mut F) -> bool {
    Log::load(flash, &APP_LOG_LAYOUT).is_ok_and(|mut l| l.take_stay(flash) == Ok(true))
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

    /// An empty boot core log names nothing: stage0 starts no core; APP
    /// records in the boot core log do not count.
    #[test]
    fn an_empty_or_app_only_log_names_no_core() {
        let mut m = Mem(vec![0xFF; 0x4_8000]);
        let v = Ed25519Small::new([0; 32]);
        let e = choose(&mut m, CLEARED, &v, &mut NoDiag).unwrap_err();
        assert_eq!((e.named, e.why), (false, CoreFail::Unnamed));
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
