//! `armv6m-mpu`: the ARMv6-M MPU (PMSAv6). Every fault is a HardFault with
//! no status or address register: a fault is attributed by the open call
//! and the stacked PC; a fault while stacking locks the core up, which the
//! lockup fuse covers.

use dagon_topology::defs::isolation::{Kind, Level, Unwind};
use dagon_topology::defs::McuDef;

use super::{
    pmsa, ExtLayout, Guarantees, Map, ProtectionKind, ReadBack, Refusal, Space, Violation,
};

/// The `armv6m-mpu` generator and checker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Armv6mMpu;

impl ProtectionKind for Armv6mMpu {
    fn kind(&self) -> Kind {
        Kind::Armv6mMpu
    }

    fn plan(&self, def: &McuDef, space: &Space, exts: &[ExtLayout]) -> Result<Map, Refusal> {
        pmsa::plan(self.kind(), def, space, exts)
    }

    fn check(&self, def: &McuDef, map: &Map) -> Vec<Violation> {
        pmsa::check(def, map)
    }

    fn unwind(&self) -> Unwind {
        Unwind::CortexMException
    }

    fn guarantees(&self) -> Guarantees {
        Guarantees {
            level: Level::Mpu,
            guaranteed: super::MPU_GUARANTEED,
            not_guaranteed: &[
                "physical effects of declared capabilities",
                "windows are aligned blocks of at least the unit's smallest region",
                "a fault during exception stacking locks the core up until the watchdog resets it",
            ],
        }
    }

    fn read_back(&self, def: &McuDef) -> Vec<ReadBack> {
        pmsa::read_back(def)
    }
}
