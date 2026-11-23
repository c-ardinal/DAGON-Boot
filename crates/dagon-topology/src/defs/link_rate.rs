//! How a link's rate comes from a clock: the UART's bit rate (V-26) and the
//! I2C controller's SCL (V-20). Each takes its clock the same way (a fixed
//! `clock_hz`, or `clock = "app"`: the core clock of the node's
//! `[[app.clock]]` setup with its `tolerance_ppm`), and each states, as
//! definition data, how far its rate may run from the one asked for
//! (`fast_ppm`, `slow_ppm`).

use serde::Deserialize;

use super::{BoardDef, ClockKind, ClockSetup, ClockStage, McuDef};

/// How the UART the MCU package's firmware runs DAGON's UART links on makes
/// a bit rate: its clock divided by `clocks_per_bit` times a divider with
/// `frac_bits` fraction bits, the divider rounded down (as the HAL the
/// package builds with computes it). Its clock is `clock_hz`, a frequency
/// no setup changes, or `clock = "app"`, the core clock of the
/// `[[app.clock]]` setup the definitions choose for the node. `bauds` lists
/// the bit rates that firmware runs its link at: a link's `baud` must be
/// one of them on both ends (V-26), and the Console offers only those, so
/// no speed the definition does not name can be set or built. `fast_ppm`
/// and `slow_ppm` bound how far the bit rate may run from `baud`, its
/// clock's tolerance included.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkUart {
    pub clock_hz: Option<u32>,
    pub clock: Option<LinkClock>,
    pub clocks_per_bit: u32,
    pub frac_bits: u8,
    pub bauds: Vec<u32>,
    pub fast_ppm: u32,
    pub slow_ppm: u32,
}

/// `clock`: the clock a link's rate comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkClock {
    /// The core clock of the node's `[[app.clock]]` setup.
    App,
}

/// The clock a node's link runs on: its frequency and how far it may be off
/// (a fraction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkClockOn {
    pub hz: u32,
    pub tolerance: f64,
}

/// The I2C bus speeds the MCU package's firmware runs DAGON's I2C link at,
/// as controller or target. A bus's `speed_hz` must be one of them on every
/// node on the bus (V-20), and the Console offers only those, so no speed
/// the definition does not name can be set or built. `controller`: how that
/// firmware makes SCL when the node drives the bus.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkI2c {
    pub hzs: Vec<u32>,
    pub controller: Option<I2cController>,
}

/// `[link_i2c.controller]`: how the firmware makes SCL as the bus
/// controller, as the HAL the package builds with computes it: its clock
/// divided by a whole number of clock cycles a period, `speed_hz`'s period
/// rounded to the nearest cycle, which the HAL refuses below `min_period`.
/// The clock is `clock_hz` or `clock = "app"`, as `[link_uart]`'s;
/// `fast_ppm` and `slow_ppm` bound how far SCL may run from `speed_hz`, its
/// clock's tolerance included.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I2cController {
    pub clock_hz: Option<u32>,
    pub clock: Option<LinkClock>,
    pub min_period: u32,
    pub fast_ppm: u32,
    pub slow_ppm: u32,
}

/// How far a link's rate may run from the one asked for: `fast` above it,
/// `slow` below it (fractions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimit {
    pub fast: f64,
    pub slow: f64,
}

impl RateLimit {
    fn of(fast_ppm: u32, slow_ppm: u32) -> RateLimit {
        RateLimit {
            fast: f64::from(fast_ppm) / 1e6,
            slow: f64::from(slow_ppm) / 1e6,
        }
    }

    /// Whether a rate `error` off (a fraction, + fast) on a clock up to
    /// `tolerance` off stays within the limit either way.
    pub fn admits(&self, error: f64, tolerance: f64) -> bool {
        error + tolerance <= self.fast && tolerance - error <= self.slow
    }

    /// The limit as a message states it: "within 2 %", or "from -100 % to
    /// +0 %" when the two sides differ.
    pub fn describe(&self) -> String {
        if self.fast == self.slow {
            format!("within {} %", percent(self.fast))
        } else {
            format!(
                "from -{} % to +{} %",
                percent(self.slow),
                percent(self.fast)
            )
        }
    }
}

