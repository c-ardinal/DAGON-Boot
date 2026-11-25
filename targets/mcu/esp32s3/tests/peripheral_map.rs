//! The definition's register blocks (`[[peripheral]] base`) start where the
//! ESP32-S3's chip data puts them, as the esp32s3 PAC ships it (generated
//! from Espressif's SVD; the data esp-hal builds on), and every block of
//! that data is declared.

use std::collections::BTreeMap;

/// (name, address) of the PAC's blocks.
macro_rules! blocks {
    ($($name:ident),* $(,)?) => {
        [$((stringify!($name), esp32s3::$name::PTR as usize as u64)),*]
    };
}

/// Every block esp32s3 0.36 has (its `pub type`s over `Periph`).
fn chip_data() -> Vec<(&'static str, u64)> {
    blocks![
        UART0,
        SPI1,
        SPI0,
        GPIO,
        GPIO_SD,
        FE2,
        FE,
        EFUSE,
        RTC_CNTL,
        RTC_IO,
        SENS,
        RTC_I2C,
        IO_MUX,
        I2C_ANA_MST,
        I2S0,
        UART1,
        I2C0,
        UHCI0,
        RMT,
        PCNT,
        LEDC,
        NRX,
        BB,
        MCPWM0,
        TIMG0,
        TIMG1,
        SYSTIMER,
        SPI2,
        SPI3,
        APB_CTRL,
        I2C1,
        SDHOST,
        PERI_BACKUP,
        TWAI0,
        MCPWM1,
        I2S1,
        UART2,
        RNG,
        USB_DEVICE,
        USB_WRAP,
        AES,
        SHA,
        RSA,
        DS,
        HMAC,
        DMA,
        APB_SARADC,
        LCD_CAM,
        USB_FS,
        SYSTEM,
        SENSITIVE,
        INTERRUPT_CORE0,
        INTERRUPT_CORE1,
        EXTMEM,
        MMU_TABLE,
        XTS_AES,
        ASSIST_DEBUG,
        WCL,
    ]
    .to_vec()
}

/// Definition ids whose block the chip data names otherwise.
const NAMED: [(&str, &str); 6] = [
    ("twai", "TWAI0"),
    ("usb-serial-jtag", "USB_DEVICE"),
    ("gdma", "DMA"),
    ("usb-otg", "USB_FS"),
    // One block for both cores' interrupt matrix.
    ("interrupt-matrix", "INTERRUPT_CORE0"),
    ("world-controller", "WCL"),
];

/// Blocks the chip data has no peripheral for.
const NOT_IN_CHIP_DATA: [&str; 0] = [];

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
fn every_block_starts_where_the_chip_data_puts_it() {
    let chip: BTreeMap<&str, u64> = chip_data().into_iter().collect();
    let mut wrong = Vec::new();
    for (id, base, _) in definition() {
        let name = NAMED
            .iter()
            .find(|(d, _)| *d == id)
            .map(|(_, s)| s.to_string())
            .unwrap_or_else(|| id.to_uppercase().replace('-', "_"));
        match chip.get(name.as_str()) {
            Some(&at) if at == base => {}
            Some(&at) => wrong.push(format!("{id}: {base:#x}, the chip data {at:#x}")),
            None if NOT_IN_CHIP_DATA.contains(&id.as_str()) => {}
            None => wrong.push(format!("{id}: not in the chip data as {name}")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_block_of_the_chip_data_is_declared() {
    let blocks = definition();
    let outside: Vec<_> = chip_data()
        .into_iter()
        .filter(|(_, at)| {
            !blocks
                .iter()
                .any(|(_, base, size)| (*base..base + size).contains(at))
        })
        .map(|(name, at)| format!("{name} at {at:#x}"))
        .collect();
    assert!(outside.is_empty(), "{outside:#?}");
}
