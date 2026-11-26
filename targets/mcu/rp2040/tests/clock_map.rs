//! What the definition says of the chip's clocks fits the RP2040:
//!
//! - every PLL value of a stage's `clock` fits its register field, as
//!   rp-pac ships it (generated from Raspberry Pi's SVD: PLL CS REFDIV,
//!   FBDIV_INT, PRIM POSTDIV1 and POSTDIV2), and lies in the range the
//!   RP2040 datasheet gives it (section 2.18, PLL);
//! - the PLL's reference and VCO run in the datasheet's ranges, and its
//!   output is the clock's `out_hz` (the USB PLL's 48 MHz);
//! - a clock without a PLL runs in its source's range (the ring
//!   oscillator, section 2.17; the crystal oscillator, section 2.16);
//! - the PIO divider of the UART link (`[link_uart]`) fits its register
//!   field at every bit rate on every app clock.

use rp_pac::pio::regs::SmClkdiv;
use rp_pac::pll::regs::{Cs, FbdivInt, Prim};

fn definition() -> toml::Table {
    include_str!("../dagon-mcu.toml").parse().unwrap()
}

/// RP2040 datasheet section 2.18.2: the feedback divider, each post
/// divider and the reference divider.
const FBDIV: std::ops::RangeInclusive<u64> = 16..=320;
const POSTDIV: std::ops::RangeInclusive<u64> = 1..=7;
const REFDIV: std::ops::RangeInclusive<u64> = 1..=63;
/// Section 2.18.2: the VCO, and the reference it locks to (at least 5 MHz,
/// at most the VCO over the lowest feedback divider).
const VCO_HZ: std::ops::RangeInclusive<u64> = 750_000_000..=1_600_000_000;
const MIN_REF_HZ: u64 = 5_000_000;
/// Section 2.17.1: the ring oscillator, over process, voltage and
/// temperature.
const ROSC_HZ: std::ops::RangeInclusive<u64> = 1_800_000..=12_000_000;
/// Section 2.16: the crystal oscillator.
const XOSC_HZ: std::ops::RangeInclusive<u64> = 1_000_000..=15_000_000;
/// Section 4.1: the USB controller's clock.
const USB_HZ: u64 = 48_000_000;

/// Whether the PLL's register fields hold the four values.
fn held(refdiv: u64, fbdiv: u64, postdiv1: u64, postdiv2: u64) -> bool {
    let mut cs = Cs(0);
    cs.set_refdiv(refdiv as u8);
    let mut fb = FbdivInt(0);
    fb.set_fbdiv_int(fbdiv as u16);
    let mut prim = Prim(0);
    prim.set_postdiv1(postdiv1 as u8);
    prim.set_postdiv2(postdiv2 as u8);
    u64::from(cs.refdiv()) == refdiv
        && u64::from(fb.fbdiv_int()) == fbdiv
        && u64::from(prim.postdiv1()) == postdiv1
        && u64::from(prim.postdiv2()) == postdiv2
}

/// What is wrong with one PLL of `pll` (its keys under `prefix`) run from
/// `in_hz` for `out_hz`.
fn pll_faults(at: &str, pll: &toml::Value, prefix: &str, in_hz: u64, out_hz: u64) -> Vec<String> {
    let get = |k: &str| pll[format!("{prefix}{k}").as_str()].as_integer().unwrap() as u64;
    let (refdiv, fbdiv, pd1, pd2) = (
        get("refdiv"),
        get("fbdiv"),
        get("postdiv1"),
        get("postdiv2"),
    );
    let mut out = Vec::new();
    if !held(refdiv, fbdiv, pd1, pd2) {
        out.push(format!("{at}: a {prefix}value does not fit its field"));
    }
    if !REFDIV.contains(&refdiv) || !FBDIV.contains(&fbdiv) {
        out.push(format!(
            "{at}: {prefix}refdiv {refdiv} or {prefix}fbdiv {fbdiv} out of range"
        ));
    }
    if !POSTDIV.contains(&pd1) || !POSTDIV.contains(&pd2) {
        out.push(format!("{at}: {prefix}postdiv {pd1}, {pd2} out of range"));
    }
    if out.is_empty() {
        let reference = in_hz / refdiv;
        let vco = reference * fbdiv;
        if in_hz % refdiv != 0 || reference < MIN_REF_HZ || !VCO_HZ.contains(&vco) {
            out.push(format!(
                "{at}: {prefix}reference {reference} Hz, VCO {vco} Hz"
            ));
        }
        if vco % (pd1 * pd2) != 0 || vco / (pd1 * pd2) != out_hz {
            out.push(format!("{at}: {prefix}output is not {out_hz} Hz"));
        }
    }
    out
}

