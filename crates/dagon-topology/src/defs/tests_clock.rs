//! Stage0's clock: the MCU's `[[stage0.clock]]` setups and their checks,
//! the board's `[clock]` source, and the setup stage0 runs on.

use super::load::parse_mcu_text;
use super::tests::{mcu_err, project_registry, FAKE_BOARD, FAKE_MCU};
use super::{ClockSource, Registry};

/// The fake MCU named `id`, with a stage0 whose `[[stage0.clock]]` entries
/// are `clock`.
fn with_clock(id: &str, clock: &str) -> String {
    FAKE_MCU
        .replace("\"fake-m0\"", &format!("\"{id}\""))
        .replace(
            "[layout]\n",
            "[layout]\nstage0 = { at = 0x0010_A000, size = 0x4000 }\n",
        )
        + "\n[stage0]\nregion_bytes = 0x4000\nhead_bytes = 0\n"
        + clock
}

/// The RP2040's ring oscillator at its nominal reset frequency.
const ROSC: &str = "\n[[stage0.clock]]\nsource = \"internal\"\nout_hz = 6_500_000\n";

/// A 12 MHz crystal through the RP2040's system PLL: 12 MHz x 125 = 1500 MHz
/// VCO, / 6 / 2 = 125 MHz.
const RP_XTAL: &str = "\n[[stage0.clock]]\nsource = \"crystal\"\nin_hz = 12_000_000\n\
                       out_hz = 125_000_000\n\
                       pll = { refdiv = 1, fbdiv = 125, postdiv1 = 6, postdiv2 = 2 }\n";

/// The STM32F401's HSI.
const HSI: &str = "\n[[stage0.clock]]\nsource = \"internal\"\nout_hz = 16_000_000\n";

/// An 8 MHz STM32F401 HSE through its PLL: 8 MHz / 8 x 336 = 336 MHz VCO,
/// / 4 = 84 MHz with two flash wait states.
const ST_HSE: &str = "\n[[stage0.clock]]\nsource = \"external\"\nin_hz = 8_000_000\n\
                      out_hz = 84_000_000\n\
                      pll = { m = 8, n = 336, p = 4, q = 7, flash_wait_states = 2 }\n";

fn rp(clock: &str) -> String {
    with_clock("rp2040", clock)
}

fn st(clock: &str) -> String {
    with_clock("stm32f401re", clock)
}

/// An MCU with an STM32F401 PLL setup and a board on an external clock:
/// the clock fields no built-in definition writes.
pub(super) fn clock_texts() -> (String, String) {
    (
        st(&format!("{HSI}{ST_HSE}")),
        format!(
            "id = \"x\"
{FAKE_BOARD}[clock]
source = {{ external_hz = 8_000_000 }}
"
        ),
    )
}

#[test]
fn a_stage0_lists_exactly_one_internal_setup() {
    let m = parse_mcu_text(&rp(&format!("{ROSC}{RP_XTAL}")), &[]).unwrap();
    assert_eq!(m.stage0.unwrap().clock.len(), 2);
    for clock in ["", RP_XTAL, &format!("{ROSC}{ROSC}")] {
        let e = mcu_err(&rp(clock));
        assert!(
            e.contains("stage0.clock needs exactly one setup with source \"internal\""),
            "{clock}: {e}"
        );
    }
}

#[test]
fn the_internal_setup_takes_no_input_and_no_pll() {
    for extra in ["in_hz = 6_500_000\n", "pll = { refdiv = 1 }\n"] {
        let e = mcu_err(&rp(&format!("{ROSC}{extra}")));
        assert!(
            e.contains("the internal setup takes only out_hz"),
            "{extra}: {e}"
        );
    }
}

#[test]
fn two_matching_setups_with_the_same_out_hz_are_refused() {
    // 12 MHz x 125 = 1500 MHz, / 4 / 3 = 125 MHz again.
    let again = RP_XTAL.replace("postdiv1 = 6, postdiv2 = 2", "postdiv1 = 4, postdiv2 = 3");
    let e = mcu_err(&rp(&format!("{ROSC}{RP_XTAL}{again}")));
    assert!(
        e.contains("two crystal setups for 12000000 Hz give 125000000 Hz"),
        "{e}"
    );
    // The same output from another input is a different match.
    let ten = RP_XTAL
        .replace("in_hz = 12_000_000", "in_hz = 10_000_000")
        .replace("fbdiv = 125", "fbdiv = 150");
    parse_mcu_text(&rp(&format!("{ROSC}{RP_XTAL}{ten}")), &[]).unwrap();
}

