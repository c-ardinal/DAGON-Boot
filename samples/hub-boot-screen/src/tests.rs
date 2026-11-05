use super::*;
use critical_section as _;
use dagon_core::boot::{BootConfig, SlotLayout};
use dagon_core::log::LogLayout;
use dagon_events::{compose_boot, BootExtension};
use dagon_port::{Flash, FlashError, FlashRegion};

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
    assert_eq!(texts::CONFIRMED, "確定した版で起動します");
    assert_eq!(texts::TRIAL, "新しい版を試します");
}

/// An erased flash: no state log, so the boot core finds nothing to start.
struct Blank;

const REGION: [FlashRegion; 1] = [FlashRegion {
    start: 0x1000_0000,
    len: 0x40_0000,
    erase_size: 0x1000,
    write_size: 256,
    erased_value: 0xFF,
}];

impl Flash for Blank {
    fn geometry(&self) -> &[FlashRegion] {
        &REGION
    }
    fn read(&mut self, _: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.fill(0xFF);
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Ok(())
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        Ok(())
    }
}

/// Records what `stage` makes of every event the boot core delivers.
struct Probe<'a>(&'a mut [Option<Stage>; 8], usize);

impl Extension for Probe<'_> {
    fn id(&self) -> &'static str {
        "probe"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.0[self.1] = stage(e);
        self.1 += 1;
    }
}

#[test]
fn the_boot_cores_events_reach_their_steps_and_no_image_draws_nothing() {
    let mut seen = [None; 8];
    let every: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7];
    let limits: &'static [BootExtension] = Box::leak(Box::new([BootExtension::new(45_000, every)]));
    let mut t = 0;
    {
        let mut chain = compose_boot(
            Probe(&mut seen, 0),
            move || {
                t += 1;
                t
            },
            limits,
        );
        let cfg = BootConfig {
            pubkey: [0; 32],
            mcu_id: 1,
            target_tag: *b"hub-rp2040-lcd\0\0",
            max_attempts: 3,
        };
        let log = LogLayout {
            faces: [0x1000_6000, 0x1000_7000],
            face_len: 0x1000,
        };
        let slots = SlotLayout {
            slots: [0x1004_8000, 0x1014_8000],
            slot_len: 0x10_0000,
        };
        let mut run = dagon_boot::BootRun::start(&mut chain, &mut Blank, &log, &slots, &cfg, 8_000);
        run.halt(&mut chain);
    }
    // watchdog armed, log read, before decision, no image, halt.
    assert_eq!(
        seen[..5],
        [
            Some(Stage::Armed),
            Some(Stage::LogRead),
            Some(Stage::Checking),
            None,
            None
        ]
    );
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
    let on = Panel::On { at: 0 };
    assert_eq!(wait_before_drawing(on, 10), None);
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
