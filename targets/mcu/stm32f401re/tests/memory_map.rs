//! The definition's memory (`ram`, `[[flash_map]]`, `[execution]` erase
//! regions, write unit and erased value, the OTP block) is the memory ST's
//! chip data gives the STM32F401RE, as stm32-metapac ships it.

use stm32_metapac::metadata::{MemoryRegion, MemoryRegionKind, METADATA};

fn definition() -> toml::Table {
    include_str!("../dagon-mcu.toml").parse().unwrap()
}

fn int(v: &toml::Value, k: &str) -> u64 {
    u64::try_from(v[k].as_integer().unwrap()).unwrap()
}

/// ST's regions of one kind whose name starts with `prefix`, in address
/// order.
fn st(kind: MemoryRegionKind, prefix: &str) -> Vec<&'static MemoryRegion> {
    let [bank] = METADATA.memory else {
        panic!("one memory layout");
    };
    let mut r: Vec<_> = bank
        .iter()
        .filter(|r| r.kind == kind && r.name.starts_with(prefix))
        .collect();
    r.sort_by_key(|r| r.address);
    r
}

#[test]
fn the_ram_is_st_sram() {
    let [sram] = st(MemoryRegionKind::Ram, "")[..] else {
        panic!("one RAM region");
    };
    let ram = &definition()["ram"];
    assert_eq!(
        (int(ram, "at"), int(ram, "size")),
        (u64::from(sram.address), u64::from(sram.size))
    );
}

#[test]
fn the_erase_regions_are_st_flash_regions() {
    let d = definition();
    let exec = &d["execution"];
    let ours: Vec<(u64, u64, u64)> = exec["erase_regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (int(r, "start"), int(r, "len"), int(r, "unit")))
        .collect();
    let bank = st(MemoryRegionKind::Flash, "BANK_1");
    let theirs: Vec<(u64, u64, u64)> = bank
        .iter()
        .map(|r| {
            let s = r.settings.as_ref().unwrap();
            (r.address.into(), r.size.into(), s.erase_size.into())
        })
        .collect();
    assert_eq!(ours, theirs);
    for r in &bank {
        let s = r.settings.as_ref().unwrap();
        assert_eq!(
            int(exec, "write_unit"),
            u64::from(s.write_size),
            "{}",
            r.name
        );
        assert_eq!(
            int(exec, "erased_value"),
            u64::from(s.erase_value),
            "{}",
            r.name
        );
    }
}

#[test]
fn the_flash_map_spans_st_flash() {
    let bank = st(MemoryRegionKind::Flash, "BANK_1");
    let base = u64::from(bank[0].address);
    let bytes: u64 = bank.iter().map(|r| u64::from(r.size)).sum();
    let d = definition();
    let maps = d["flash_map"].as_array().unwrap();
    assert!(!maps.is_empty());
    for m in maps {
        assert_eq!(
            (int(m, "base"), int(m, "bytes")),
            (base, bytes),
            "{:?}",
            m["name"]
        );
    }
}

#[test]
fn the_otp_block_is_st_otp() {
    let [otp] = st(MemoryRegionKind::Flash, "OTP")[..] else {
        panic!("one OTP region");
    };
    let d = definition();
    let ours = d["peripheral"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"].as_str() == Some("otp"))
        .unwrap();
    assert_eq!(
        (int(ours, "base"), int(ours, "size")),
        (u64::from(otp.address), u64::from(otp.size))
    );
}
