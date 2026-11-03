//! Layout tests: every screen is rendered into memory. With
//! `DAGON_SCREEN_OUT=<dir>` the renders are also written there as PPM.

extern crate std;

use std::format;
use std::vec::Vec;

use dagon_format::Version;
use dagon_mcu_rp2040::nodes::{named, HUB};
use dagon_mcu_rp2040::view::{Home, Node, Progress, Shade};
use dagon_proto::payload::{link, phase};
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::pixelcolor::RgbColor;
use embedded_graphics::primitives::Rectangle;

use super::*;

const SIZE: i32 = 240;

struct Canvas(Vec<Rgb565>);

impl Canvas {
    fn new() -> Self {
        Canvas(std::vec![Rgb565::BLACK; (SIZE * SIZE) as usize])
    }

    fn at(&self, p: Point) -> Rgb565 {
        self.0[(p.y * SIZE + p.x) as usize]
    }

    /// Writes a binary PPM to `$DAGON_SCREEN_OUT/<name>.ppm` if set.
    fn save(&self, name: &str) {
        let Ok(dir) = std::env::var("DAGON_SCREEN_OUT") else {
            return;
        };
        let mut out = format!("P6 {SIZE} {SIZE} 255\n").into_bytes();
        for c in &self.0 {
            out.extend([c.r() << 3, c.g() << 2, c.b() << 3]);
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(SIZE as u32, SIZE as u32)
    }
}

impl DrawTarget for Canvas {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        px: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in px {
            if (0..SIZE).contains(&p.x) && (0..SIZE).contains(&p.y) {
                self.0[(p.y * SIZE + p.x) as usize] = c;
            }
        }
        Ok(())
    }
}

fn node(index: u8, name: &str, shade: Shade) -> Node {
    Node {
        node: named(index, name, link::I2C0, 0x40 + index),
        version: Some(Version::new(1, 12, 3)),
        shade,
    }
}

fn home(subs: &[Shade]) -> Home {
    let mut h = Home {
        hub: Node {
            node: HUB,
            version: Some(Version::new(1, 0, 0)),
            shade: Shade::Confirmed,
        },
        ..Home::default()
    };
    for (i, &s) in subs.iter().enumerate() {
        let _ = h
            .subs
            .push(node(i as u8 + 1, ["seg", "watch", "mesh"][i], s));
    }
    h
}

/// Every state and phase word the screen can show.
fn all_words() -> Vec<&'static str> {
    let shades = [
        Shade::Unknown,
        Shade::Confirmed,
        Shade::Trial,
        Shade::NoAnswer,
        Shade::RolledBack,
    ];
    let mut w: Vec<&str> = shades.iter().map(|&s| state_word(s)).collect();
    let texts = [
        "E_SIGNATURE",
        "E_KEY",
        "E_COMPAT",
        "E_DIGEST",
        "ROLLED BACK",
        "NO ANSWER",
        "",
    ];
    for ph in [
        phase::VERIFY,
        phase::REBOOT,
        phase::CONFIRM,
        phase::REJECTED,
        phase::FAILED,
        phase::ROM,
    ] {
        for t in texts {
            w.push(phase_word(ph, t));
        }
    }
    w.sort();
    w.dedup();
    w
}

#[test]
fn every_word_has_its_glyphs() {
    for w in all_words() {
        for c in w.chars().filter(|c| !c.is_ascii()) {
            assert!(
                GLYPHS.iter().any(|(g, _)| *g == c),
                "no glyph for {c} in {w}"
            );
        }
    }
}

#[test]
fn words_fit_inside_the_rings() {
    for (at, d, font) in [(HUB_AT, HUB_D, &FONT_10X20), (BOTTOM, SUB_D, &FONT_6X10)] {
        let inner = (d / 2 - RING / 2 - 1) as i32;
        for w in all_words().into_iter().filter(|w| !w.is_ascii()) {
            for p in word_pixels(w, word_top(at, font)) {
                let (dx, dy) = (p.x - at.x, p.y - at.y);
                assert!(
                    dx * dx + dy * dy <= inner * inner,
                    "{w} leaves the ring at {p:?}"
                );
            }
        }
    }
}

/// The area "TEST KEY" covers.
fn test_key_box() -> Rectangle {
    Text::with_alignment("TEST KEY", TEST_KEY_AT, test_key_style(), Alignment::Center)
        .bounding_box()
}

#[test]
fn test_key_label_is_clear_of_zero_to_three_sub_nodes() {
    let label = test_key_box();
    for n in 0..=3 {
        // The longest state words, under every ring.
        let subs = std::vec![Shade::RolledBack; n];
        let mut c = Canvas::new();
        draw(&mut c, &home(&subs), false).unwrap();
        for p in label.points() {
            assert_eq!(
                c.at(p),
                Rgb565::BLACK,
                "{n} sub nodes: {p:?} under TEST KEY"
            );
        }
        // Inside the round panel.
        for p in [label.top_left, label.bottom_right().unwrap()] {
            let (dx, dy) = (p.x - 120, p.y - 120);
            assert!(dx * dx + dy * dy < 120 * 120, "{p:?} off the panel");
        }
        let mut c = Canvas::new();
        draw(&mut c, &home(&subs), true).unwrap();
        c.save(&format!("home-{n}-subs-test-key"));
    }
}

#[test]
fn renders_of_every_state_and_phase_word() {
    let mut c = Canvas::new();
    draw(
        &mut c,
        &home(&[Shade::Trial, Shade::NoAnswer, Shade::RolledBack]),
        false,
    )
    .unwrap();
    c.save("home-trial-silent-rolledback");
    for (i, (ph, text)) in [
        (phase::REJECTED, "E_SIGNATURE"),
        (phase::REJECTED, "E_KEY"),
        (phase::FAILED, "ROLLED BACK"),
        (phase::VERIFY, ""),
        (phase::REBOOT, ""),
        (phase::CONFIRM, ""),
    ]
    .into_iter()
    .enumerate()
    {
        let mut h = home(&[Shade::Confirmed, Shade::Confirmed, Shade::Confirmed]);
        let mut p = Progress {
            node: 2,
            phase: ph,
            percent: 100,
            version: Version::new(1, 13, 0),
            ..Progress::default()
        };
        let _ = p.text.push_str(text);
        h.progress = Some(p);
        let mut c = Canvas::new();
        draw(&mut c, &h, false).unwrap();
        // The word is drawn in the phase colour under the node.
        let word = phase_word(ph, text);
        let top = word_top(BOTTOM, &FONT_6X10);
        if !word.is_ascii() {
            assert!(
                word_pixels(word, top).all(|q| c.at(q) == phase_colour(ph)),
                "{word}"
            );
        }
        c.save(&format!("phase-{i}"));
    }
}
