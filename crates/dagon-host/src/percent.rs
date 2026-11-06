//! A node's one progress number over its whole write, from the first byte to
//! its confirmation. The hub's display and the display of the node being
//! written (SHOW) and every caller read this one calculation, so they never
//! disagree.
//!
//! The transfer and the trial weigh as long as each takes: the transfer's time
//! is projected from the bytes sent so far, the trial's is the MCU definition's
//! `[watchdog] trial_ms`. Without a trial period only the bytes count. Only the
//! confirmation makes 100, and within one run the number never moves back, also
//! when a second image (the boot core, then the app) starts its own transfer. A
//! new run of the same node starts again from 0.

use dagon_proto::payload::phase;

/// Highest number before the confirmation.
const BEFORE_CONFIRMED: u8 = 99;

/// The number during the transfer, `xfer_pct` of the image's bytes sent `elapsed_ms` in.
pub fn transfer_percent(elapsed_ms: u64, xfer_pct: u8, trial_ms: Option<u32>, floor: u8) -> u8 {
    let pct = u64::from(xfer_pct);
    let p = match trial_ms {
        _ if pct == 0 => 0,
        None => pct,
        Some(trial) => ratio(elapsed_ms, elapsed_ms * 100 / pct + u64::from(trial)),
    };
    settle(p, floor)
}

/// The number after the transfer took `xfer_ms`, `on_trial_ms` into the node's trial.
pub fn trial_percent(xfer_ms: u64, on_trial_ms: u64, trial_ms: Option<u32>, floor: u8) -> u8 {
    let p = match trial_ms {
        None => u64::from(BEFORE_CONFIRMED),
        Some(trial) => {
            let trial = u64::from(trial);
            ratio(xfer_ms + on_trial_ms.min(trial), xfer_ms + trial)
        }
    };
    settle(p, floor)
}

fn ratio(part: u64, whole: u64) -> u64 {
    part * 100 / whole.max(1)
}

fn settle(p: u64, floor: u8) -> u8 {
    p.min(u64::from(BEFORE_CONFIRMED)).max(u64::from(floor)) as u8
}

/// One node's write as it goes, fed with the host's progress reports.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WriteTrack {
    node: Option<u8>,
    started_ms: u64,
    xfer_ms: Option<u64>,
    rebooted_ms: Option<u64>,
    /// What was last said: the number never goes under it.
    shown: u8,
}

impl WriteTrack {
    /// What was last said.
    pub(crate) fn shown(&self) -> u8 {
        self.shown
    }

    /// Takes one report and returns the number to show for it.
    pub(crate) fn report(
        &mut self,
        node: u8,
        ph: u8,
        percent: u8,
        boot_core: bool,
        now_ms: u64,
        trial_ms: Option<u32>,
    ) -> u8 {
        if self.node != Some(node) {
            *self = WriteTrack {
                node: Some(node),
                ..WriteTrack::default()
            };
        }
        self.shown = match ph {
            phase::TRANSFER => {
                // Every image's transfer opens with a 0 % report.
                if percent == 0 {
                    self.started_ms = now_ms;
                    self.xfer_ms = None;
                }
                let elapsed = now_ms.saturating_sub(self.started_ms);
                transfer_percent(elapsed, percent, trial_ms, self.shown)
            }
            phase::VERIFY => {
                let xfer = now_ms.saturating_sub(self.started_ms).max(1);
                self.xfer_ms = Some(xfer);
                trial_percent(xfer, 0, trial_ms, self.shown)
            }
            phase::REBOOT => {
                self.rebooted_ms = Some(now_ms);
                self.shown
            }
            phase::CONFIRM if !boot_core => 100,
            // The boot core's trial has passed: what a caller counted for it
            // stays when the next image's transfer starts.
            phase::CONFIRM => match (self.xfer_ms, self.rebooted_ms) {
                (Some(xfer), Some(at)) => {
                    trial_percent(xfer, now_ms.saturating_sub(at), trial_ms, self.shown)
                }
                _ => self.shown,
            },
            // The ROM path has no trial: its bytes are the whole write.
            phase::ROM => percent.min(BEFORE_CONFIRMED).max(self.shown),
            _ => percent,
        };
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIAL: Option<u32> = Some(4000);

    #[test]
    fn nothing_sent_is_zero() {
        assert_eq!(transfer_percent(500, 0, TRIAL, 0), 0);
    }

    #[test]
    fn transfer_weighs_as_long_as_it_projects_against_the_trial() {
        // 50 % of the bytes in 1 s: the transfer takes 2 s, then 4 s of trial.
        assert_eq!(transfer_percent(1000, 50, TRIAL, 0), 16);
    }

    #[test]
    fn trial_counts_time_and_stops_under_100() {
        assert_eq!(trial_percent(2000, 2000, TRIAL, 0), 66);
        assert_eq!(trial_percent(2000, 55_000, TRIAL, 0), 66 + 33);
    }

    #[test]
    fn without_a_trial_period_only_the_bytes_count() {
        assert_eq!(transfer_percent(1000, 40, None, 0), 40);
        assert_eq!(trial_percent(2000, 0, None, 0), 99);
    }

    #[test]
    fn the_verify_report_continues_the_transfer_number_and_never_says_100() {
        let mut t = WriteTrack::default();
        t.report(0, phase::TRANSFER, 0, false, 0, TRIAL);
        let sent = t.report(0, phase::TRANSFER, 100, false, 2000, TRIAL);
        let verified = t.report(0, phase::VERIFY, 100, false, 2000, TRIAL);
        assert_eq!((sent, verified), (33, 33));
        assert_eq!(t.report(0, phase::REBOOT, 0, false, 2100, TRIAL), 33);
        assert_eq!(t.report(0, phase::CONFIRM, 100, false, 6000, TRIAL), 100);
    }

    #[test]
    fn a_second_image_never_moves_the_number_back_and_only_the_apps_confirmation_is_100() {
        let mut t = WriteTrack::default();
        t.report(1, phase::TRANSFER, 0, true, 0, TRIAL);
        t.report(1, phase::TRANSFER, 100, true, 4000, TRIAL);
        let core = t.report(1, phase::VERIFY, 100, true, 4000, TRIAL);
        assert_eq!(t.report(1, phase::REBOOT, 0, true, 4000, TRIAL), core);
        // 4 s of transfer and 2 s of its 4 s trial: what the Console counted.
        let trial = t.report(1, phase::CONFIRM, 100, true, 6000, TRIAL);
        assert_eq!((core, trial), (50, 75));
        t.report(1, phase::TRANSFER, 0, false, 6000, TRIAL);
        assert_eq!(t.report(1, phase::TRANSFER, 10, false, 6100, TRIAL), trial);
        assert_eq!(t.report(1, phase::CONFIRM, 100, false, 9000, TRIAL), 100);
    }

    #[test]
    fn another_node_starts_again() {
        let mut t = WriteTrack::default();
        t.report(0, phase::CONFIRM, 100, false, 10, TRIAL);
        assert_eq!(t.report(1, phase::TRANSFER, 0, false, 20, TRIAL), 0);
    }
}