/// What is wrong with clock `c` of stage `at`.
fn clock_faults(at: &str, c: &toml::Value) -> Vec<String> {
    let hz = |k: &str| c.get(k).and_then(|v| v.as_integer()).map(|v| v as u64);
    let out_hz = hz("out_hz").unwrap();
    let source = c["source"].as_str().unwrap();
    let mut out = Vec::new();
    match (source, c.get("pll")) {
        ("internal", None) => {
            if !ROSC_HZ.contains(&out_hz) {
                out.push(format!("{at}: ring oscillator at {out_hz} Hz"));
            }
        }
        ("crystal", pll) => {
            let in_hz = hz("in_hz").unwrap_or(out_hz);
            if !XOSC_HZ.contains(&in_hz) {
                out.push(format!("{at}: crystal at {in_hz} Hz"));
            }
            match pll {
                None if in_hz != out_hz => out.push(format!("{at}: no PLL for {out_hz} Hz")),
                None => {}
                Some(pll) => {
                    out.extend(pll_faults(at, pll, "", in_hz, out_hz));
                    if pll.get("usb_fbdiv").is_some() {
                        out.extend(pll_faults(at, pll, "usb_", in_hz, USB_HZ));
                    }
                }
            }
        }
        _ => out.push(format!(
            "{at}: source {source} with this setup is not checked"
        )),
    }
    out
}

const STAGES: [&str; 3] = ["stage0", "bootcore", "app"];

#[test]
fn every_clock_runs_within_what_the_chip_allows() {
    let d = definition();
    let mut out = Vec::new();
    let mut plls = 0;
    for stage in STAGES {
        for c in d[stage]["clock"].as_array().unwrap() {
            plls += usize::from(c.get("pll").is_some());
            out.extend(clock_faults(stage, c));
        }
    }
    assert!(plls > 0);
    assert!(out.is_empty(), "{out:#?}");
}

/// The check refuses what the chip cannot run.
#[test]
fn a_clock_outside_the_chips_ranges_is_refused() {
    let refused = |text: &str| {
        let c: toml::Table = text.parse().unwrap();
        !clock_faults("x", &toml::Value::Table(c)).is_empty()
    };
    let crystal = "source = \"crystal\"\nin_hz = 12_000_000\nout_hz = 125_000_000\n";
    assert!(!refused(&format!(
        "{crystal}pll = {{ refdiv = 1, fbdiv = 125, postdiv1 = 6, postdiv2 = 2 }}"
    )));
    // A feedback divider past its range, and one the field cannot hold.
    for fbdiv in [321, 4096] {
        assert!(refused(&format!(
            "{crystal}pll = {{ refdiv = 1, fbdiv = {fbdiv}, postdiv1 = 6, postdiv2 = 2 }}"
        )));
    }
    // A post divider past its field (3 bits).
    assert!(refused(&format!(
        "{crystal}pll = {{ refdiv = 1, fbdiv = 125, postdiv1 = 8, postdiv2 = 2 }}"
    )));
    // A VCO under its range, and a reference under 5 MHz.
    assert!(refused(&format!(
        "{crystal}pll = {{ refdiv = 1, fbdiv = 50, postdiv1 = 6, postdiv2 = 2 }}"
    )));
    assert!(refused(&format!(
        "{crystal}pll = {{ refdiv = 3, fbdiv = 320, postdiv1 = 6, postdiv2 = 2 }}"
    )));
    // An output the dividers do not give.
    assert!(refused(&format!(
        "{crystal}pll = {{ refdiv = 1, fbdiv = 125, postdiv1 = 5, postdiv2 = 2 }}"
    )));
    // A USB PLL that does not give 48 MHz.
    assert!(refused(&format!(
        "{crystal}pll = {{ refdiv = 1, fbdiv = 125, postdiv1 = 6, postdiv2 = 2, usb_refdiv = 1, usb_fbdiv = 120, usb_postdiv1 = 6, usb_postdiv2 = 4 }}"
    )));
    assert!(refused("source = \"internal\"\nout_hz = 13_000_000\n"));
    assert!(refused(
        "source = \"crystal\"\nin_hz = 16_000_000\nout_hz = 16_000_000\n"
    ));
}

#[test]
fn the_uart_links_pio_divider_fits_its_field_on_every_app_clock() {
    let d = definition();
    let uart = &d["link_uart"];
    assert_eq!(uart["clock"].as_str(), Some("app"));
    // The PIO program's divider is whole: its fraction bits are left zero.
    assert_eq!(uart["frac_bits"].as_integer(), Some(0));
    let per_bit = uart["clocks_per_bit"].as_integer().unwrap() as u64;
    let bauds = uart["bauds"].as_array().unwrap();
    assert!(!bauds.is_empty());
    for c in d["app"]["clock"].as_array().unwrap() {
        let clk = c["out_hz"].as_integer().unwrap() as u64;
        for baud in bauds {
            let baud = baud.as_integer().unwrap() as u64;
            let div = clk / (per_bit * baud);
            let mut reg = SmClkdiv(0);
            reg.set_int(div as u16);
            assert!(
                div >= 1 && u64::from(reg.int()) == div,
                "{clk} Hz at {baud} baud: divider {div}"
            );
        }
    }
}
