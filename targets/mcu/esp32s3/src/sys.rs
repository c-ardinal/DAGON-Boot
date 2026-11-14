//! `Clock` (DAGON's microsecond clock), `Watchdog` and `Reset`.

use dagon_events::Clock;
use dagon_port::{Reset, ResetCause, Watchdog};
use esp_hal::{
    peripherals::LPWR,
    rtc_cntl::{Rwdt, SocResetReason},
    time::Instant,
};

pub struct EspClock;

impl Clock for EspClock {
    fn now_us(&mut self) -> u64 {
        Instant::now().duration_since_epoch().as_micros()
    }
}

/// RTC watchdog, stage 0 resets the whole system (the 2nd stage runs again).
/// `esp_hal::init` stops the WDT the 2nd stage started, so an app on trial
/// must `start` this before anything that can hang.
pub struct RtcWatchdog {
    rwdt: Rwdt,
}

impl RtcWatchdog {
    /// Take `rwdt` from `esp_hal::rtc_cntl::Rtc::new(p.RTC_TIMER)`.
    pub fn new(rwdt: Rwdt) -> Self {
        Self { rwdt }
    }

    /// Disables the RWDT. It lives in the RTC domain, so it would keep
    /// running through a software reset into the ROM loader and reset it.
    pub fn stop(&mut self) {
        self.rwdt.disable();
    }
}

/// The RTC watchdog's write protection key (esp-hal 1.2 `Rwdt`).
const WDT_WKEY: u32 = 0x50D8_3AA1;

/// Sets stage 0 to `timeout_ms`, counted by [`crate::rwdt::stage0_hold`]
/// from the slow clock period esp-hal calibrated at start-up (STORE1)
/// instead of `Rwdt::set_timeout`, which cuts the count to 32 bits.
fn set_stage0(timeout_ms: u32) {
    let regs = LPWR::regs();
    let cal = regs.store1().read().bits().max(1);
    let hold = crate::rwdt::stage0_hold(timeout_ms, cal, esp_hal::efuse::rwdt_multiplier());
    regs.wdtwprotect().write(|w| unsafe { w.bits(WDT_WKEY) });
    regs.wdtconfig(0)
        .modify(|_, w| unsafe { w.hold().bits(hold) });
    regs.wdtwprotect().write(|w| unsafe { w.bits(0) });
}

impl Watchdog for RtcWatchdog {
    fn start(&mut self, timeout_ms: u32) {
        set_stage0(timeout_ms);
        self.rwdt.enable();
        self.rwdt.feed();
    }

    fn feed(&mut self) {
        self.rwdt.feed();
    }
}

pub struct EspReset;

impl Reset for EspReset {
    fn reset(&mut self) -> ! {
        esp_hal::system::software_reset()
    }

    fn reset_cause(&self) -> ResetCause {
        use SocResetReason::*;
        match esp_hal::system::reset_reason() {
            Some(ChipPowerOn | SysBrownOut | CorePwrGlitch) => ResetCause::PowerOn,
            Some(
                CoreMwdt0 | CoreMwdt1 | CoreRtcWdt | CpuMwdt0 | CpuMwdt1 | CpuRtcWdt | SysRtcWdt
                | SysSuperWdt,
            ) => ResetCause::Watchdog,
            Some(CoreSw | CpuSw) => ResetCause::Software,
            // Reset requested by the host over USB (espflash, monitor).
            Some(CoreUsbUart | CoreUsbJtag) => ResetCause::Pin,
            _ => ResetCause::Unknown,
        }
    }
}
