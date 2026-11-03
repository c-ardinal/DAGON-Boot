//! The builtin definitions carry what signing, provisioning, the host's ROM
//! path and the CLI know today. Public values are compared directly;
//! private CLI/web constants are checked against their source text, so a
//! change on either side fails here until both agree.

use dagon_format::header::{image_kind, slot_binding};
use dagon_sign::{image, provision};
use dagon_topology::defs::{Expect, Registry, Writer};

fn reg() -> &'static Registry {
    Registry::builtin()
}

const CLI_FLASH: &str = include_str!("../../dagon-cli/src/flash.rs");
const CLI_COMMANDS: &str = include_str!("../../dagon-cli/src/commands.rs");
const CLI_ESP: &str = include_str!("../../dagon-cli/src/esp.rs");
const CLI_DOCTOR: &str = include_str!("../../dagon-cli/src/doctor.rs");
const SIGN_PROVISION: &str = include_str!("../../dagon-sign/src/provision.rs");
const WEB_COPY: &str = include_str!("../../../web/src/app/copy.ts");

fn has(src: &str, needle: &str) {
    assert!(src.contains(needle), "source no longer has: {needle}");
}

/// `0x0800_8000` / `0x4000` as rustfmt-era source writes them.
fn rust_hex(v: u32) -> String {
    let h = format!("{v:X}");
    if h.len() <= 4 {
        return format!("0x{h}");
    }
    let h = format!("{v:08X}");
    format!("0x{}_{}", &h[..4], &h[4..])
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
    let Writer::Uf2Drive {
        family,
        sector,
        info_file,
        info_header,
        board_id,
    } = &rp.writer
    else {
        panic!("rp2040 writes a UF2 drive")
    };
    assert_eq!(*family, provision::UF2_FAMILY_RP2040);
    has(
        SIGN_PROVISION,
        &format!("const UF2_SECTOR: u32 = {};", rust_hex(*sector)),
    );
    let stage0 = rp.layout.stage0.unwrap();
    assert_eq!(
        (stage0.at, stage0.size),
        (base, provision::RP2040_STAGE0_MAX)
    );
    has(
        SIGN_PROVISION,
        &format!("const RP2040_XIP: u32 = {};", rust_hex(base)),
    );
    let bl = rp.layout.bootcore_log.unwrap();
    let app = rp.layout.app_log.unwrap();
    for (name, at) in [
        ("RP2040_BL_LOG_A", bl.faces[0]),
        ("RP2040_BL_LOG_B", bl.faces[1]),
        ("RP2040_APP_LOG_A", app.faces[0]),
        ("RP2040_APP_LOG_B", app.faces[1]),
    ] {
        has(
            SIGN_PROVISION,
            &format!("const {name}: u32 = RP2040_XIP + {};", rust_hex(at - base)),
        );
    }
    assert_eq!(bl.face_size, app.face_size);
    has(
        SIGN_PROVISION,
        &format!("const RP2040_LOG_FACE: u32 = {};", rust_hex(bl.face_size)),
    );
    has(
        CLI_FLASH,
        &format!("const UF2_INFO: &str = \"{info_file}\";"),
    );
    has(
        CLI_FLASH,
        &format!("const UF2_BOARD: &str = \"{board_id}\";"),
    );
    has(
        CLI_FLASH,
        &format!(".is_some_and(|l| l.starts_with(\"{info_header}\"));"),
    );
    has(
        CLI_COMMANDS,
        &format!("read_file(stage0_path)?, {})", rust_hex(base)),
    );

    let f4 = reg().mcu("stm32f401re").unwrap();
    assert_eq!(f4.default_flash_map().base, provision::F401_FLASH_BASE);
    let log_a = f4.layout.app_log.unwrap().faces[0];
    has(
        SIGN_PROVISION,
        &format!("const F401_LOG_A: u32 = {};", rust_hex(log_a)),
    );
    let Writer::ProbeRs { chip } = &f4.writer else {
        panic!("stm32f401re writes with probe-rs")
    };
    has(
        CLI_COMMANDS,
        &format!("const F401_CHIP: &str = \"{chip}\";"),
    );
}

