//! Layout tests: every screen is rendered into a frame in memory.

extern crate std;

use std::boxed::Box;
use std::vec::Vec;

use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::renderer::TextRenderer;

use super::*;
use crate::frame::Frame;

const PEERS: [Peer; 4] = [
    Peer {
        name: "seg",
        link: "i2c0",
    },
    Peer {
        name: "watch",
        link: "pio-uart",
    },
    Peer {
        name: "third",
        link: "i2c0",
    },
    Peer {
        name: "fourth",
        link: "i2c0",
    },
];

fn home(status: Status, percent: Option<u8>, peers: usize, test_key: bool) -> Home<'static> {
    Home {
        name: "hub",
        version: Version::new(1, 10, 0),
        status,
        percent,
        peers: &PEERS[..peers],
        test_key,
    }
}

fn render(h: &Home) -> Box<Frame> {
    let mut f = Box::new(Frame::new());
    draw(&mut *f, h).unwrap();
    f
}

fn lit(f: &Frame) -> Vec<Point> {
    let n = i32::from(crate::SIZE);
    (0..n)
        .flat_map(|y| (0..n).map(move |x| Point::new(x, y)))
        .filter(|&p| f.at(p) != Rgb565::BLACK)
        .collect()
}

fn within(p: Point, at: Point, r: i32) -> bool {
    let (dx, dy) = (p.x - at.x, p.y - at.y);
    dx * dx + dy * dy <= r * r
}

#[test]
fn every_state_has_its_word_and_the_words_glyphs() {
    for s in Status::ALL {
        let w = s.word();
        assert!(!w.is_empty(), "{s:?}");
        assert!(w.chars().all(|c| glyph(c).is_some()), "{s:?}: {w}");
        assert!(texts::WORDS.contains(&w), "{w}");
    }
    // The word tells the state; colour only repeats it.
    let words: Vec<_> = Status::ALL.iter().map(|s| s.word()).collect();
    for (i, w) in words.iter().enumerate() {
        assert!(!words[..i].contains(w), "{w} twice");
    }
}

#[test]
fn everything_lies_inside_the_round_panel() {
    let c = Point::new(120, 120);
    for s in Status::ALL {
        for peers in 0..=PEERS.len() {
            for percent in [None, Some(0), Some(50), Some(100)] {
                let f = render(&home(s, percent, peers, true));
                for p in lit(&f) {
                    assert!(within(p, c, 120), "{s:?} {peers} {percent:?}: {p:?}");
                }
            }
        }
    }
}

#[test]
fn the_state_word_sits_between_the_rings() {
    let outside = |at: Point, d: u32| {
        let r = (d / 2 + RING) as i32;
        move |p: &Point| !within(*p, at, r)
    };
    for s in Status::ALL {
        let pixels: Vec<_> = word_pixels(s.word(), Point::new(OWN_AT.x, WORD_TOP)).collect();
        assert!(!pixels.is_empty(), "{s:?}");
        assert!(pixels.iter().all(outside(OWN_AT, OWN_D)), "{s:?}");
        for &at in peers_at(3) {
            assert!(pixels.iter().all(outside(at, PEER_D)), "{s:?} {at:?}");
        }
    }
}

#[test]
fn at_most_three_peers_are_shown() {
    assert_eq!(peers_at(4).len(), 3);
    let three = lit(&render(&home(Status::Confirmed, None, 3, false)));
    let four = lit(&render(&home(Status::Confirmed, None, 4, false)));
    assert_eq!(three, four);
}

#[test]
fn the_ring_shows_the_write_s_progress() {
    // Just right of 12 o'clock, on the ring.
    let top = OWN_AT - Point::new(-4, OWN_D as i32 / 2);
    let at = |percent| render(&home(Status::Writing, Some(percent), 0, false)).at(top);
    assert_eq!(at(50), BLUE);
    assert_eq!(at(0), DIM);
    // Without a percentage the ring takes the state's colour.
    let f = render(&home(Status::Writing, None, 0, false));
    assert_eq!(f.at(top), BLUE);
    let f = render(&home(Status::Confirmed, Some(50), 0, false));
    assert_eq!(f.at(top), GREEN);
}

#[test]
fn test_key_sits_clear_of_every_ring() {
    let style = MonoTextStyle::new(&FONT_10X20, RED);
    let bounds = style
        .measure_string(
            "TEST KEY",
            TEST_KEY_AT,
            embedded_graphics::text::Baseline::Alphabetic,
        )
        .bounding_box;
    let bounds = Rectangle::new(
        bounds.top_left - Point::new(bounds.size.width as i32 / 2, 0),
        bounds.size,
    );
    for &at in peers_at(3) {
        let r = (PEER_D / 2 + RING) as i32;
        assert!(bounds.points().all(|p| !within(p, at, r)), "{at:?}");
    }
}
