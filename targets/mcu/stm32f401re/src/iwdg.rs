//! The independent watchdog (IWDG) programmed for a period: stage0, the
//! boot core and the app all set it here. The period follows the LSI,
//! which varies between parts (RM0368: 17 to 47 kHz); the setting is
//! computed for its nominal 32 kHz, as embassy-stm32's driver assumes.

/// The LSI's nominal frequency, Hz (RM0368, IWDG).
const LSI_HZ: u64 = 32_000;
/// The reload counter's top: 12 bits.
const RELOAD_MAX: u64 = 0xFFF;
/// The largest prescaler register value, LSI / 256.
const PR_MAX: u32 = 6;

/// The prescaler (PR, LSI / (4 << PR)) and reload (RLR) register values
/// for `period_ms`: the smallest prescaler whose longest period holds it,
/// so the period is realized short of `period_ms` by less than one tick of
/// that prescaler. Every period of `[watchdog] range_ms`, 1..=32,768 ms,
/// is realized; a longer one gets the longest the IWDG has.
pub const fn setting(period_ms: u32) -> (u32, u32) {
    // LSI cycles of the period.
    let cycles = period_ms as u64 * LSI_HZ / 1000;
    let mut pr = 0;
    while pr < PR_MAX && cycles > (RELOAD_MAX + 1) * (4 << pr) {
        pr += 1;
    }
    let ticks = cycles / (4 << pr);
    let rl = if ticks == 0 {
        0
    } else if ticks > RELOAD_MAX + 1 {
        RELOAD_MAX
    } else {
        ticks - 1
    };
    (pr, rl as u32)
}

#[cfg(target_os = "none")]
mod regs {
    pub const KR: *mut u32 = 0x4000_3000 as *mut u32;
    pub const PR: *mut u32 = 0x4000_3004 as *mut u32;
    pub const RLR: *mut u32 = 0x4000_3008 as *mut u32;
    pub const SR: *const u32 = 0x4000_300C as *const u32;
    pub const KEY_UNLOCK: u32 = 0x5555;
    pub const KEY_RELOAD: u32 = 0xAAAA;
    pub const KEY_START: u32 = 0xCCCC;
}

/// Starts the IWDG with `period_ms` (on a running IWDG: sets the period).
#[cfg(target_os = "none")]
pub fn start(period_ms: u32) {
    // SAFETY: the fixed IWDG key register; START on a running IWDG does
    // nothing.
    unsafe { regs::KR.write_volatile(regs::KEY_START) };
    set(period_ms);
}

/// Sets the running IWDG's period to `period_ms` and reloads it.
#[cfg(target_os = "none")]
pub fn set(period_ms: u32) {
    let (pr, rl) = setting(period_ms);
    // SAFETY: fixed IWDG registers. PR and RLR take a new value only once
    // the previous update is done (SR.PVU, SR.RVU clear) and after the
    // unlock key (RM0368, IWDG).
    unsafe {
        while regs::SR.read_volatile() != 0 {}
        regs::KR.write_volatile(regs::KEY_UNLOCK);
        regs::PR.write_volatile(pr);
        regs::RLR.write_volatile(rl);
        while regs::SR.read_volatile() != 0 {}
        regs::KR.write_volatile(regs::KEY_RELOAD);
    }
}

/// Reloads the IWDG with the period it runs.
#[cfg(target_os = "none")]
pub fn feed() {
    // SAFETY: the fixed IWDG key register; the reload key only restarts
    // the countdown.
    unsafe { regs::KR.write_volatile(regs::KEY_RELOAD) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The period a setting runs at the nominal LSI, us.
    fn realized_us((pr, rl): (u32, u32)) -> u64 {
        (u64::from(rl) + 1) * (4u64 << pr) * 1_000_000 / LSI_HZ
    }

    /// Every period of `[watchdog] range_ms` (1..=32,768 ms) is realized,
    /// no longer than asked and short of it by at most one tick of the
    /// prescaler used (4 << PR LSI cycles, 0.125 ms at /4 up to 8 ms at
    /// /256). Each period of the range is checked, stage0's boundaries
    /// 1, 8,190, 8,191 and 32,768 ms among them.
    #[test]
    fn every_period_of_the_range_is_realized_within_one_tick() {
        // The longest period is not cut short.
        assert_eq!(realized_us(setting(32_768)), 32_768_000);
        assert_eq!(realized_us(setting(8_191)) / 1000, 8_190);
        for ms in 1..=32_768 {
            let s = setting(ms);
            let (pr, rl) = s;
            assert!(pr <= PR_MAX && u64::from(rl) <= RELOAD_MAX, "{ms}: {s:?}");
            let asked = u64::from(ms) * 1000;
            let tick = (4u64 << pr) * 1_000_000 / LSI_HZ;
            let got = realized_us(s);
            assert!(
                got <= asked && asked - got < tick,
                "{ms} ms: {got} us, tick {tick} us"
            );
        }
    }

    /// The smallest prescaler that holds the period: the finest tick.
    #[test]
    fn the_smallest_prescaler_that_holds_the_period_is_taken() {
        assert_eq!(setting(1), (0, 7));
        assert_eq!(setting(512), (0, 4095));
        assert_eq!(setting(513), (1, 2051));
        assert_eq!(setting(8_190), (4, 4094));
        assert_eq!(setting(8_191), (4, 4094));
        assert_eq!(setting(8_192), (4, 4095));
        assert_eq!(setting(8_193), (5, 2047));
        assert_eq!(setting(32_768), (6, 4095));
    }
}
