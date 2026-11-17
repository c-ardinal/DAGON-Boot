//! The boot loader's and the app's clocks: the MCU's `[[bootcore.clock]]`
//! and `[[app.clock]]` setups, their checks, and the setup each runs on
//! for a node's board.

use super::load::parse_mcu_text;
use super::tests::{mcu_err, project_registry, FAKE_BOARD, FAKE_MCU};
use super::{ClockSource, ClockStage, Registry};

/// The RP2040's ring oscillator at its nominal frequency, for `stage`.
fn rosc(stage: &str) -> String {
    format!("\n[[{stage}.clock]]\nsource = \"internal\"\nout_hz = 6_500_000\n")
}

/// A 12 MHz crystal through the RP2040's system PLL (125 MHz) for `stage`,
/// and through its USB PLL (12 MHz x 120 = 1440 MHz, / 6 / 5 = 48 MHz)
/// with `usb`.
fn rp_xtal(stage: &str, usb: bool) -> String {
    let usb = if usb {
        ", usb_refdiv = 1, usb_fbdiv = 120, usb_postdiv1 = 6, usb_postdiv2 = 5"
    } else {
        ""
    };
    format!(
        "\n[[{stage}.clock]]\nsource = \"crystal\"\nin_hz = 12_000_000\nout_hz = 125_000_000\n\
         pll = {{ refdiv = 1, fbdiv = 125, postdiv1 = 6, postdiv2 = 2{usb} }}\n"
    )
}

/// The STM32F401's HSI through its PLL: 16 MHz / 16 x 336 = 336 MHz VCO,
/// / 4 = 84 MHz, two flash wait states.
fn hsi_pll(stage: &str) -> String {
    format!(
        "\n[[{stage}.clock]]\nsource = \"internal\"\nin_hz = 16_000_000\nout_hz = 84_000_000\n\
         pll = {{ m = 16, n = 336, p = 4, q = 7, flash_wait_states = 2 }}\n"
    )
}

/// The fake MCU (it has a boot core slot) named `id`, with `clock` (its
/// `[[bootcore.clock]]` and `[[app.clock]]` entries).
fn mcu(id: &str, clock: &str) -> String {
    FAKE_MCU
        .replace("\"fake-m0\"", &format!("\"{id}\""))
        // FAKE_MCU carries clock tables for the fixtures that do not test
        // them; these tests write their own.
        .split("\n[[bootcore.clock]]")
        .next()
        .unwrap()
        .to_string()
        + clock
}

/// An MCU whose boot loader and app both take a crystal through the RP2040
/// system and USB PLLs: the clock fields no built-in definition writes.
pub(super) fn hal_clock_text() -> String {
    mcu(
        "rp2040",
        &format!(
            "{}{}{}tolerance_ppm = 10_000\n{}",
            rosc("bootcore"),
            rp_xtal("bootcore", true),
            rosc("app"),
            rp_xtal("app", true)
        ),
    )
}

/// An internal app setup may bound its frequency for the UART check; no
/// other setup states a tolerance.
#[test]
fn only_the_apps_internal_setup_states_a_tolerance() {
    parse_mcu_text(&hal_clock_text(), &[]).unwrap();
    let tol = "tolerance_ppm = 10_000\n";
    for clock in [
        format!("{}{tol}{}", rosc("bootcore"), rosc("app")),
        format!(
            "{}{}{}{tol}",
            rosc("bootcore"),
            rosc("app"),
            rp_xtal("app", true)
        ),
    ] {
        let e = mcu_err(&mcu("rp2040", &clock));
        assert!(
            e.contains("tolerance_ppm is for the app's internal setup"),
            "{e}"
        );
    }
}

fn both_rosc() -> String {
    format!("{}{}", rosc("bootcore"), rosc("app"))
}