#[test]
fn frequencies_are_checked() {
    let bad = [
        (ROSC.replace("6_500_000", "0"), "out_hz 0"),
        (
            format!("{ROSC}{}", RP_XTAL.replace("in_hz = 12_000_000\n", "")),
            "a crystal setup needs in_hz",
        ),
        (
            format!("{ROSC}{}", RP_XTAL.replace("12_000_000", "0")),
            "in_hz 0",
        ),
    ];
    for (text, want) in bad {
        let e = mcu_err(&rp(&text));
        assert!(e.contains(want), "{want}: {e}");
    }
}

#[test]
fn the_rp2040_pll_follows_the_rp2040_rules() {
    let bad = [
        // 12 MHz x 125 / 6 / 2 is 125 MHz, not 124 MHz.
        (
            "out_hz = 125_000_000",
            "out_hz = 124_000_000",
            "gives 125000000 Hz",
        ),
        (
            "fbdiv = 125",
            "fbdiv = 321",
            "fbdiv 321 is outside 16..=320",
        ),
        (
            "postdiv1 = 6",
            "postdiv1 = 8",
            "postdiv1 8 is outside 1..=7",
        ),
        ("refdiv = 1", "refdiv = 3", "below 5000000 Hz"),
        // 12 MHz x 50 = 600 MHz.
        (
            "fbdiv = 125, postdiv1 = 6",
            "fbdiv = 50, postdiv1 = 6",
            "VCO 600000000 Hz",
        ),
        // 1500 MHz / 5 / 2 = 150 MHz.
        (
            "out_hz = 125_000_000",
            "out_hz = 150_000_000",
            "above 133000000 Hz",
        ),
        (
            "in_hz = 12_000_000",
            "in_hz = 16_000_000",
            "crystal 16000000 Hz",
        ),
    ];
    for (from, to, want) in bad {
        let mut setup = RP_XTAL.replace(from, to);
        if want.starts_with("above") {
            setup = setup.replace("postdiv1 = 6", "postdiv1 = 5");
        }
        let e = mcu_err(&rp(&format!("{ROSC}{setup}")));
        assert!(e.contains(want), "{to}: {e}");
    }
    let e = mcu_err(&rp(&format!(
        "{ROSC}{}",
        RP_XTAL.replace("\"crystal\"", "\"external\"")
    )));
    assert!(
        e.contains("on the RP2040 system PLL takes no external clock (it takes: crystal)"),
        "{e}"
    );
}

/// A divided frequency is compared as the exact fraction, so a fraction of a
/// hertz past a bound is refused.
#[test]
fn a_fraction_of_a_hertz_past_a_bound_is_refused() {
    // 6,300,001 / 3 = 2,100,000.33 Hz into the PLL; 31,500,005 Hz out is exact.
    let input = ST_HSE
        .replace("in_hz = 8_000_000", "in_hz = 6_300_001")
        .replace("out_hz = 84_000_000", "out_hz = 31_500_005")
        .replace("m = 8, n = 336", "m = 3, n = 60")
        .replace("flash_wait_states = 2", "flash_wait_states = 1");
    let e = mcu_err(&st(&format!("{HSI}{input}")));
    assert!(
        e.contains("PLL input 6300001/3 Hz is above 2100000 Hz"),
        "{e}"
    );
    // 7,160,221 / 6 x 362 = 432,000,000.17 Hz in the VCO.
    let vco = ST_HSE
        .replace("in_hz = 8_000_000", "in_hz = 7_160_221")
        .replace("m = 8, n = 336", "m = 6, n = 362");
    let e = mcu_err(&st(&format!("{HSI}{vco}")));
    assert!(
        e.contains("VCO 2592000002/6 Hz is above 432000000 Hz"),
        "{e}"
    );
    // 14,999,999 / 3 = 4,999,999.67 Hz into the RP2040 PLL.
    let reference = RP_XTAL
        .replace("in_hz = 12_000_000", "in_hz = 14_999_999")
        .replace("refdiv = 1", "refdiv = 3");
    let e = mcu_err(&rp(&format!("{ROSC}{reference}")));
    assert!(
        e.contains("reference (in_hz / refdiv) 14999999/3 Hz is below 5000000 Hz"),
        "{e}"
    );
}

