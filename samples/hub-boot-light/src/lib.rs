//! A trusted boot extension (copied from targets/boot-extension-template)
//! that makes the boot core's steps visible on the hub: the backlight of
//! the round LCD (the board's `[boot] led`, GPIO25) flashes once when the
//! watchdog is armed and once when the state log is read, stays lit while
//! the boot core checks the image it will start, then goes dark for a
//! confirmed image, or flickers and stays lit for an image on trial, until
//! the app takes the pin over. With nothing startable the boot
//! core's own halt blink takes over.
//!
//! Each call stays within the manifest's `max_run_us` (45 ms; a handler of
//! an observer is bounded by 50 ms), so the whole show adds at most about
//! 80 ms to a boot. It changes nothing the boot core decides.
#![no_std]

use dagon_boot::sequence;
use dagon_events::{Ctx, Event, Extension};

/// What the boot core links: `boot_wiring.rs` names these two.
pub type DagonExtension = BootLight;
pub const DAGON_EXTENSION: DagonExtension = BootLight { ready: false };

/// The boot steps this extension shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Armed,
    LogRead,
    Checking,
    Chosen { trial: bool },
}

/// Backlight on or off, then held for `us` microseconds.
pub type Step = (bool, u32);

const FLASH: [Step; 2] = [(true, 30_000), (false, 10_000)];
const CHECKING: [Step; 1] = [(true, 0)];
const CONFIRMED: [Step; 1] = [(false, 0)];
const TRIAL: [Step; 4] = [(false, 10_000), (true, 10_000), (false, 10_000), (true, 0)];

/// The longest pattern of one call.
pub const LONGEST_US: u32 = 40_000;

/// The backlight pattern of `stage`; the last step's level stays until
/// the next stage (or the app) changes it.
pub fn pattern(stage: Stage) -> &'static [Step] {
    match stage {
        Stage::Armed | Stage::LogRead => &FLASH,
        Stage::Checking => &CHECKING,
        Stage::Chosen { trial: false } => &CONFIRMED,
        Stage::Chosen { trial: true } => &TRIAL,
    }
}

fn stage(e: &Event<'_>) -> Option<Stage> {
    if e.get::<sequence::WatchdogArmed>().is_some() {
        Some(Stage::Armed)
    } else if e.get::<sequence::LogRead>().is_some() {
        Some(Stage::LogRead)
    } else if e.get::<sequence::BeforeDecision>().is_some() {
        Some(Stage::Checking)
    } else {
        e.get::<sequence::SlotChosen>()
            .map(|c| Stage::Chosen { trial: c.trial })
    }
}

pub struct BootLight {
    /// The backlight pin is taken (output).
    ready: bool,
}

impl Extension for BootLight {
    fn id(&self) -> &'static str {
        // The manifest's id.
        "boot-light"
    }

    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        let Some(stage) = stage(e) else { return };
        if !self.ready {
            self.ready = hw::take();
        }
        if self.ready {
            pattern(stage).iter().for_each(|&(on, us)| hw::show(on, us));
        }
    }
}

#[cfg(target_os = "none")]
mod hw {
    use dagon_mcu_rp2040::arm::led;

    /// TIMERAWL: the 1 MHz TIMER's low word, counting since HAL init
    /// (RP2040 datasheet 4.6.5, TIMER at 0x40054000).
    const TIMERAWL: *const u32 = 0x4005_4028 as *const u32;

    fn now_us() -> u32 {
        // SAFETY: read-only access to a fixed RP2040 timer register.
        unsafe { TIMERAWL.read_volatile() }
    }

    /// Takes the LED the boot core selected from the board's `[boot] led`.
    pub fn take() -> bool {
        let Some(gpio) = led::selected() else {
            return false;
        };
        led::init(gpio, led::active_high());
        true
    }

    pub fn show(on: bool, us: u32) {
        led::set(on);
        let start = now_us();
        while now_us().wrapping_sub(start) < us {}
    }
}

/// Host builds (tests) drive no pin.
#[cfg(not(target_os = "none"))]
mod hw {
    pub fn take() -> bool {
        false
    }

    pub fn show(_on: bool, _us: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use critical_section as _;

    const ALL: [Stage; 5] = [
        Stage::Armed,
        Stage::LogRead,
        Stage::Checking,
        Stage::Chosen { trial: false },
        Stage::Chosen { trial: true },
    ];

    fn total(p: &[Step]) -> u32 {
        p.iter().map(|&(_, us)| us).sum()
    }

    fn manifest_max_run_us() -> u32 {
        include_str!("../dagon-extension.toml")
            .lines()
            .find_map(|l| l.strip_prefix("max_run_us = "))
            .and_then(|v| v.trim().parse().ok())
            .expect("max_run_us in the manifest")
    }

    #[test]
    fn every_call_fits_its_budget() {
        let longest = ALL.iter().map(|&s| total(pattern(s))).max().unwrap();
        assert_eq!(longest, LONGEST_US);
        assert!(LONGEST_US < manifest_max_run_us());
        // The bound of a handler that observes events.
        assert!(manifest_max_run_us() <= 50_000);
    }

    #[test]
    fn steps_are_visible_and_the_outcome_differs() {
        // Each flash is lit long enough to be seen and ends dark, so two
        // flashes in a row read as two.
        assert_eq!(pattern(Stage::Armed), [(true, 30_000), (false, 10_000)]);
        assert_eq!(pattern(Stage::LogRead), pattern(Stage::Armed));
        let last = |s| pattern(s).last().unwrap().0;
        assert!(last(Stage::Checking));
        assert!(!last(Stage::Chosen { trial: false }));
        assert!(last(Stage::Chosen { trial: true }));
        assert!(pattern(Stage::Chosen { trial: true }).len() > 2);
    }

    #[test]
    fn it_starts_with_the_pin_not_taken() {
        let x = DAGON_EXTENSION;
        assert_eq!((x.ready, x.id()), (false, "boot-light"));
    }
}
