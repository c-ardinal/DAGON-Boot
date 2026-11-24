//! The reset cause the RP2040 logs (datasheet section 2.10.2, chip-level
//! reset, and 4.7, watchdog): WATCHDOG REASON names a watchdog reset (its
//! timer, or a forced one: DAGON resets through the watchdog), and
//! VREG_AND_CHIP_RESET CHIP_RESET the last chip-level reset (power-on or
//! brown-out, the RUN pin, the debug port). The hardware writes both at
//! each reset, so the app reads them at its start and nothing needs
//! clearing: a chip-level reset leaves REASON zero.

use dagon_port::ResetCause;

/// WATCHDOG REASON bits.
pub mod reason {
    pub const TIMER: u32 = 1 << 0;
    pub const FORCE: u32 = 1 << 1;
}

/// VREG_AND_CHIP_RESET CHIP_RESET bits.
pub mod chip {
    pub const HAD_POR: u32 = 1 << 8;
    pub const HAD_RUN: u32 = 1 << 16;
    pub const HAD_PSM_RESTART: u32 = 1 << 20;
}

/// The cause the two registers give: a watchdog reset first (REASON is
/// zero after any other), then the chip-level reset.
pub const fn cause(reason: u32, chip_reset: u32) -> ResetCause {
    if reason & reason::TIMER != 0 {
        ResetCause::Watchdog
    } else if reason & reason::FORCE != 0 {
        ResetCause::Software
    } else if chip_reset & chip::HAD_POR != 0 {
        ResetCause::PowerOn
    } else if chip_reset & chip::HAD_RUN != 0 {
        ResetCause::Pin
    } else {
        ResetCause::Unknown
    }
}

/// The cause of the last reset. Call at the app's start.
#[cfg(all(target_os = "none", feature = "hal"))]
pub fn read() -> ResetCause {
    use embassy_rp::pac;
    let reason = pac::WATCHDOG.reason().read().0;
    let chip_reset = pac::VREG_AND_CHIP_RESET.chip_reset().read().0;
    cause(reason, chip_reset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watchdog_reset_comes_before_the_chip_level_one() {
        for (r, c, want) in [
            (0, chip::HAD_POR, ResetCause::PowerOn),
            (0, chip::HAD_RUN, ResetCause::Pin),
            (0, chip::HAD_PSM_RESTART, ResetCause::Unknown),
            // A watchdog reset leaves the chip-level bits of the reset
            // before it.
            (reason::TIMER, chip::HAD_POR, ResetCause::Watchdog),
            (reason::FORCE, chip::HAD_RUN, ResetCause::Software),
            (0, 0, ResetCause::Unknown),
        ] {
            assert_eq!(cause(r, c), want, "{r:#x} {c:#x}");
        }
    }
}
