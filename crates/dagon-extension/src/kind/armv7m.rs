//! `armv7m-mpu`: the ARMv7-M MPU (PMSAv7). Faults are MemManage faults with
//! `CFSR`, and the data address in `MMFAR` when `MMARVALID` is set;
//! stacking errors are flagged and unwound.

use dagon_topology::defs::isolation::{Kind, Level, Unwind};
use dagon_topology::defs::McuDef;

use super::{
    pmsa, ExtLayout, Guarantees, Map, ProtectionKind, ReadBack, Refusal, Space, Violation,
};

/// The `armv7m-mpu` generator and checker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Armv7mMpu;

impl ProtectionKind for Armv7mMpu {
    fn kind(&self) -> Kind {
        Kind::Armv7mMpu
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
                "windows are power-of-two blocks",
                "a fault's data address when MMARVALID is clear",
            ],
        }
    }

    fn read_back(&self, def: &McuDef) -> Vec<ReadBack> {
        pmsa::read_back(def)
    }
}