#[test]
fn host_rom_path_matches() {
    use dagon_host::rom;
    let f4 = reg().mcu("stm32f401re").unwrap();
    let boot = reg()
        .slot("stm32f401re", image_kind::BOOTCORE, slot_binding::A)
        .unwrap();
    let app = reg()
        .slot("stm32f401re", image_kind::APP, slot_binding::A)
        .unwrap();
    assert_eq!(boot.start, rom::F401_BOOTCORE);
    assert_eq!(boot.header_addr, rom::F401_BOOTCORE_HEADER);
    assert_eq!(f4.layout.app_log.unwrap().faces[0], rom::F401_LOG_A);
    assert_eq!(app.start, rom::F401_APP_A);
    assert_eq!(app.size as usize, rom::F401_APP_A_LEN);
    let r = f4.rom_i2c.as_ref().unwrap();
    assert_eq!(r.erase_sectors, rom::F401_ERASE);
    assert_eq!(r.addr, dagon_romlink::ROM_ADDR);
    has(
        include_str!("../../dagon-host/src/ops.rs"),
        &format!("const ROM_ADDR: u8 = 0x{:X};", r.addr),
    );
}

#[test]
fn esp_writer_params_match_the_cli() {
    let esp = reg().mcu("esp32s3").unwrap();
    let Writer::Espflash {
        chip,
        flash_size,
        table_at,
        bootloader,
        checksums,
        usb_hint,
    } = &esp.writer
    else {
        panic!("esp32s3 writes with espflash")
    };
    has(CLI_ESP, &format!("const CHIP: &str = \"{chip}\";"));
    has(
        CLI_ESP,
        &format!("const FLASH_SIZE: &str = \"{flash_size}\";"),
    );
    has(
        CLI_ESP,
        &format!("const TABLE_AT: u32 = {};", rust_hex(*table_at)),
    );
    let usb = usb_hint.unwrap();
    has(
        CLI_ESP,
        &format!(
            "const USB_JTAG: (u16, u16) = (0x{:04X}, 0x{:04X});",
            usb.vid, usb.pid
        ),
    );
    let pkg = "targets/mcu/esp32s3";
    let partitions = esp.partitions.as_deref().unwrap();
    has(
        CLI_ESP,
        &format!("const PARTITIONS: &str = \"{pkg}/{partitions}\";"),
    );
    let (dir, file) = bootloader.rsplit_once('/').unwrap();
    has(
        CLI_ESP,
        &format!("const VENDOR_DIR: &str = \"{pkg}/{dir}\";"),
    );
    has(CLI_ESP, &format!("const BOOTLOADER: &str = \"{file}\";"));
    assert_eq!(checksums, &format!("{dir}/SHA256SUMS"));
    has(CLI_ESP, "vendor.join(\"SHA256SUMS\")");
}

#[test]
fn first_write_parts_match_the_cli() {
    for m in reg().mcus() {
        for p in &m.parts {
            let bin = p
                .bin
                .as_deref()
                .map_or("None".into(), |b| format!("Some(\"{b}\")"));
            has(
                CLI_FLASH,
                &format!("part(\"{}\", mcu_pkg(), \"{}\", {bin})", p.name, p.sub),
            );
        }
    }
    for b in reg().boards() {
        for p in &b.parts {
            let bin = p
                .bin
                .as_deref()
                .map_or("None".into(), |b| format!("Some(\"{b}\")"));
            has(
                CLI_FLASH,
                &format!("part(\"{}\", board_pkg, \"{}\", {bin})", p.name, p.sub),
            );
        }
    }
    let mcu_parts: usize = reg().mcus().iter().map(|m| m.parts.len()).sum();
    let board_parts: usize = reg().boards().iter().map(|b| b.parts.len()).sum();
    assert_eq!(CLI_FLASH.matches("mcu_pkg(), \"").count(), mcu_parts);
    assert_eq!(
        CLI_FLASH.matches("part(\"").count(),
        mcu_parts + board_parts
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
