//! The MCU packages lay out their firmware from the MCU definition the node
//! build is given, through the module `dagon_mcu_build` generates from it
//! (targets/mcu/build): that generator reads every definition as the
//! loader does, and each package's layout, build script (memory.x),
//! stage0 and slot store take addresses and lengths only from it, with no
//! copy of their own.

use std::path::{Path, PathBuf};

use dagon_topology::defs::{Binding, ImageKind, Registry};

#[allow(dead_code)]
#[path = "../../../targets/mcu/build/src/lib.rs"]
mod mcu_build;

fn mcu_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/mcu")
}

/// Every MCU package folder with its definition: (folder, MCU id).
fn packages() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(mcu_dir()).unwrap().flatten() {
        let def = e.path().join("dagon-mcu.toml");
        let Ok(text) = std::fs::read_to_string(&def) else {
            continue;
        };
        let t: toml::Table = text.parse().unwrap();
        out.push((e.path(), t["id"].as_str().unwrap().to_string()));
    }
    assert!(out.len() >= 3, "{out:?}");
    out
}

fn kind_name(k: ImageKind) -> String {
    format!("{k:?}").to_ascii_lowercase()
}

/// The generator reads each built-in definition as the loader does: the
/// same slots per image kind in slot order (partitions resolved), stage0,
/// RAM and boot record.
#[test]
fn the_generated_layout_is_the_loaders() {
    for (dir, id) in packages() {
        let text = std::fs::read_to_string(dir.join("dagon-mcu.toml")).unwrap();
        let def = mcu_build::Definition::parse(&text, |rel| {
            std::fs::read_to_string(dir.join(rel)).unwrap()
        });
        let m = Registry::builtin().mcu(&id).unwrap();
        let kinds: std::collections::BTreeSet<ImageKind> =
            m.slots.iter().map(|s| s.image).collect();
        for k in kinds {
            let mut want: Vec<(u8, (u32, u32))> = m
                .slots
                .iter()
                .filter(|s| s.image == k)
                .map(|s| (u8::from(s.binding == Binding::B), (s.start, s.size)))
                .collect();
            want.sort();
            let want: Vec<(u32, u32)> = want.into_iter().map(|(_, p)| p).collect();
            assert_eq!(def.slots(&kind_name(k)), want, "{id} {k:?}");
        }
        assert_eq!(
            def.stage0(),
            m.layout.stage0.map(|s| (s.at, s.size)),
            "{id}"
        );
        assert_eq!(def.ram(), m.ram.map(|r| (r.at, r.size)), "{id}");
        assert_eq!(
            def.boot_record(),
            m.record().map(|r| (r.at, r.size)),
            "{id}"
        );
        let rs = def.rs();
        assert!(
            rs.contains(&format!("pub const MCU_WIRE_ID: u16 = {:#x};", m.wire_id)),
            "{id}"
        );
    }
}

/// The source of `path` before its tests, without comments.
fn code(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let text = text.split("#[cfg(test)]").next().unwrap_or_default();
    text.lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Integer literals of 0x200 (512) and more in `src`: addresses and
/// lengths. Smaller ones are units, counts and the RP2040's 256-byte boot2.
fn large_literals(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        // Text in string literals (messages) is no layout.
        if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        let starts = b[i].is_ascii_digit()
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'));
        if !starts {
            i += 1;
            continue;
        }
        let at = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let lit = src[at..i].replace('_', "");
        let digits = ["u32", "u16", "u8", "usize", "u64"]
            .iter()
            .find_map(|s| lit.strip_suffix(s))
            .unwrap_or(&lit);
        let value = match digits.strip_prefix("0x") {
            Some(h) => u64::from_str_radix(h, 16).ok(),
            None => digits.parse::<u64>().ok(),
        };
        if value.is_some_and(|v| v >= 0x200) {
            out.push(src[at..i].to_string());
        }
    }
    out
}

/// Each package's layout, build script, stage0 and slot store hold no
/// address or length of their own: they take the generated module's.
#[test]
fn the_packages_use_the_generated_layout_only() {
    let mut checked = 0;
    for (dir, id) in packages() {
        let build = std::fs::read_to_string(dir.join("build.rs")).unwrap();
        assert!(
            build.contains("dagon_mcu_build::definition()"),
            "{id}: build.rs generates the layout"
        );
        assert!(
            !build.contains("#[path"),
            "{id}: build.rs includes no layout copy"
        );
        for rel in [
            "build.rs",
            "src/layout.rs",
            "src/store.rs",
            "src/regions.rs",
            "src/bl.rs",
            "stage0/src/main.rs",
        ] {
            let path = dir.join(rel);
            if !path.is_file() {
                continue;
            }
            checked += 1;
            let found = large_literals(&code(&path));
            assert!(found.is_empty(), "{id} {rel}: {found:?}");
        }
        let layout = code(&dir.join("src/layout.rs"));
        assert!(
            ["crate::slots::", "crate::PARTITION_TABLE"]
                .iter()
                .any(|g| layout.contains(g)),
            "{id}: layout.rs takes the generated slots"
        );
    }
    assert!(checked >= 10, "{checked}");
}

/// The literal scan finds what it is meant to find.
#[test]
fn the_literal_scan_finds_addresses() {
    assert_eq!(
        large_literals("const A: u32 = 0x1000_8000; let n = 8_000; x[1]; v2 = 3; \"RFC 8032\";"),
        ["0x1000_8000", "8_000"]
    );
    assert!(large_literals("thumbv6m 0x100 255u8 HEADER_SIZE as u32").is_empty());
}
