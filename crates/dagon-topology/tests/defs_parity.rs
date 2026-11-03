//! The builtin definitions carry what signing, provisioning, the host's ROM
//! path and the CLI know today. Public values are compared directly;
//! private CLI/web constants are checked against their source text, so a
//! change on either side fails here until both agree.

use dagon_sign::{image, provision};
use dagon_topology::defs::{Expect, Registry, Writer};

fn reg() -> &'static Registry {
    Registry::builtin()
}

const CLI_FLASH: &str = include_str!("../../dagon-cli/src/flash.rs");
const CLI_COMMANDS: &str = include_str!("../../dagon-cli/src/commands.rs");
const CLI_ESP: &str = include_str!("../../dagon-cli/src/esp.rs");
const CLI_DOCTOR: &str = include_str!("../../dagon-cli/src/doctor.rs");
const WEB_COPY: &str = include_str!("../../../web/src/app/copy.ts");

fn has(src: &str, needle: &str) {
    assert!(src.contains(needle), "source no longer has: {needle}");
}

#[test]
fn every_slot_matches_dagon_sign() {
    let mut seen = 0;
    for mcu in ["rp2040", "stm32f401re", "esp32s3", "fake", ""] {
        for kind in 0..=u8::MAX {
            for slot in 0..=u8::MAX {
                let want = image::slot(mcu, kind, slot);
                let got = reg().slot(mcu, kind, slot).map(|s| image::Slot {
                    header_addr: s.header_addr,
                    load_addr: s.load_addr,
                    max_payload: s.max_payload,
                    placement: s.placement.wire(),
                    slot_binding: s.binding.wire(),
                });
                assert_eq!(got, want, "{mcu} kind {kind:#x} slot {slot:#x}");
                seen += usize::from(want.is_some());
            }
        }
    }
    assert_eq!(seen, 9);
}

#[test]
fn provision_layouts_match() {
    let rp = reg().mcu("rp2040").unwrap();
    let base = rp.default_flash_map().base;
    let Writer::Uf2Drive { family, sector, .. } = &rp.writer else {
        panic!("rp2040 writes a UF2 drive")
    };
    // dagon-sign builds the first-install files from these definitions
    // (its first_write_golden test pins the bytes); dagon-cli reads them
    // too (the_cli_holds_no_mcu_knowledge).
    assert_eq!(*family, provision::UF2_FAMILY_RP2040);
    assert_eq!(*sector, 0x1000);
    let stage0 = rp.layout.stage0.unwrap();
    assert_eq!(
        (stage0.at, stage0.size),
        (base, provision::RP2040_STAGE0_MAX)
    );
    let f4 = reg().mcu("stm32f401re").unwrap();
    assert_eq!(f4.default_flash_map().base, provision::F401_FLASH_BASE);
    assert!(matches!(f4.writer, Writer::ProbeRs { .. }));
}

#[test]
fn host_rom_path_matches() {
    use dagon_host::rom::{rom_maps, RomSlot};
    // The values dagon-host's rom.rs held before it read the definitions.
    let maps = rom_maps(reg());
    assert_eq!(maps.len(), 1, "only the F401 has a ROM I2C loader");
    let m = &maps[0];
    assert_eq!(m.wire_id, dagon_format::header::mcu_id::STM32F401XE);
    assert_eq!(
        m.bootcore,
        RomSlot {
            header: 0x0800_7E00,
            load: 0x0800_0000,
            max_payload: 0x7E00
        }
    );
    assert_eq!(
        m.app_a,
        RomSlot {
            header: 0x0802_0000,
            load: 0x0802_0200,
            max_payload: 0x2_0000 - 512
        }
    );
    assert_eq!(m.log_a, 0x0800_8000);
    assert_eq!(m.erase, [0, 1, 2, 3, 5]);
    assert_eq!(m.addr, dagon_romlink::ROM_ADDR);
}

/// The part of a source file before its tests.
fn code(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

/// dagon-cli's first write (flash, commands, esp) reads the definitions:
/// no MCU or board id, writer parameter or USB ID is left in its code.
#[test]
fn the_cli_holds_no_mcu_knowledge() {
    let cli = [code(CLI_FLASH), code(CLI_COMMANDS), code(CLI_ESP)];
    let mut words: Vec<String> = Vec::new();
    for m in reg().mcus() {
        words.push(format!("\"{}\"", m.id));
        match &m.writer {
            Writer::Uf2Drive {
                info_file,
                board_id,
                ..
            } => words.extend([info_file.clone(), board_id.clone()]),
            Writer::ProbeRs { chip } => words.push(chip.clone()),
            Writer::Espflash {
                chip,
                flash_size,
                usb_hint,
                ..
            } => {
                words.extend([format!("\"{chip}\""), format!("\"{flash_size}\"")]);
                if let Some(u) = usb_hint {
                    words.push(format!("0x{:04X}", u.vid));
                }
            }
        }
    }
    for b in reg().boards() {
        words.push(format!("\"{}\"", b.id));
    }
    for w in &words {
        for src in cli {
            assert!(!src.contains(w.as_str()), "dagon-cli code still has {w}");
        }
    }
}

/// The parts the CLI built before it read the definitions, now from the
/// source tree (loading checks each part folder exists in its package).
#[test]
fn first_write_parts_load_from_the_tree() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = Registry::load(&[dagon_topology::defs::Source::Tree(&root)]).unwrap();
    let names = |parts: &[dagon_topology::defs::Part]| {
        parts.iter().map(|p| p.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        names(&tree.mcu("rp2040").unwrap().parts),
        ["stage0", "bootcore"]
    );
    assert_eq!(names(&tree.mcu("stm32f401re").unwrap().parts), ["bootcore"]);
    assert!(tree.mcu("esp32s3").unwrap().parts.is_empty());
    let watch = &tree.board("m5stopwatch").unwrap().parts;
    assert_eq!(
        (names(watch), watch[0].bin.as_deref()),
        (vec!["factory".to_string()], Some("factory"))
    );
}

fn quoted(v: &[String]) -> String {
    v.iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn doctor_checks_match_the_cli() {
    let ids: Vec<String> = reg()
        .mcus()
        .iter()
        .map(|m| format!("\"{}\"", m.id))
        .collect();
    has(
        CLI_DOCTOR,
        &format!("FAMILIES: [&str; {}] = [{}];", ids.len(), ids.join(", ")),
    );
    let mut count = 0;
    for m in reg().mcus() {
        for c in &m.doctor {
            let expect = match &c.expect {
                Expect::Runs => "Expect::Runs".to_string(),
                Expect::Line(l) => format!("Expect::Line(\"{l}\")"),
                Expect::Prefix(p) => format!("Expect::Prefix(\"{p}\")"),
            };
            let fix = c
                .fix
                .as_ref()
                .map_or("None".into(), |f| format!("Some(&[{}])", quoted(f)));
            let block = format!(
                "what: \"{}\",\n        for_mcu: &[\"{}\"],\n        probe: &[{}],\n        \
                 expect: {expect},\n        fix: {fix},\n        hint: \"{}\",",
                c.what,
                m.id,
                quoted(&c.probe),
                c.hint
            );
            has(CLI_DOCTOR, &block);
            count += 1;
        }
    }
    assert_eq!(CLI_DOCTOR.matches("for_mcu: &[\"").count(), count);
}

#[test]
fn connection_copy_matches_the_web() {
    for m in reg().mcus() {
        let c = &m.connection;
        has(
            WEB_COPY,
            &format!("{}: ['{}', '{}'],", m.id, c.label, c.hint),
        );
    }
}
