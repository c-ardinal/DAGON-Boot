//! What the 4 digits show while DAGON updates this node: one pattern per
//! phase of the node's published `AppSnapshot` (dagon-agent's public
//! snapshot, read through the `AppEvents` the start returns). `None` is the
//! normal display (the version).

use core::fmt::Write;

use dagon_agent::snapshot::Phase;

use crate::seg::{render, Buf, DIGITS};

/// How long a result (confirmed, rolled back, failed) stays before the
/// normal display returns.
/// Demo readability value, pending owner confirmation.
pub const RESULT_MS: u32 = 5_000;
/// On trial the display alternates the trial mark and the version, each
/// this long.
/// Demo readability value, pending owner confirmation.
pub const TRIAL_BLINK_MS: u32 = 1_000;

/// The pattern for `phase` (`progress`: bytes received and expected while
/// transferring), `since_ms` after the phase began; `None`: the version.
pub fn pattern(phase: Phase, progress: Option<(u32, u32)>, since_ms: u32) -> Option<[u8; DIGITS]> {
    let word = match phase {
        Phase::Transferring => return percent(progress),
        Phase::Verifying => "ChEc",
        Phase::Written => "donE",
        Phase::Pending => "PEnd",
        Phase::RebootPending => "boot",
        Phase::Trial if (since_ms / TRIAL_BLINK_MS).is_multiple_of(2) => "tri",
        Phase::Confirmed if since_ms < RESULT_MS => "ConF",
        Phase::RolledBack if since_ms < RESULT_MS => "roLL",
        Phase::Failed if since_ms < RESULT_MS => "FAIL",
        _ => return None,
    };
    render(word)
}

/// `d` and the share received, 0 to 100 %.
fn percent(progress: Option<(u32, u32)>) -> Option<[u8; DIGITS]> {
    let pct = match progress {
        Some((done, total)) if total > 0 => (u64::from(done) * 100 / u64::from(total)).min(100),
        _ => 0,
    };
    let mut buf = Buf::default();
    write!(buf, "d{pct:>3}").ok()?;
    buf.as_str().and_then(render)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(s: &str) -> Option<[u8; DIGITS]> {
        Some(render(s).unwrap())
    }

    #[test]
    fn idle_and_starting_show_the_version() {
        assert_eq!(pattern(Phase::Idle, None, 0), None);
        assert_eq!(pattern(Phase::Starting, None, 0), None);
    }

    #[test]
    fn transfer_shows_its_percentage() {
        let at = |done, total| pattern(Phase::Transferring, Some((done, total)), 0);
        assert_eq!(at(0, 1000), word("d  0"));
        assert_eq!(at(421, 1000), word("d 42"));
        assert_eq!(at(1000, 1000), word("d100"));
        // A large image does not overflow; nothing known yet is 0 %.
        assert_eq!(at(u32::MAX, u32::MAX), word("d100"));
        assert_eq!(at(5, 0), word("d  0"));
        assert_eq!(pattern(Phase::Transferring, None, 0), word("d  0"));
    }

    #[test]
    fn each_step_of_an_update_has_its_word() {
        assert_eq!(pattern(Phase::Verifying, None, 0), word("ChEc"));
        assert_eq!(pattern(Phase::Written, None, 0), word("donE"));
        assert_eq!(pattern(Phase::Pending, None, 0), word("PEnd"));
        assert_eq!(pattern(Phase::RebootPending, None, 0), word("boot"));
        // They stay for as long as the phase does.
        assert_eq!(pattern(Phase::Pending, None, 600_000), word("PEnd"));
    }

    #[test]
    fn trial_alternates_with_the_version() {
        assert_eq!(pattern(Phase::Trial, None, 0), word("tri"));
        assert_eq!(pattern(Phase::Trial, None, TRIAL_BLINK_MS), None);
        assert_eq!(
            pattern(Phase::Trial, None, 2 * TRIAL_BLINK_MS + 1),
            word("tri")
        );
    }

    #[test]
    fn results_show_for_a_while_then_the_version() {
        for (phase, w) in [
            (Phase::Confirmed, "ConF"),
            (Phase::RolledBack, "roLL"),
            (Phase::Failed, "FAIL"),
        ] {
            assert_eq!(pattern(phase, None, 0), word(w), "{phase:?}");
            assert_eq!(pattern(phase, None, RESULT_MS - 1), word(w), "{phase:?}");
            assert_eq!(pattern(phase, None, RESULT_MS), None, "{phase:?}");
        }
    }

    #[test]
    fn every_phase_looks_different() {
        let shown = [
            pattern(Phase::Transferring, Some((1, 2)), 0),
            pattern(Phase::Verifying, None, 0),
            pattern(Phase::Written, None, 0),
            pattern(Phase::Pending, None, 0),
            pattern(Phase::RebootPending, None, 0),
            pattern(Phase::Trial, None, 0),
            pattern(Phase::Confirmed, None, 0),
            pattern(Phase::RolledBack, None, 0),
            pattern(Phase::Failed, None, 0),
        ];
        for (i, a) in shown.iter().enumerate() {
            assert!(a.is_some(), "phase {i} shows the version");
            for b in &shown[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
