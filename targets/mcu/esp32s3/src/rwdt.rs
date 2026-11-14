//! The RTC watchdog's stage 0 hold value for a period, computed from the
//! calibrated RTC slow clock.

/// Fraction bits of the calibrated slow clock period esp-hal keeps in
/// RTC_CNTL_STORE1 (us, fixed point; esp-hal 1.2 `RtcClock::CAL_FRACT`).
pub const CAL_FRACT: u32 = 19;

/// The stage 0 hold value for `period_ms`: slow clock ticks of the period
/// at the calibrated period `cal` (us with [`CAL_FRACT`] fraction bits,
/// above 0), counted in 64 bits, shifted right by 1 + the eFuse
/// WDT_DELAY_SEL `multiplier` as esp-hal 1.2 `Rwdt::set_timeout` does. A
/// count past 32 bits is held at the longest, never cut to a shorter one.
pub const fn stage0_hold(period_ms: u32, cal: u32, multiplier: u8) -> u32 {
    let ticks = ((period_ms as u64 * 1000) << CAL_FRACT) / cal as u64;
    let ticks = if ticks > u32::MAX as u64 {
        u32::MAX
    } else {
        ticks as u32
    };
    ticks >> (1 + multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calibrated period of a slow clock running `hz`.
    const fn cal(hz: u64) -> u32 {
        ((1_000_000u64 << CAL_FRACT) / hz) as u32
    }

    /// The ticks of `ms` at `hz`, computed wide.
    fn wide(ms: u32, cal: u32, multiplier: u8) -> u64 {
        (((u64::from(ms) * 1000) << CAL_FRACT) / u64::from(cal)) >> (1 + multiplier)
    }

    /// The longest period of `[watchdog] range_ms` (3,600,000 ms) at the
    /// nominal 136 kHz RC_SLOW_CLK and at a slow clock calibrated at
    /// 1.19 MHz: the ticks fit 32 bits before the shift, and the hold value
    /// is the wide computation.
    #[test]
    fn the_longest_period_fits_32_bits_at_any_calibrated_slow_clock() {
        for hz in [136_000, 1_190_000] {
            let c = cal(hz);
            let ticks = ((3_600_000u64 * 1000) << CAL_FRACT) / u64::from(c);
            assert!(ticks <= u64::from(u32::MAX), "{hz} Hz: {ticks}");
            for m in 0..=3 {
                for ms in [1, 4_000, 3_600_000] {
                    assert_eq!(
                        u64::from(stage0_hold(ms, c, m)),
                        wide(ms, c, m),
                        "{hz} {m} {ms}"
                    );
                }
            }
        }
        assert_eq!(
            crate::WATCHDOG_RANGE_MS,
            (1, 3_600_000),
            "the range the tick count is checked for"
        );
    }

    /// A period whose ticks pass 32 bits is held at the longest count,
    /// never at a wrapped (shorter) one.
    #[test]
    fn a_count_past_32_bits_saturates_and_never_wraps() {
        let c = cal(1_190_000);
        assert!(wide(30_000_000, c, 0) > u64::from(u32::MAX >> 1));
        assert_eq!(stage0_hold(30_000_000, c, 0), u32::MAX >> 1);
        assert_eq!(stage0_hold(u32::MAX, c, 3), u32::MAX >> 4);
    }
}
