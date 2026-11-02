//! `Clock`, `Watchdog` and `Reset`.

use dagon_port::{Clock, Reset, ResetCause, Watchdog};
use esp_hal::{
    delay::Delay,
    rtc_cntl::{Rwdt, RwdtStage, SocResetReason},
    time::{Duration, Instant},
};

pub struct EspClock;

impl Clock for EspClock {
    fn now_us(&self) -> u64 {
        Instant::now().duration_since_epoch().as_micros()
    }

    fn delay_us(&mut self, us: u32) {
        Delay::new().delay_micros(us);
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
}

impl Watchdog for RtcWatchdog {
    fn start(&mut self, timeout_ms: u32) {
        self.rwdt
            .set_timeout(RwdtStage::Stage0, Duration::from_millis(timeout_ms.into()));
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
