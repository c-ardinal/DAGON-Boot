//! The builtin definitions carry what signing, provisioning, the host's ROM
//! path and the CLI knew before they read them; the CLI's first-write code
//! is checked to hold no MCU or board knowledge of its own.

use dagon_sign::image;
use dagon_topology::defs::{Registry, Writer};

fn reg() -> &'static Registry {
    Registry::builtin()
}

const CLI_FLASH: &str = include_str!("../../dagon-cli/src/flash.rs");
const CLI_COMMANDS: &str = include_str!("../../dagon-cli/src/commands.rs");
const CLI_ESP: &str = include_str!("../../dagon-cli/src/esp.rs");

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
    assert_eq!(seen, 10);
}

#[test]
fn provision_layouts_match() {
    let rp = reg().mcu("rp2040").unwrap();
    let base = rp.default_flash_map().base;
    let Writer::Uf2Drive { family, sector, .. } = &rp.writer else {
        panic!("rp2040 writes a UF2 drive")
    };
    // The values dagon-sign's provision.rs held before it read the
    // definitions; its first_write_golden test pins the bytes built from them.
    assert_eq!(*family, 0xE48B_FF56);
    assert_eq!(*sector, 0x1000);
    let stage0 = rp.layout.stage0.unwrap();
    assert_eq!((stage0.at, stage0.size), (base, 0x4000));
    let f4 = reg().mcu("stm32f401re").unwrap();
    assert_eq!(f4.default_flash_map().base, 0x0800_0000);
    assert!(matches!(f4.writer, Writer::ProbeRs { .. }));
}

#[test]
fn host_rom_path_matches() {
    use dagon_host::rom::{rom_maps, RomSlot};
    // The F401's layout: stage0 in sector 0, one log for both targets in
    // sectors 1-2, boot core A in sector 4, APP-A in sector 5.
    let maps = rom_maps(reg());
    assert_eq!(maps.len(), 1, "only the F401 has a ROM I2C loader");
    let m = &maps[0];
    assert_eq!(m.wire_id, dagon_format::header::mcu_id::STM32F401XE);
    assert_eq!(
        m.bootcore,
        RomSlot {
            header: 0x0801_0000,
            load: 0x0801_0200,
            max_payload: 0x1_0000 - 512
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
    assert_eq!(m.log_a, 0x0800_4000);
    assert_eq!(m.stage0, Some((0x0800_0000, 0x4000)));
    assert!(m.shared_log);
    assert_eq!(m.erase, [0, 1, 2, 4, 5]);
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
    assert_eq!(
        names(&tree.mcu("stm32f401re").unwrap().parts),
        ["stage0", "bootcore"]
    );
    assert!(tree.mcu("esp32s3").unwrap().parts.is_empty());
    let watch = &tree.board("m5stopwatch").unwrap().parts;
    assert_eq!(
        (names(watch), watch[0].bin.as_deref()),
        (vec!["factory".to_string()], Some("factory"))
    );
}
