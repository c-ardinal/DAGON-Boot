//! What the watch shows: the running version, or while DAGON updates it the
//! pattern of the update phase in the node's published `AppSnapshot`
//! (dagon-agent's public snapshot, read through the `AppEvents` the start
//! returns), with a progress bar while an image arrives.

use dagon_agent::snapshot::Phase;
use dagon_mcu_esp32s3::Version;

use crate::digits::{self, Rect, MAX_CHARS, SIZE};

/// How long a result (confirmed, rolled back, failed) stays before the
/// version returns.
/// Chosen so the display stays readable; change it to suit your device.
pub const RESULT_MS: u32 = 5_000;
/// On trial the screen alternates the trial mark and the version, each
/// this long.
/// Chosen so the display stays readable; change it to suit your device.
pub const TRIAL_BLINK_MS: u32 = 1_000;

/// The progress bar: below the 4-character line, inside the round screen.
const BAR_WIDTH: u16 = 200;
const BAR_HEIGHT: u16 = 16;
const BAR_Y: u16 = 320;

/// One screen: a line of characters (`digits::layout`) and, while
/// transferring, the number the host told this node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub text: [u8; MAX_CHARS],
    pub len: usize,
    pub percent: Option<u8>,
}

impl Screen {
    pub fn chars(&self) -> &[u8] {
        &self.text[..self.len]
    }
}

/// The screen for `phase` (`told`: the number the host told this node while
/// transferring), `since_ms` after the phase began; the `version` line when
/// nothing is under way.
pub fn screen(
    phase: Phase,
    told: Option<u8>,
    since_ms: u32,
    version: Option<Version>,
) -> Screen {
    let mut s = Screen {
        text: [0; MAX_CHARS],
        len: 0,
        percent: None,
    };
    let word: &[u8] = match phase {
        // No number of this node's own competes with the host's.
        Phase::Transferring if told.is_none() => b"d---",
        Phase::Transferring => {
            let pct = told.unwrap_or_default();
            s.text[..4].copy_from_slice(&[
                b'd',
                if pct >= 100 { b'1' } else { b' ' },
                if pct >= 10 {
                    b'0' + pct / 10 % 10
                } else {
                    b' '
                },
                b'0' + pct % 10,
            ]);
            (s.len, s.percent) = (4, Some(pct));
            return s;
        }
        Phase::Verifying => b"ChEc",
        Phase::Written => b"donE",
        Phase::Pending => b"PEnd",
        Phase::RebootPending => b"boot",
        Phase::Trial if (since_ms / TRIAL_BLINK_MS).is_multiple_of(2) => b"tri",
        Phase::Confirmed if since_ms < RESULT_MS => b"ConF",
        Phase::RolledBack if since_ms < RESULT_MS => b"roLL",
        Phase::Failed if since_ms < RESULT_MS => b"FAIL",
        _ => {
            s.len = digits::text(version, &mut s.text);
            return s;
        }
    };
    s.text[..word.len()].copy_from_slice(word);
    s.len = word.len();
    s
}

/// The filled part of the progress bar for `percent`, centred under the
/// line; `None` at 0 %. Even origin and size, as the panel draws.
pub fn bar(percent: u8) -> Option<Rect> {
    let w = (u32::from(BAR_WIDTH) * u32::from(percent.min(100)) / 100) as u16;
    let w = (w + 1) & !1;
    (w > 0).then_some(Rect {
        x: ((SIZE - BAR_WIDTH) / 2) & !1,
        y: BAR_Y,
        w,
        h: BAR_HEIGHT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(phase: Phase, told: Option<u8>, since_ms: u32) -> Vec<u8> {
        screen(phase, told, since_ms, Some(Version::new(1, 2, 0)))
            .chars()
            .to_vec()
    }

    #[test]
    fn idle_shows_the_version() {
        assert_eq!(shown(Phase::Idle, None, 0), b"1.2.0");
        assert_eq!(shown(Phase::Starting, None, 0), b"1.2.0");
        let unknown = screen(Phase::Idle, None, 0, None);
        assert_eq!((unknown.chars(), unknown.percent), (&b"---"[..], None));
    }

    #[test]
    fn transfer_shows_percent_and_bar() {
        let s = screen(Phase::Transferring, Some(42), 0, None);
        assert_eq!((s.chars(), s.percent), (&b"d 42"[..], Some(42)));
        let s = screen(Phase::Transferring, Some(0), 0, None);
        assert_eq!((s.chars(), s.percent), (&b"d  0"[..], Some(0)));
        // Told nothing: no number and no bar of its own.
        let s = screen(Phase::Transferring, None, 0, None);
        assert_eq!((s.chars(), s.percent), (&b"d---"[..], None));
        // The bar only while transferring.
        assert_eq!(screen(Phase::Verifying, None, 0, None).percent, None);
    }

    #[test]
    fn each_phase_has_its_word() {
        assert_eq!(shown(Phase::Verifying, None, 0), b"ChEc");
        assert_eq!(shown(Phase::Written, None, 0), b"donE");
        assert_eq!(shown(Phase::Pending, None, 600_000), b"PEnd");
        assert_eq!(shown(Phase::RebootPending, None, 0), b"boot");
        assert_eq!(shown(Phase::Trial, None, 0), b"tri");
        assert_eq!(shown(Phase::Trial, None, TRIAL_BLINK_MS), b"1.2.0");
        for (phase, w) in [
            (Phase::Confirmed, &b"ConF"[..]),
            (Phase::RolledBack, b"roLL"),
            (Phase::Failed, b"FAIL"),
        ] {
            assert_eq!(shown(phase, None, RESULT_MS - 1), w, "{phase:?}");
            assert_eq!(shown(phase, None, RESULT_MS), b"1.2.0", "{phase:?}");
        }
    }

    #[test]
    fn words_and_bar_are_drawable_inside_the_round_screen() {
        let c = i32::from(SIZE) / 2;
        let inside = |r: &Rect| {
            [
                (r.x, r.y),
                (r.x + r.w, r.y),
                (r.x, r.y + r.h),
                (r.x + r.w, r.y + r.h),
            ]
            .iter()
            .all(|&(x, y)| {
                let (dx, dy) = (i32::from(x) - c, i32::from(y) - c);
                dx * dx + dy * dy <= c * c
            })
        };
        for w in [
            &b"d100"[..],
            b"d  0",
            b"ChEc",
            b"donE",
            b"PEnd",
            b"boot",
            b"tri",
            b"ConF",
            b"roLL",
            b"FAIL",
        ] {
            let mut out = [Rect::default(); digits::MAX_RECTS];
            let n = digits::layout(w, &mut out);
            // Every character is a 7-segment cell, none a dot.
            assert!(n >= w.len(), "{w:?}");
            assert!(out[..n].iter().all(|r| r.w > 0 && inside(r)), "{w:?}");
            // Only the transfer line has the bar under it.
            let bottom = out[..n].iter().map(|r| r.y + r.h).max().unwrap();
            assert!(w[0] != b'd' || bottom < BAR_Y, "{w:?} runs into the bar");
        }
        assert_eq!(bar(0), None);
        let full = bar(100).unwrap();
        assert_eq!(full.w, BAR_WIDTH);
        assert!(inside(&full));
        let half = bar(50).unwrap();
        assert_eq!(
            (half.x, half.y, half.w, half.h),
            (full.x, BAR_Y, 100, BAR_HEIGHT)
        );
        assert_eq!((half.x | half.y | half.w | half.h) & 1, 0);
        assert_eq!(bar(1).unwrap().w, 2);
    }
}
