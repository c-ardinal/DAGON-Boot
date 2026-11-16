//! `[boot_receive]`: the listen window of the boot stage that receives when
//! the app does not answer, and per link kind whether that kind takes part
//! in it. The range, the default, the kinds that cannot (enumeration), the
//! kinds a definition may name, when the table is required, and a
//! project's override of the window.

use std::collections::BTreeMap;

use super::tests::{mcu_err, FAKE_MCU};
use super::*;

/// [`FAKE_MCU`] with its boot core writing the app too, and `table` as
/// its `[boot_receive]` (none when empty).
fn fake(table: &str) -> String {
    let text = FAKE_MCU.replace(
        "[writers]\napp = [\"app\"]",
        "[writers]\napp = [\"app\", \"bootcore\"]",
    );
    format!("{text}\n[links]\nup = [\"i2c\", \"usb-cdc\"]\n{table}")
}

fn parsed(table: &str) -> Result<McuDef, String> {
    super::load::parse_mcu_text(&fake(table), &[]).map_err(|e| e.to_string())
}

/// The built-in MCUs: the boot core (or the vendor-stage2 MCU's recovery
/// agent) is a writer of the app, and receives on every link kind its
/// firmware has, USB outside the window, with the design's 50 ms.
#[test]
fn the_built_in_definitions_give_the_listen_window_per_link_kind() {
    use LinkKind::{I2c, Uart, UsbCdc};
    let want: BTreeMap<&str, Vec<(LinkKind, bool)>> = [
        ("rp2040", vec![(UsbCdc, false), (Uart, true), (I2c, true)]),
        ("stm32f401re", vec![(I2c, true)]),
        ("esp32s3", vec![(UsbCdc, false), (Uart, true)]),
    ]
    .into_iter()
    .collect();
    for m in Registry::builtin().mcus() {
        let r = m
            .boot_receive
            .as_ref()
            .unwrap_or_else(|| panic!("{}", m.id));
        assert_eq!(r.listen_window_ms, 50, "{}", m.id);
        let mut expected = want[m.id.as_str()].clone();
        expected.sort();
        let have: Vec<_> = r.kinds.iter().map(|(k, on)| (*k, *on)).collect();
        assert_eq!(have, expected, "{}", m.id);
        let mut all: Vec<LinkKind> = m.links.up.iter().chain(&m.links.down).copied().collect();
        all.sort();
        all.dedup();
        let listed: Vec<LinkKind> = r.kinds.keys().copied().collect();
        assert_eq!(
            listed, all,
            "{}: every link kind it has, none above another",
            m.id
        );
        let receiver = m.writers[&ImageKind::App]
            .iter()
            .any(|w| *w != ImageKind::App);
        assert!(receiver, "{}: a boot stage writes the app", m.id);
    }
}

/// The window is 0 to 200 ms; left out it is 50; 0 means none.
#[test]
fn the_listen_window_is_0_to_200_ms_and_50_by_default() {
    let kinds = "kinds = { i2c = true }";
    for ms in [0u16, 1, 50, 200] {
        let m = parsed(&format!(
            "[boot_receive]\nlisten_window_ms = {ms}\n{kinds}\n"
        ))
        .unwrap();
        assert_eq!(m.boot_receive.unwrap().listen_window_ms, ms);
    }
    let m = parsed(&format!("[boot_receive]\n{kinds}\n")).unwrap();
    assert_eq!(m.boot_receive.unwrap().listen_window_ms, 50);
    let e = parsed(&format!(
        "[boot_receive]\nlisten_window_ms = 201\n{kinds}\n"
    ))
    .unwrap_err();
    assert!(e.contains("listen_window_ms") && e.contains("200"), "{e}");
}

/// A kind that needs enumeration (USB) cannot take part in the window. The
/// window opens only on a kind of `[links]` marked as taking part: a kind
/// left out does not, and one the MCU has no link of opens none (a
/// project may take a link kind out of `[links]` alone).
#[test]
fn usb_cannot_take_part_in_the_window() {
    let m = parsed("[boot_receive]\nkinds = { i2c = true, usb-cdc = false }\n").unwrap();
    assert_eq!(m.listen_window_ms(LinkKind::I2c), 50);
    assert_eq!(m.listen_window_ms(LinkKind::UsbCdc), 0);
    let k = m.boot_receive.unwrap().kinds;
    assert_eq!((k[&LinkKind::I2c], k[&LinkKind::UsbCdc]), (true, false));
    let e = parsed("[boot_receive]\nkinds = { usb-cdc = true }\n").unwrap_err();
    assert!(e.contains("usb-cdc") && e.contains("enumeration"), "{e}");
    let m = parsed("[boot_receive]\nkinds = { uart = true }\n").unwrap();
    assert_eq!(m.listen_window_ms(LinkKind::Uart), 0, "no uart link");
    assert_eq!(m.listen_window_ms(LinkKind::I2c), 0, "left out");
    assert!(LinkKind::UsbCdc.enumerated());
    for k in [LinkKind::I2c, LinkKind::Uart, LinkKind::Can] {
        assert!(!k.enumerated(), "{k:?}");
    }
}

/// An MCU whose app another kind writes says how that kind receives; one
/// whose app only the app writes need not.
#[test]
fn a_receiving_boot_stage_needs_the_table() {
    let e = parsed("").unwrap_err();
    assert!(e.contains("boot_receive"), "{e}");
    let e = mcu_err(&format!("{FAKE_MCU}\n[boot_receive]\nkinds = {{}}\n"));
    assert!(e.contains("boot_receive"), "{e}");
    assert!(parsed("[boot_receive]\nkinds = {}\n").is_ok());
    let plain = super::load::parse_mcu_text(FAKE_MCU, &[]).unwrap();
    assert_eq!(plain.boot_receive, None);
}

fn project(name: &str, body: &str) -> Result<Registry, DefError> {
    let dir = std::env::temp_dir().join(format!("dagon-listen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(&toml, format!("format = 1\n{body}")).unwrap();
    let r = Registry::load(&[Source::Project(&toml)]);
    std::fs::remove_dir_all(dir).unwrap();
    r
}

/// A project changes the window with `[mcu_override]`, within the same
/// range; the kinds stay the definition's unless it names them.
#[test]
fn a_project_changes_the_window_with_an_override() {
    let r = project(
        "window",
        "[mcu_override.rp2040.boot_receive]\nlisten_window_ms = 120\n",
    )
    .unwrap();
    let have = r.mcu("rp2040").unwrap().boot_receive.clone().unwrap();
    let builtin = Registry::builtin().mcu("rp2040").unwrap();
    let want = builtin.boot_receive.clone().unwrap();
    assert_eq!(have.listen_window_ms, 120);
    assert_eq!(have.kinds, want.kinds);
    let e = project(
        "too-long",
        "[mcu_override.rp2040.boot_receive]\nlisten_window_ms = 300\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("listen_window_ms"), "{e}");
}
