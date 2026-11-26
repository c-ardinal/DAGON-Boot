//! The definition's register blocks (`[[peripheral]] base`, `size`) are the
//! address ranges of the ESP32-S3 Technical Reference Manual (version 1.8,
//! chapter 4 "System and Memory", table 4.3-3 "Module/Peripheral Address
//! Mapping"). The blocks the table leaves out lie in ranges it calls
//! reserved; `peripheral_map.rs` holds those to Espressif's SVD.

/// Table 4.3-3: (target, low address, high address), reserved rows left out.
const TRM: [(&str, u64, u64); 44] = [
    ("UART Controller 0", 0x6000_0000, 0x6000_0FFF),
    ("SPI Controller 1", 0x6000_2000, 0x6000_2FFF),
    ("SPI Controller 0", 0x6000_3000, 0x6000_3FFF),
    ("GPIO", 0x6000_4000, 0x6000_4FFF),
    ("eFuse Controller", 0x6000_7000, 0x6000_7FFF),
    ("Low-Power Management", 0x6000_8000, 0x6000_8FFF),
    ("IO MUX", 0x6000_9000, 0x6000_9FFF),
    ("I2S Controller 0", 0x6000_F000, 0x6000_FFFF),
    ("UART Controller 1", 0x6001_0000, 0x6001_0FFF),
    ("I2C Controller 0", 0x6001_3000, 0x6001_3FFF),
    ("UHCI0", 0x6001_4000, 0x6001_4FFF),
    ("Remote Control Peripheral", 0x6001_6000, 0x6001_6FFF),
    ("Pulse Count Controller", 0x6001_7000, 0x6001_7FFF),
    ("LED PWM Controller", 0x6001_9000, 0x6001_9FFF),
    ("Motor Control PWM 0", 0x6001_E000, 0x6001_EFFF),
    ("Timer Group 0", 0x6001_F000, 0x6001_FFFF),
    ("Timer Group 1", 0x6002_0000, 0x6002_0FFF),
    ("RTC SLOW Memory", 0x6002_1000, 0x6002_2FFF),
    ("System Timer", 0x6002_3000, 0x6002_3FFF),
    ("SPI Controller 2", 0x6002_4000, 0x6002_4FFF),
    ("SPI Controller 3", 0x6002_5000, 0x6002_5FFF),
    ("SYSCON", 0x6002_6000, 0x6002_6FFF),
    ("I2C Controller 1", 0x6002_7000, 0x6002_7FFF),
    ("SD/MMC Host Controller", 0x6002_8000, 0x6002_8FFF),
    ("Two-wire Automotive Interface", 0x6002_B000, 0x6002_BFFF),
    ("Motor Control PWM 1", 0x6002_C000, 0x6002_CFFF),
    ("I2S Controller 1", 0x6002_D000, 0x6002_DFFF),
    ("UART Controller 2", 0x6002_E000, 0x6002_EFFF),
    ("USB Serial/JTAG Controller", 0x6003_8000, 0x6003_8FFF),
    ("USB External Control registers", 0x6003_9000, 0x6003_9FFF),
    ("AES Accelerator", 0x6003_A000, 0x6003_AFFF),
    ("SHA Accelerator", 0x6003_B000, 0x6003_BFFF),
    ("RSA Accelerator", 0x6003_C000, 0x6003_CFFF),
    ("Digital Signature", 0x6003_D000, 0x6003_DFFF),
    ("HMAC Accelerator", 0x6003_E000, 0x6003_EFFF),
    ("GDMA Controller", 0x6003_F000, 0x6003_FFFF),
    ("ADC Controller", 0x6004_0000, 0x6004_0FFF),
    ("Camera-LCD Controller", 0x6004_1000, 0x6004_1FFF),
    ("USB core registers", 0x6008_0000, 0x600B_FFFF),
    ("System Registers", 0x600C_0000, 0x600C_0FFF),
    ("PMS Registers", 0x600C_1000, 0x600C_1FFF),
    ("Interrupt Matrix", 0x600C_2000, 0x600C_2FFF),
    (
        "External Memory Encryption and Decryption",
        0x600C_C000,
        0x600C_CFFF,
    ),
    ("World Controller", 0x600D_0000, 0x600D_0FFF),
];

/// Rows of the table that are memory, not a register block.
const MEMORY: [&str; 1] = ["RTC SLOW Memory"];

/// Definition blocks in ranges the table calls reserved.
const RESERVED_IN_TRM: [&str; 10] = [
    "fe2",
    "fe",
    "i2c-ana-mst",
    "nrx",
    "bb",
    "peri-backup",
    "rng",
    "extmem",
    "mmu-table",
    "assist-debug",
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
fn every_block_is_a_row_of_the_manual_or_in_a_reserved_range() {
    let mut wrong = Vec::new();
    for (id, base, size) in definition() {
        let last = base + size - 1;
        let reserved = RESERVED_IN_TRM.contains(&id.as_str());
        match TRM.iter().find(|(_, low, _)| *low == base) {
            Some(_) if reserved => {
                wrong.push(format!("{id}: listed as reserved, the manual has it"))
            }
            Some((_, _, high)) if *high == last => {}
            Some((name, _, high)) => {
                wrong.push(format!("{id}: ends at {last:#x}, {name} at {high:#x}"));
            }
            None if reserved => {
                if let Some((name, ..)) = TRM
                    .iter()
                    .find(|(_, low, high)| base <= *high && *low <= last)
                {
                    wrong.push(format!("{id}: overlaps {name}"));
                }
            }
            None => wrong.push(format!("{id}: {base:#x} is no row of the manual")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_register_block_of_the_manual_is_declared() {
    let blocks = definition();
    let missing: Vec<_> = TRM
        .iter()
        .filter(|(name, ..)| !MEMORY.contains(name))
        .filter(|(_, low, _)| !blocks.iter().any(|(_, base, _)| base == low))
        .map(|(name, low, _)| format!("{name} at {low:#x}"))
        .collect();
    assert!(missing.is_empty(), "{missing:#?}");
}