#[test]
fn the_stm32f401_pll_follows_the_stm32f401_rules() {
    let ok = format!("{HSI}{ST_HSE}");
    parse_mcu_text(&st(&ok), &[]).unwrap();
    parse_mcu_text(&st(&ok.replace("\"external\"", "\"crystal\"")), &[]).unwrap();
    let bad = [
        ("p = 4", "p = 3", "p 3 is not one of 2, 4, 6, 8"),
        ("n = 336", "n = 433", "n 433 is outside 50..=432"),
        ("q = 7", "q = 16", "q 16 is outside 2..=15"),
        ("m = 8", "m = 64", "m 64 is outside 2..=63"),
        // 8 MHz / 2 = 4 MHz into the PLL.
        ("m = 8, n = 336", "m = 2, n = 84", "PLL input 4000000 Hz"),
        (
            "flash_wait_states = 2",
            "flash_wait_states = 1",
            "needs 2 flash wait states",
        ),
        (
            "in_hz = 8_000_000",
            "in_hz = 60_000_000",
            "external 60000000 Hz",
        ),
    ];
    for (from, to, want) in bad {
        let e = mcu_err(&st(&format!("{HSI}{}", ST_HSE.replace(from, to))));
        assert!(e.contains(want), "{to}: {e}");
    }
    let e = mcu_err(&st(&format!(
        "{HSI}{}",
        ST_HSE.replace("q = 7, ", "").replace("m = 8", "refdiv = 8")
    )));
    assert!(
        e.contains("no rules for a pll with flash_wait_states, n, p, refdiv"),
        "{e}"
    );
}

/// The PLL's fields name its rules: a copy of the RP2040 definition under
/// another id keeps them; fields no rules name are refused.
#[test]
fn a_pll_is_checked_by_the_rules_its_fields_name() {
    parse_mcu_text(&with_clock("my-rp2040", &format!("{ROSC}{RP_XTAL}")), &[]).unwrap();
    let bad = RP_XTAL
        .replace("refdiv = 1, ", "")
        .replace("fbdiv = 125", "mul = 125");
    let e = mcu_err(&with_clock("fake-m0", &format!("{ROSC}{bad}")));
    assert!(
        e.contains(
            "no rules for a pll with mul, postdiv1, postdiv2; it knows the RP2040 system PLL"
        ),
        "{e}"
    );
    let e = mcu_err(&with_clock(
        "fake-m0",
        &format!(
            "{ROSC}{}",
            RP_XTAL.replace(", postdiv2 = 2 }", ", postdiv2 = 2, x = 1 }")
        ),
    ));
    assert!(e.contains("no rules for a pll with fbdiv"), "{e}");
}

/// A project with the fake MCU (its internal setup only) and the fake
/// board with `clock = { source = <source> }` (none when empty).
fn board_registry(name: &str, source: &str) -> Result<Registry, super::DefError> {
    let clock = if source.is_empty() {
        String::new()
    } else {
        format!(
            "clock = {{ source = {source} }}
"
        )
    };
    project_registry(
        name,
        &with_clock("fake-m0", ROSC),
        &format!("{FAKE_BOARD}{clock}"),
    )
}

const FORMS: &str = "clock.source is \"internal\", { crystal_hz = N } or { external_hz = N }";

