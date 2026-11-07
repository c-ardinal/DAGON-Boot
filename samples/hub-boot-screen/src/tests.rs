use super::*;
use critical_section as _;
use dagon_events::{compose_boot, BootExtension};

// DAGON's boot test bench: an in-memory flash, signed test images and state
// logs (in a copy, point this path at the DAGON checkout too).
#[path = "../../../crates/dagon-core/tests/bench/mod.rs"]
mod bench;
use bench::*;

const CONFIRMED_SCREEN: Screen = Screen {
    step: 4,
    text: texts::CONFIRMED,
};

#[test]
fn each_boot_step_has_its_gauge_step_and_text() {
    let want = [
        (Stage::Armed, 1, texts::STARTING),
        (Stage::LogRead, 2, texts::STARTING),
        (Stage::Checking, 3, texts::CHECKING),
        (Stage::Chosen { trial: false }, 4, texts::CONFIRMED),
        (Stage::Chosen { trial: true }, 4, texts::TRIAL),
    ];
    for (stage, step, text) in want {
        assert_eq!(screen(stage), Screen { step, text }, "{stage:?}");
    }
    assert_eq!(STEPS, 4);
    assert_eq!(texts::STARTING, "起動しています");
    assert_eq!(texts::CHECKING, "ファームウェアを確かめています");
    assert_eq!(texts::CONFIRMED, "確定したバージョンで起動します");
    assert_eq!(texts::TRIAL, "新しいバージョンを試します");
}

#[test]
fn the_screen_uses_the_defined_words() {
    // DAGON's screen words: バージョン, never 版; ノード, never 台.
    for w in [
        texts::STARTING,
        texts::CHECKING,
        texts::CONFIRMED,
        texts::TRIAL,
    ] {
        assert!(!w.contains('版') && !w.contains('台'), "{w}");
    }
    let readme = include_str!("../README.md");
    assert!(!readme.contains('版') && !readme.contains('台'));
}

/// Records the screen of every event the boot core delivers (`None`: the
/// event draws nothing).
struct Probe<'a>(&'a mut Vec<Option<Screen>>);

impl Extension for Probe<'_> {
    fn id(&self) -> &'static str {
        "probe"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.0.push(stage(e).map(screen));
    }
}

/// The screens of one real boot core run on `m`, through to the jump or
/// the halt.
fn screens_of_a_boot(m: &mut Mem) -> Vec<Option<Screen>> {
    let mut seen = Vec::new();
    let every: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7];
    let limits: &'static [BootExtension] = Box::leak(Box::new([BootExtension::new(45_000, every)]));
    let mut t = 0;
    let clock = move || {
        t += 1;
        t
    };
    let mut chain = compose_boot(Probe(&mut seen), clock, limits);
    let mut run = dagon_boot::BootRun::start(&mut chain, m, &LOG, &SLOTS, &cfg(), 8_000);
    run.before_jump(&mut chain);
    run.halt(&mut chain);
    seen
}

const fn at(step: u8, text: &'static str) -> Option<Screen> {
    Some(Screen { step, text })
}

/// The first three events of every boot.
const FIRST: [Option<Screen>; 3] = [
    at(1, texts::STARTING),
    at(2, texts::STARTING),
    at(3, texts::CHECKING),
];

#[test]
fn a_boot_of_the_confirmed_image_ends_on_its_text() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    // ..., slot chosen (confirmed), before jump.
    let want = [&FIRST[..], &[at(4, texts::CONFIRMED), None]].concat();
    assert_eq!(screens_of_a_boot(&mut m), want);
}

#[test]
fn a_boot_of_a_new_image_on_trial_ends_on_its_text() {
    // A confirmed, B pending: B is started on trial.
    let mut m = with_pending_b();
    let want = [&FIRST[..], &[at(4, texts::TRIAL), None]].concat();
    assert_eq!(screens_of_a_boot(&mut m), want);
}

#[test]
fn a_boot_with_nothing_to_start_draws_nothing_after_the_check() {
    let mut m = Mem::new();
    // ..., no image, halt.
    let want = [&FIRST[..], &[None, None]].concat();
    assert_eq!(screens_of_a_boot(&mut m), want);
}

#[test]
fn the_glyphs_are_exactly_the_words_characters() {
    let mut want: Vec<char> = Vec::new();
    for w in [
        texts::STARTING,
        texts::CHECKING,
        texts::CONFIRMED,
        texts::TRIAL,
    ] {
        for c in w.chars() {
            if !want.contains(&c) {
                want.push(c);
            }
        }
    }
    let have: Vec<char> = glyphs::GLYPHS.iter().map(|g| g.0).collect();
    assert_eq!(have, want, "run tools/gen_glyphs.py");
    assert_eq!(glyphs::GLYPHS[0].1.len(), usize::from(CELL));
    // Every glyph has ink, inside the cell.
    for (c, rows) in glyphs::GLYPHS {
        assert!(rows.iter().any(|&r| r != 0), "{c}");
        assert!(rows.iter().all(|&r| r >> CELL == 0), "{c}");
    }
}

fn inside_circle(x: u16, y: u16, margin: u16) -> bool {
    let c = i32::from(SIZE) / 2;
    let (dx, dy) = (i32::from(x) - c, i32::from(y) - c);
    let r = c - i32::from(margin);
    dx * dx + dy * dy <= r * r
}