#[test]
fn a_boot_loader_mcu_lists_its_boot_loader_and_app_clocks() {
    let m = parse_mcu_text(&mcu("rp2040", &both_rosc()), &[]).unwrap();
    assert_eq!(m.clock_setups(ClockStage::Bootcore).unwrap().len(), 1);
    assert_eq!(m.clock_setups(ClockStage::App).unwrap().len(), 1);
    for (clock, want) in [
        (rosc("app"), "bootcore.clock needs exactly one setup"),
        (rosc("bootcore"), "app.clock needs exactly one setup"),
        (
            format!("{}{}", both_rosc(), rosc("app")),
            "app.clock needs exactly one setup",
        ),
        (
            format!("{}{}", rp_xtal("bootcore", false), rosc("app")),
            "bootcore.clock needs exactly one setup with source \"internal\"",
        ),
    ] {
        let e = mcu_err(&mcu("rp2040", &clock));
        assert!(e.contains(want), "{clock}: {e}");
    }
}

/// An MCU without a DAGON boot loader (no boot core slot) has its vendor's
/// boot loader, and its app keeps its HAL's clock: it lists neither table.
#[test]
fn an_mcu_without_a_dagon_boot_loader_lists_no_boot_loader_or_app_clock() {
    let without = |clock: &str| {
        mcu("fake-m0", clock)
            .replacen(
                "[[slot]]\nimage = \"bootcore\"\nbinding = \"a\"\nheader = \"trailer\"\nat = 0x0010_0000\nsize = 0x8000\n",
                "",
                1,
            )
            .replacen("bootcore = [\"app\"]\n", "", 1)
    };
    let m = parse_mcu_text(&without(""), &[]).unwrap();
    assert!(m.clock_setups(ClockStage::App).is_none());
    assert!(m.clock_for(ClockStage::App, None).is_none());
    let e = mcu_err(&without(&both_rosc()));
    assert!(e.contains("has no DAGON boot loader"), "{e}");
}

/// The boot loader and the app set their clocks up through their HAL,
/// which may run a PLL from the internal oscillator (the STM32F401's HSI);
/// stage0 runs its internal setup as the chip starts.
#[test]
fn a_hal_stage_may_run_a_pll_from_the_internal_oscillator() {
    let ok = format!("{}{}", hsi_pll("bootcore"), hsi_pll("app"));
    let m = parse_mcu_text(&mcu("stm32f401re", &ok), &[]).unwrap();
    let c = m.clock_for(ClockStage::App, None).unwrap();
    assert_eq!((c.in_hz, c.out_hz), (Some(16_000_000), 84_000_000));
    // The internal setup's PLL follows its rules too.
    let slow = ok.replacen("flash_wait_states = 2", "flash_wait_states = 1", 1);
    let e = mcu_err(&mcu("stm32f401re", &slow));
    assert!(
        e.contains(
            "bootcore.clock (internal 16000000 Hz): out_hz 84000000 needs 2 flash wait states"
        ),
        "{e}"
    );
    // The RP2040's PLLs take only the crystal.
    let rp = format!(
        "{}{}",
        rp_xtal("bootcore", false).replace("\"crystal\"", "\"internal\""),
        rosc("app")
    );
    let e = mcu_err(&mcu("rp2040", &rp));
    assert!(
        e.contains("takes no internal clock (it takes: crystal)"),
        "{e}"
    );
    // An internal setup without a PLL takes no input frequency.
    let e = mcu_err(&mcu(
        "rp2040",
        &format!(
            "{}{}",
            rosc("bootcore").replace("out_hz", "in_hz = 1\nout_hz"),
            rosc("app")
        ),
    ));
    assert!(
        e.contains("an internal setup without a pll takes only out_hz"),
        "{e}"
    );
    // A PLL from the internal oscillator needs that oscillator's frequency.
    let e = mcu_err(&mcu(
        "stm32f401re",
        &format!(
            "{}{}",
            hsi_pll("bootcore").replace("in_hz = 16_000_000\n", ""),
            rosc("app")
        ),
    ));
    assert!(
        e.contains("an internal setup with a pll needs in_hz"),
        "{e}"
    );
}