/// A fraction as a percentage, to two places without trailing zeros (0.02
/// is "2", 0.001 is "0.1").
fn percent(f: f64) -> String {
    let s = format!("{:.2}", f * 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl LinkUart {
    /// Whether `baud` is one this firmware runs its link at.
    pub fn runs(&self, baud: u32) -> bool {
        self.bauds.contains(&baud)
    }

    /// How far its bit rate may run from `baud`.
    pub fn limit(&self) -> RateLimit {
        RateLimit::of(self.fast_ppm, self.slow_ppm)
    }

    /// The bit rate this UART runs at on a `clock_hz` clock when asked for
    /// `baud`; `None` when it cannot divide its clock down to it (0, or
    /// faster than the clock).
    pub fn actual_baud(&self, clock_hz: u32, baud: u32) -> Option<f64> {
        let per_bit = u64::from(self.clocks_per_bit) * u64::from(baud);
        let steps = (u64::from(clock_hz) << self.frac_bits).checked_div(per_bit)?;
        if steps >> self.frac_bits == 0 {
            return None;
        }
        let divider = steps as f64 / f64::from(1u32 << self.frac_bits);
        Some(f64::from(clock_hz) / (f64::from(self.clocks_per_bit) * divider))
    }

    /// How far [`LinkUart::actual_baud`] is from `baud`, as a fraction
    /// (+0.0122 is 1.22 % fast).
    pub fn baud_error(&self, clock_hz: u32, baud: u32) -> Option<f64> {
        self.actual_baud(clock_hz, baud)
            .map(|a| a / f64::from(baud) - 1.0)
    }
}

impl I2cController {
    /// How far SCL may run from `speed_hz`.
    pub fn limit(&self) -> RateLimit {
        RateLimit::of(self.fast_ppm, self.slow_ppm)
    }

    /// The SCL frequency this controller runs on a `clock_hz` clock when
    /// asked for `hz`; `None` when it cannot (0, or a period shorter than
    /// `min_period` cycles).
    pub fn actual_hz(&self, clock_hz: u32, hz: u32) -> Option<f64> {
        let hz = u64::from(hz);
        let period = (u64::from(clock_hz) + hz / 2).checked_div(hz)?;
        if period < u64::from(self.min_period.max(1)) {
            return None;
        }
        Some(f64::from(clock_hz) / period as f64)
    }

    /// How far [`I2cController::actual_hz`] is from `hz`, as a fraction.
    pub fn hz_error(&self, clock_hz: u32, hz: u32) -> Option<f64> {
        self.actual_hz(clock_hz, hz)
            .map(|a| a / f64::from(hz) - 1.0)
    }
}

/// The clock an app setup gives a link: its core clock, bounded by its
/// tolerance; an internal oscillator that states none is refused.
fn clock_of(s: &ClockSetup, what: &str) -> Result<LinkClockOn, String> {
    match (s.source, s.tolerance_ppm) {
        (ClockKind::Internal, None) => Err(format!(
            "runs its {what} on the internal oscillator, whose frequency no tolerance bounds; \
             a board that states its crystal ([clock] source) can run the link"
        )),
        (_, ppm) => Ok(LinkClockOn {
            hz: s.out_hz,
            tolerance: f64::from(ppm.unwrap_or(0)) / 1e6,
        }),
    }
}

impl McuDef {
    /// The clock a link section (`section`, its `clock_hz` and `clock`)
    /// runs on for a node on `board` (`None`: no board, so the internal
    /// oscillator); an error says why it has none a link can rely on.
    fn link_clock(
        &self,
        what: &str,
        section: &str,
        clock_hz: Option<u32>,
        clock: Option<LinkClock>,
        board: Option<&BoardDef>,
    ) -> Result<LinkClockOn, String> {
        match (clock_hz, clock) {
            (Some(hz), _) => Ok(LinkClockOn { hz, tolerance: 0.0 }),
            (None, Some(LinkClock::App)) => self
                .clock_for(ClockStage::App, board)
                .ok_or_else(|| "cannot be checked: its MCU definition has no [[app.clock]]".into())
                .and_then(|s| clock_of(s, what)),
            (None, None) => Err(format!("cannot be checked: its {section} names no clock")),
        }
    }

    /// The clock this MCU's UART runs on for a node on `board`.
    pub fn uart_clock(&self, board: Option<&BoardDef>) -> Result<LinkClockOn, String> {
        let u = self
            .link_uart
            .as_ref()
            .ok_or("cannot be checked: its MCU definition has no [link_uart]")?;
        self.link_clock("UART", "[link_uart]", u.clock_hz, u.clock, board)
    }

    /// The clock this MCU's I2C controller makes SCL from for a node on
    /// `board`.
    pub fn i2c_controller_clock(&self, board: Option<&BoardDef>) -> Result<LinkClockOn, String> {
        let c = self
            .link_i2c
            .as_ref()
            .and_then(|l| l.controller.as_ref())
            .ok_or("cannot be checked as the bus controller: its MCU definition has no [link_i2c.controller]")?;
        self.link_clock(
            "I2C controller",
            "[link_i2c.controller]",
            c.clock_hz,
            c.clock,
            board,
        )
    }
}