#[test]
fn the_screen_fits_the_round_panel() {
    for r in [TEXT, GAUGE] {
        for (x, y) in [
            (r.x, r.y),
            (r.x + r.w - 1, r.y),
            (r.x, r.y + r.h - 1),
            (r.x + r.w - 1, r.y + r.h - 1),
        ] {
            assert!(inside_circle(x, y, 8), "{r:?}");
        }
    }
    const { assert!(TEXT.y + TEXT.h <= GAUGE.y, "text above the gauge") };
    // The longest word fills the text area: a larger cell would not fit.
    let longest = [
        texts::STARTING,
        texts::CHECKING,
        texts::CONFIRMED,
        texts::TRIAL,
    ]
    .iter()
    .map(|w| w.chars().count() as u16)
    .max()
    .unwrap();
    assert_eq!(TEXT.w, longest * CELL);
    assert_eq!(TEXT.h, CELL);
    let wider = longest * (CELL + 1);
    let x0 = (SIZE - wider) / 2;
    assert!(!(inside_circle(x0, TEXT.y, 8) && inside_circle(x0 + wider - 1, TEXT.y + CELL, 8)));
}

#[test]
fn the_gauge_shows_steps_and_the_text_its_word() {
    for step in 1..=STEPS {
        let lit = (0..STEPS)
            .filter(|&i| {
                let x = GAUGE.x + i as u16 * (SEGMENT_W + SEGMENT_GAP) + SEGMENT_W / 2;
                gauge_rgb(step, x, GAUGE.y + GAUGE.h / 2) == FILLED
            })
            .count();
        assert_eq!(lit, usize::from(step));
        // Gaps between segments are background.
        let gap = GAUGE.x + SEGMENT_W + SEGMENT_GAP / 2;
        assert_eq!(gauge_rgb(step, gap, GAUGE.y), BACKGROUND);
    }
    // Each word draws ink only where its characters are, centred.
    for w in [
        texts::STARTING,
        texts::CHECKING,
        texts::CONFIRMED,
        texts::TRIAL,
    ] {
        let n = w.chars().count() as u16;
        let x0 = TEXT.x + (TEXT.w - n * CELL) / 2;
        let ink = |x| (TEXT.y..TEXT.y + TEXT.h).any(|y| text_rgb(w, x, y) == INK);
        assert!((TEXT.x..x0).all(|x| !ink(x)), "{w}");
        assert!((x0 + n * CELL..TEXT.x + TEXT.w).all(|x| !ink(x)), "{w}");
        assert!((x0..x0 + n * CELL).any(ink), "{w}");
    }
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
    // The bound of a handler that observes events.
    assert!(manifest_max_run_us() <= 50_000);
    assert!(DEADLINE_US < manifest_max_run_us());
    // The longest call: the panel's remaining reset-release wait, the init
    // commands and both areas, at the slowest byte time.
    let worst = WAIT_MAX_US + worst_call_bytes() * NS_PER_BYTE_MAX / 1000;
    assert!(worst <= DEADLINE_US, "{worst} us");
    // Never a full screen in one call.
    assert!(worst_call_bytes() < u32::from(SIZE) * u32::from(SIZE) * 2 / 4);
}

#[test]
fn the_panel_is_drawn_only_once_its_waits_fit_a_call() {
    let released = Panel::Released { at: 1_000 };
    // Right after the release: the rest of its wait is longer than a call may wait.
    assert_eq!(wait_before_drawing(released, 2_000), None);
    // Close to its end: wait the rest.
    assert_eq!(
        wait_before_drawing(released, 1_000 + RESET_RELEASE_US - 5_000),
        Some(5_000)
    );
    assert_eq!(
        wait_before_drawing(released, 1_000 + RESET_RELEASE_US + 1),
        Some(0)
    );
    // After sleep out it never waits, not even a little.
    let on = Panel::On { at: 0 };
    assert_eq!(wait_before_drawing(on, 10), None);
    assert_eq!(wait_before_drawing(on, SLEEP_OUT_US - 1), None);
    assert_eq!(wait_before_drawing(on, SLEEP_OUT_US), Some(0));
    assert_eq!(wait_before_drawing(Panel::Off, 0), None);
    // The timer's low word wraps.
    let late = Panel::Released { at: u32::MAX - 10 };
    assert_eq!(wait_before_drawing(late, RESET_RELEASE_US), Some(0));
}

#[test]
fn it_starts_with_the_panel_not_taken() {
    let x = DAGON_EXTENSION;
    assert_eq!(
        (x.panel, x.shown, x.id()),
        (Panel::Off, None, "boot-screen")
    );
    assert_eq!(CONFIRMED_SCREEN, screen(Stage::Chosen { trial: false }));
}

#[test]
fn the_readme_states_what_the_extension_adds_to_a_boot() {
    // The first event releases the reset and never draws.
    const { assert!(RESET_RELEASE_US > WAIT_MAX_US) };
    assert_eq!(DRAWS_MAX, u32::from(STEPS) - 1);
    assert_eq!(BOOT_DELAY_MAX_US, DRAWS_MAX * manifest_max_run_us());
    let ms = |us: u32| (us + 500) / 1000;
    let readme = include_str!("../README.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let want = format!(
        "at most {} ms to a boot (estimated; at most {} ms at any bus speed)",
        ms(boot_delay_estimate_us()),
        ms(BOOT_DELAY_MAX_US)
    );
    assert!(readme.contains(&want), "README: {want}");
}