/// The RP2040 app's USB PLL makes the 48 MHz the USB controller runs on
/// (RP2040 datasheet section 2.15.2); its rules are the system PLL's.
#[test]
fn the_rp2040_usb_pll_makes_48_mhz_for_the_app() {
    let ok = format!(
        "{}{}{}{}",
        rosc("bootcore"),
        rosc("app"),
        rp_xtal("bootcore", false),
        rp_xtal("app", true)
    );
    let m = parse_mcu_text(&mcu("rp2040", &ok), &[]).unwrap();
    let c = m.clock_for(ClockStage::App, None).map(|c| c.out_hz);
    assert_eq!(c, Some(6_500_000));
    // 12 MHz x 100 = 1200 MHz, / 5 / 5 = 48 MHz.
    parse_mcu_text(
        &mcu(
            "rp2040",
            &ok.replace(
                "usb_fbdiv = 120, usb_postdiv1 = 6",
                "usb_fbdiv = 100, usb_postdiv1 = 5",
            ),
        ),
        &[],
    )
    .unwrap();
    for (from, to, want) in [
        (
            "usb_fbdiv = 120",
            "usb_fbdiv = 125",
            "the USB PLL gives 50000000 Hz, not 48000000",
        ),
        (
            "usb_postdiv1 = 6",
            "usb_postdiv1 = 8",
            "usb_postdiv1 8 is outside 1..=7",
        ),
        (
            "usb_fbdiv = 120",
            "usb_fbdiv = 40",
            "USB VCO 480000000 Hz is below 750000000 Hz",
        ),
        // embassy-rp 0.10 configures a PLL only with POSTDIV2 at most POSTDIV1.
        (
            "usb_postdiv1 = 6, usb_postdiv2 = 5",
            "usb_postdiv1 = 5, usb_postdiv2 = 6",
            "usb_postdiv2 6 is above usb_postdiv1 5",
        ),
    ] {
        let e = mcu_err(&mcu("rp2040", &ok.replace(from, to)));
        assert!(e.contains(want), "{to}: {e}");
    }
    // Stage0 starts no USB clock: the USB PLL's row serves only the HAL stages.
    let s0 = FAKE_MCU.replace(
        "[layout]\n",
        "[layout]\nstage0 = { at = 0x0010_A000, size = 0x4000 }\n",
    ) + "\n[stage0]\nregion_bytes = 0x4000\nhead_bytes = 0\n"
        + &rosc("stage0")
        + &rp_xtal("stage0", true);
    let e = mcu_err(&s0.replace("\"fake-m0\"", "\"rp2040\""));
    assert!(
        e.contains("stage0.clock: the RP2040 system and USB PLLs set up no stage0 clock"),
        "{e}"
    );
}

/// A project with the fake MCU (a crystal setup for both stages) and the
/// fake board with `clock = { source = <source> }` (none when empty).
fn board_registry(name: &str, source: &str) -> Registry {
    let clock = if source.is_empty() {
        String::new()
    } else {
        format!("clock = {{ source = {source} }}\n")
    };
    let setups = format!(
        "{}{}{}{}",
        rosc("bootcore"),
        rp_xtal("bootcore", false),
        rosc("app"),
        rp_xtal("app", true)
    );
    project_registry(
        name,
        &mcu("fake-m0", &setups),
        &format!("{FAKE_BOARD}{clock}"),
    )
    .unwrap()
}