#[test]
fn a_board_states_its_clock_source() {
    for (source, want) in [
        ("", ClockSource::Internal),
        ("\"internal\"", ClockSource::Internal),
        (
            "{ crystal_hz = 12_000_000 }",
            ClockSource::Crystal(12_000_000),
        ),
        (
            "{ external_hz = 8_000_000 }",
            ClockSource::External(8_000_000),
        ),
    ] {
        let r = board_registry("clock-ok", source).unwrap();
        assert_eq!(r.board("fake-board").unwrap().clock_source(), want);
    }
    for (source, want) in [
        ("\"pll\"", FORMS),
        ("{ crystal = 12_000_000 }", FORMS),
        (
            "{ crystal_hz = 12_000_000, external_hz = 12_000_000 }",
            FORMS,
        ),
        ("{ crystal_hz = 0 }", "clock.source crystal_hz 0"),
        ("{ external_hz = 0 }", "clock.source external_hz 0"),
    ] {
        let e = board_registry("clock-bad", source).unwrap_err().to_string();
        assert!(e.contains(want), "{source}: {e}");
    }
}

#[test]
fn stage0_runs_from_the_fastest_matching_setup_else_the_internal_one() {
    // 12 MHz x 133 = 1596 MHz VCO, / 6 / 2 = 133 MHz.
    let faster = RP_XTAL
        .replace("out_hz = 125_000_000", "out_hz = 133_000_000")
        .replace("fbdiv = 125", "fbdiv = 133");
    let m = parse_mcu_text(&rp(&format!("{ROSC}{RP_XTAL}{faster}")), &[]).unwrap();
    let s = m.stage0.as_ref().unwrap();
    for (source, out_hz) in [
        (ClockSource::Internal, 6_500_000),
        (ClockSource::Crystal(12_000_000), 133_000_000),
        // No setup for this crystal, nor for an external clock.
        (ClockSource::Crystal(8_000_000), 6_500_000),
        (ClockSource::External(12_000_000), 6_500_000),
    ] {
        assert_eq!(s.clock_for(source).out_hz, out_hz, "{source:?}");
    }
}

/// Every built-in MCU with a stage0 runs it on its internal setup for a
/// node without a board, and every built-in board's node on the setup its
/// `[clock]` picks; an MCU without a stage0 takes a board `[clock]` and
/// has nothing to choose.
#[test]
fn every_builtin_mcu_and_board_chooses_a_stage0_clock() {
    let reg = Registry::builtin();
    let mut seen = Vec::new();
    for m in reg.mcus() {
        let Some(s) = &m.stage0 else {
            assert!(m.stage0_clock(None).is_none(), "{}", m.id);
            continue;
        };
        let internal = m.stage0_clock(None).unwrap();
        assert_eq!(internal, s.clock_for(ClockSource::Internal));
        seen.push((m.id.clone(), None, internal.out_hz));
        for b in reg.boards().iter().filter(|b| b.mcu == m.id) {
            let c = m.stage0_clock(Some(b)).unwrap();
            seen.push((m.id.clone(), Some(b.id.clone()), c.out_hz));
        }
    }
    let want = [
        ("rp2040", None, 6_500_000),
        (
            "rp2040",
            Some("waveshare-rp2040-touch-lcd-1.28"),
            125_000_000,
        ),
        ("stm32f401re", None, 16_000_000),
        ("stm32f401re", Some("nucleo-f401re"), 16_000_000),
    ];
    for w in want {
        let w = (w.0.to_string(), w.1.map(str::to_string), w.2);
        assert!(seen.contains(&w), "{w:?} in {seen:?}");
    }
    assert_eq!(seen.len(), want.len(), "{seen:?}");
}

/// The requirement table names, for each stage, exactly the PLL fields of
/// the rules that serve it.
#[test]
fn the_requirements_name_every_pll_field_of_the_rules() {
    let (mcu, _) = super::requirements::declared_fields();
    for stage in super::ClockStage::ALL {
        let prefix = format!("{}.clock[].pll.", stage.name());
        let mut declared: Vec<&str> = mcu.iter().filter_map(|f| f.strip_prefix(&prefix)).collect();
        let mut rules: Vec<&str> = super::clock::PLL_RULES
            .iter()
            .filter(|r| r.stages.contains(&stage))
            .flat_map(|r| r.fields.iter().copied())
            .collect();
        declared.sort_unstable();
        rules.sort_unstable();
        rules.dedup();
        assert_eq!(declared, rules, "{stage:?}");
    }
}
