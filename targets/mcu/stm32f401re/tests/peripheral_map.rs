//! The definition's register blocks (`[[peripheral]] base`) lie where ST's
//! chip data for the STM32F401RE puts them, as stm32-metapac ships it: a
//! block ST's data names starts at ST's address, and every block of ST's
//! data lies inside one the definition declares.

use std::collections::BTreeMap;

/// Definition ids whose block ST's data names otherwise.
const NAMED: [(&str, &str); 4] = [
    ("rtc-bkp", "RTC"),
    // The block starts at the unique device ID.
    ("device-signature", "UID"),
    // One block for the ports: it starts at port A.
    ("gpio", "GPIOA"),
    ("usb-otg-fs", "USB_OTG_FS"),
];

/// Blocks ST's data has no peripheral for: the I2S extension blocks and
/// SPI4, and memory that is not a peripheral (RM0368 section 2.3, memory
/// map).
const NOT_IN_ST_DATA: [&str; 7] = [
    "i2s2ext",
    "i2s3ext",
    "spi4",
    "system-memory",
    "otp",
    "option-bytes",
    "ppb",
];

/// (id, base, size) of every block the definition declares.
fn definition() -> Vec<(String, u64, u64)> {
    let t: toml::Table = include_str!("../dagon-mcu.toml").parse().unwrap();
    t["peripheral"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let int = |k: &str| u64::try_from(p[k].as_integer().unwrap()).unwrap();
            (
                p["id"].as_str().unwrap().to_string(),
                int("base"),
                int("size"),
            )
        })
        .collect()
}

#[test]
fn every_block_starts_where_st_chip_data_puts_it() {
    let st: BTreeMap<&str, u64> = stm32_metapac::metadata::METADATA
        .peripherals
        .iter()
        .map(|p| (p.name, p.address))
        .collect();
    let mut wrong = Vec::new();
    for (id, base, _) in definition() {
        let name = NAMED
            .iter()
            .find(|(d, _)| *d == id)
            .map(|(_, s)| s.to_string())
            .unwrap_or_else(|| id.to_uppercase());
        match st.get(name.as_str()) {
            Some(&at) if at == base => {}
            Some(&at) => wrong.push(format!("{id}: {base:#x}, ST's data {at:#x}")),
            None if NOT_IN_ST_DATA.contains(&id.as_str()) => {}
            None => wrong.push(format!("{id}: not in ST's data as {name}")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_block_of_st_chip_data_is_declared() {
    let blocks = definition();
    let outside: Vec<_> = stm32_metapac::metadata::METADATA
        .peripherals
        .iter()
        .filter(|p| {
            !blocks
                .iter()
                .any(|(_, base, size)| (*base..base + size).contains(&p.address))
        })
        .map(|p| format!("{} at {:#x}", p.name, p.address))
        .collect();
    assert!(outside.is_empty(), "{outside:#?}");
}