/// A board without a crystal (no `[clock]`, `"internal"`, or a source no
/// setup matches) runs the boot loader and the app on the internal setup;
/// a board's crystal picks the matching setup.
#[test]
fn a_board_without_a_crystal_runs_the_boot_loader_and_app_on_the_internal_setup() {
    for (source, out_hz) in [
        ("", 6_500_000),
        ("\"internal\"", 6_500_000),
        ("{ crystal_hz = 16_000_000 }", 6_500_000),
        ("{ external_hz = 12_000_000 }", 6_500_000),
        ("{ crystal_hz = 12_000_000 }", 125_000_000),
    ] {
        let r = board_registry("hal-clock", source);
        let m = r.mcu("fake-m0").unwrap();
        let b = r.board("fake-board").unwrap();
        for stage in [ClockStage::Bootcore, ClockStage::App] {
            let c = m.clock_for(stage, Some(b)).unwrap();
            assert_eq!(c.out_hz, out_hz, "{source} {stage:?}");
            let internal = out_hz == 6_500_000;
            assert_eq!(c.matches(ClockSource::Internal), internal, "{source}");
        }
        let app = m.clock_for(ClockStage::App, Some(b)).unwrap();
        assert_eq!(app.pll.contains_key("usb_fbdiv"), out_hz != 6_500_000);
    }
}

/// Every built-in MCU with a DAGON boot loader runs it and its app on the
/// internal setup for a node without a board, and on the setup each
/// built-in board's `[clock]` picks; an MCU without one has nothing to
/// choose.
#[test]
fn every_builtin_mcu_and_board_chooses_a_boot_loader_and_app_clock() {
    let reg = Registry::builtin();
    let mut seen = Vec::new();
    for m in reg.mcus() {
        let boards = reg.boards().iter().filter(|b| b.mcu == m.id);
        for board in [None].into_iter().chain(boards.map(Some)) {
            for stage in [ClockStage::Bootcore, ClockStage::App] {
                let Some(c) = m.clock_for(stage, board) else {
                    assert!(m.clock_setups(stage).is_none(), "{} {stage:?}", m.id);
                    continue;
                };
                seen.push((
                    m.id.clone(),
                    board.map(|b| b.id.clone()),
                    stage,
                    c.source.name(),
                    c.out_hz,
                ));
            }
        }
    }
    let ws = Some("waveshare-rp2040-touch-lcd-1.28");
    let nucleo = Some("nucleo-f401re");
    let want = [
        ("rp2040", None, ClockStage::Bootcore, "internal", 6_500_000),
        ("rp2040", None, ClockStage::App, "internal", 6_500_000),
        ("rp2040", ws, ClockStage::Bootcore, "crystal", 125_000_000),
        ("rp2040", ws, ClockStage::App, "crystal", 125_000_000),
        (
            "stm32f401re",
            None,
            ClockStage::Bootcore,
            "internal",
            84_000_000,
        ),
        ("stm32f401re", None, ClockStage::App, "internal", 84_000_000),
        (
            "stm32f401re",
            nucleo,
            ClockStage::Bootcore,
            "internal",
            84_000_000,
        ),
        (
            "stm32f401re",
            nucleo,
            ClockStage::App,
            "internal",
            84_000_000,
        ),
    ];
    for w in want {
        let w = (w.0.to_string(), w.1.map(str::to_string), w.2, w.3, w.4);
        assert!(seen.contains(&w), "{w:?} in {seen:?}");
    }
    assert_eq!(seen.len(), want.len(), "{seen:?}");
    // The RP2040 app on a crystal has the USB clock; the boot loader does not.
    let rp = reg.mcu("rp2040").unwrap();
    let b = reg.board("waveshare-rp2040-touch-lcd-1.28").unwrap();
    assert!(rp
        .clock_for(ClockStage::App, Some(b))
        .unwrap()
        .pll
        .contains_key("usb_fbdiv"));
    assert!(!rp
        .clock_for(ClockStage::Bootcore, Some(b))
        .unwrap()
        .pll
        .contains_key("usb_fbdiv"));
}

/// A stage0 still takes only its own table: its setups never change with
/// the boot loader's or the app's.
#[test]
fn stage0_keeps_its_own_setups() {
    let reg = Registry::builtin();
    for m in reg.mcus().iter().filter(|m| m.stage0.is_some()) {
        assert_eq!(
            m.clock_for(ClockStage::Stage0, None),
            m.stage0_clock(None),
            "{}",
            m.id
        );
    }
}
