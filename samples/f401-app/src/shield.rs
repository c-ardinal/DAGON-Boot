//! The X-NUCLEO-53L0A1's display expanders: two STMPE1600 on I2C1 drive the
//! 28 segments statically (common anode at +3.3 V, a segment lights while
//! its GPIO is low) and the three VL53L0X XSHUT lines (low = sensor held
//! off). Sources: ST UM2047 Figure 4 and X-CUBE-53L0A1; STMPE1600 datasheet.
//!
//! | Expander | GPIO 0..6 | GPIO 7..13 | GPIO 14 | GPIO 15 |
//! |---|---|---|---|---|
//! | U21 at [`LEFT`] | digit 1 | digit 2 | XSHUT left | XSHUT right |
//! | U19 at [`RIGHT`] | digit 3 | digit 4 | unused (input) | XSHUT centre |
//!
//! Within a digit, GPIO n+0..n+6 = segments e, f, g, a, d, b, c. No digit
//! point is wired.

use crate::seg::DIGITS;

/// U21: left digits 1-2, satellite XSHUTs (7-bit address).
pub const LEFT: u8 = 0x43;
/// U19: right digits 3-4, centre XSHUT (7-bit address).
pub const RIGHT: u8 = 0x42;

/// GPIO set state (output level, 1 = high), LSB first.
pub const GPSR: u8 = 0x12;
/// GPIO direction (1 = output), LSB first.
pub const GPDR: u8 = 0x14;

/// The 14 segment GPIOs of either expander.
const SEGMENTS: u16 = 0x3FFF;
const XSHUT_LEFT: u16 = 1 << 14;
const XSHUT_RIGHT: u16 = 1 << 15;
const XSHUT_CENTRE: u16 = 1 << 15;

/// GPIO offset of segments a..g within a digit.
const SEGMENT_GPIO: [u8; 7] = [3, 5, 6, 4, 0, 1, 2];

/// One I2C write: the expander's address and [register, LSB, MSB] (the
/// STMPE1600 increments the register after each byte).
pub type Write = (u8, [u8; 3]);

const fn write(addr: u8, reg: u8, value: u16) -> Write {
    (addr, [reg, value as u8, (value >> 8) as u8])
}

/// Start-up: every segment off and every XSHUT low, levels set before the
/// pins turn outputs (outputs first would flash all segments on).
pub const INIT: [Write; 4] = [
    write(RIGHT, GPSR, SEGMENTS),
    write(RIGHT, GPDR, SEGMENTS | XSHUT_CENTRE),
    write(LEFT, GPSR, SEGMENTS),
    write(LEFT, GPDR, SEGMENTS | XSHUT_LEFT | XSHUT_RIGHT),
];

/// Writes one [`show`] makes.
pub const SHOW_WRITES: usize = 2;

/// Shows `codes` (see [`crate::seg`]; points are dropped), sensors kept in
/// XSHUT: the GPSR writes after [`INIT`].
pub fn show(codes: &[u8; DIGITS]) -> [Write; SHOW_WRITES] {
    [
        write(LEFT, GPSR, levels(codes[0], codes[1])),
        write(RIGHT, GPSR, levels(codes[2], codes[3])),
    ]
}

/// GPSR of one expander: low for each lit segment, XSHUTs low.
fn levels(first: u8, second: u8) -> u16 {
    SEGMENTS & !(lit(first) | lit(second) << 7)
}

fn lit(code: u8) -> u16 {
    SEGMENT_GPIO
        .iter()
        .enumerate()
        .filter(|&(s, _)| code & (1 << s) != 0)
        .fold(0, |m, (_, &gpio)| m | 1 << gpio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seg::{render, version, BLANK, POINT};
    use dagon_format::Version;

    #[test]
    fn init_turns_everything_off_and_holds_the_sensors() {
        assert_eq!(
            INIT,
            [
                (0x42, [0x12, 0xFF, 0x3F]),
                (0x42, [0x14, 0xFF, 0xBF]),
                (0x43, [0x12, 0xFF, 0x3F]),
                (0x43, [0x14, 0xFF, 0xFF]),
            ]
        );
    }

    #[test]
    fn shows_version_1_2_0() {
        // " 1.2.0" -> blank, 1, 2, 0: 1 = b c (U21 GPIO 12 13), 2 = a b d e g
        // (U19 GPIO 3 5 4 0 2), 0 = all but g (U19 GPIO 10 12 13 11 7 8).
        assert_eq!(
            show(&version(Version::new(1, 2, 0))),
            [(0x43, [0x12, 0xFF, 0x0F]), (0x42, [0x12, 0x42, 0x02])]
        );
    }

    #[test]
    fn segment_wiring() {
        // UM2047 example: digit 1 shows 0 (only g = GPIO2 high), digit 2 off.
        assert_eq!(levels(render("0").unwrap()[3], 0), 0x3F84);
        let eights = render("8888").unwrap();
        assert_eq!(show(&eights), [(0x43, [0x12, 0, 0]), (0x42, [0x12, 0, 0])]);
        assert_eq!(
            show(&BLANK),
            [(0x43, [0x12, 0xFF, 0x3F]), (0x42, [0x12, 0xFF, 0x3F])]
        );
        assert_eq!(show(&[POINT; DIGITS]), show(&BLANK));
        // Segment a alone: GPIO3 of each digit.
        let a = [0x01; DIGITS];
        let a_low = SEGMENTS & !(1 << 3 | 1 << 10);
        assert_eq!(
            show(&a),
            [write(LEFT, GPSR, a_low), write(RIGHT, GPSR, a_low)]
        );
    }
}
