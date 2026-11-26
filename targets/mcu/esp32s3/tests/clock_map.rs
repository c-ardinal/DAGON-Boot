//! What the definition says of the UART link's clock (`[link_uart]`) fits
//! the ESP32-S3:
//!
//! - its divider has the fraction bits of the UART's CLKDIV register, as
//!   Espressif's SVD (the esp32s3 PAC: field FRAG) and chip metadata
//!   (esp-metadata-generated: the baud rate generator's ranges) give them;
//! - at every bit rate of the link, the whole part of the divider fits the
//!   register's CLKDIV field;
//! - its clock is the crystal's 40 MHz, the only crystal the chip runs
//!   from (ESP32-S3 datasheet, section 2.4 "Clock").

use esp32s3::uart0::clkdiv::{CLKDIV_SPEC, CLKDIV_W, FRAG_W};

fn definition() -> toml::Table {
    include_str!("../dagon-mcu.toml").parse().unwrap()
}

/// ESP32-S3 datasheet: the main crystal.
const XTAL_HZ: i64 = 40_000_000;

/// Espressif's ranges of the baud rate generator: (whole part, fraction).
fn generator() -> ((u64, u64), (u64, u64)) {
    (
        esp_metadata_generated::property!("clock_tree.uart.baud_rate_generator.integral"),
        esp_metadata_generated::property!("clock_tree.uart.baud_rate_generator.fractional"),
    )
}

/// The whole part of the divider `[link_uart]` of `uart` runs `baud` at,
/// as the definition tells it: its clock over `clocks_per_bit` times
/// `baud`, in `frac_bits` fraction bits, rounded down.
fn whole_part(uart: &toml::Value, baud: u64) -> u64 {
    let get = |k: &str| uart[k].as_integer().unwrap() as u64;
    let scaled = (get("clock_hz") << get("frac_bits")) / (get("clocks_per_bit") * baud);
    scaled >> get("frac_bits")
}

#[test]
fn the_svd_and_the_metadata_give_the_divider_the_same_fields() {
    let ((whole_min, whole_max), (frac_min, frac_max)) = generator();
    assert_eq!((whole_min, frac_min), (0, 0));
    assert_eq!(whole_max, (1 << CLKDIV_W::<CLKDIV_SPEC>::WIDTH) - 1);
    assert_eq!(frac_max, (1 << FRAG_W::<CLKDIV_SPEC>::WIDTH) - 1);
}

#[test]
fn the_uart_links_divider_is_the_chips() {
    let d = definition();
    let uart = &d["link_uart"];
    assert_eq!(uart["clock_hz"].as_integer(), Some(XTAL_HZ));
    assert_eq!(
        uart["frac_bits"].as_integer(),
        Some(i64::from(FRAG_W::<CLKDIV_SPEC>::WIDTH))
    );
    let (_, whole_max) = generator().0;
    let bauds = uart["bauds"].as_array().unwrap();
    assert!(!bauds.is_empty());
    for baud in bauds {
        let baud = baud.as_integer().unwrap() as u64;
        let whole = whole_part(uart, baud);
        assert!(
            (1..=whole_max).contains(&whole),
            "{baud} baud: divider {whole}"
        );
    }
}

/// The check refuses a bit rate the divider cannot hold.
#[test]
fn a_bit_rate_too_low_for_the_field_is_past_its_range() {
    let d = definition();
    let (_, whole_max) = generator().0;
    // 40 MHz / 9600 = 4166, past the 12-bit whole part.
    assert!(whole_part(&d["link_uart"], 9_600) > whole_max);
    assert!(whole_part(&d["link_uart"], 115_200) <= whole_max);
}
