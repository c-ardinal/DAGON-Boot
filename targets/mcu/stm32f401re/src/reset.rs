//! The reset flags (RCC_CSR, RM0368 section 6.3.21): the app reads them
//! once at its start, then clears them (RMVF), so the next start reports
//! only the resets since this one. The boot core and stage0 run before
//! the app and leave the flags as they are.

use dagon_port::ResetCause;

/// RCC_CSR bits.
pub mod bit {
    pub const RMVF: u32 = 1 << 24;
    pub const BORRSTF: u32 = 1 << 25;
    pub const PINRSTF: u32 = 1 << 26;
    pub const PORRSTF: u32 = 1 << 27;
    pub const SFTRSTF: u32 = 1 << 28;
    pub const IWDGRSTF: u32 = 1 << 29;
    pub const WWDGRSTF: u32 = 1 << 30;
    pub const LPWRRSTF: u32 = 1 << 31;
}

/// The cause the flags of `csr` give. Every reset but a power-on one is
/// also seen on the reset pin (the MCU drives NRST low itself), and the
/// flags of every reset since the last clear stay set, so the most telling
/// one is named: a watchdog, then the software, then power-on or brown-out,
/// then the pin alone.
pub const fn cause(csr: u32) -> ResetCause {
    if csr & (bit::IWDGRSTF | bit::WWDGRSTF) != 0 {
        ResetCause::Watchdog
    } else if csr & bit::SFTRSTF != 0 {
        ResetCause::Software
    } else if csr & (bit::PORRSTF | bit::BORRSTF) != 0 {
        ResetCause::PowerOn
    } else if csr & bit::LPWRRSTF != 0 {
        ResetCause::Unknown
    } else if csr & bit::PINRSTF != 0 {
        ResetCause::Pin
    } else {
        ResetCause::Unknown
    }
}

/// RCC_CSR (RCC at 0x4002_3800, RM0368 section 2.3; CSR at offset 0x74).
#[cfg(target_os = "none")]
const CSR: *mut u32 = 0x4002_3874 as *mut u32;

/// The cause of the last reset, its flags then cleared. Call once, at the
/// app's start.
#[cfg(target_os = "none")]
pub fn take() -> ResetCause {
    // SAFETY: the fixed RCC_CSR register. Setting RMVF clears the reset
    // flags only; the LSI bits are written back as read.
    let csr = unsafe { CSR.read_volatile() };
    unsafe { CSR.write_volatile(csr | bit::RMVF) };
    cause(csr)
}

/// The cause the reset flags name now; the flags stay set.
#[cfg(target_os = "none")]
pub fn peek() -> ResetCause {
    // SAFETY: a read of the fixed RCC_CSR register.
    cause(unsafe { CSR.read_volatile() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flags each reset leaves (RM0368 section 6.1.2: every reset
    /// source but power-on drives the pin), one at a time and over one
    /// another.
    #[test]
    fn the_most_telling_flag_is_named() {
        let pin = bit::PINRSTF;
        for (csr, want) in [
            (bit::PORRSTF | bit::BORRSTF | pin, ResetCause::PowerOn),
            (pin, ResetCause::Pin),
            (bit::SFTRSTF | pin, ResetCause::Software),
            (bit::IWDGRSTF | pin, ResetCause::Watchdog),
            (bit::WWDGRSTF | pin, ResetCause::Watchdog),
            (bit::LPWRRSTF | pin, ResetCause::Unknown),
            // A watchdog reset, then the boot core's own reset after its
            // recovery session, before the app cleared them.
            (bit::IWDGRSTF | bit::SFTRSTF | pin, ResetCause::Watchdog),
            (0, ResetCause::Unknown),
            // The LSI bits are not reset flags.
            (0b11 | pin, ResetCause::Pin),
            // RMVF reads back as 0; were it set, it names nothing.
            (bit::RMVF | pin, ResetCause::Pin),
        ] {
            assert_eq!(cause(csr), want, "{csr:#010x}");
        }
    }
}
