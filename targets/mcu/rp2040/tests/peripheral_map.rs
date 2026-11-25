//! The definition's register blocks (`[[peripheral]] base`) start where the
//! RP2040's chip data puts them, as rp-pac ships it (generated from
//! Raspberry Pi's SVD; the data embassy-rp builds on), and every block of
//! that data is declared.

use std::collections::BTreeMap;

/// (name, address) of rp-pac's blocks.
macro_rules! blocks {
    ($($name:ident),* $(,)?) => {
        [$((stringify!($name), rp_pac::$name.as_ptr() as usize as u64)),*]
    };
}

/// Every block rp-pac 7 has for the RP2040 (`rp_pac::rp2040`, its `pub
/// const`s).
fn chip_data() -> Vec<(&'static str, u64)> {
    blocks![
        XIP_CTRL,
        SSI,
        SYSINFO,
        SYSCFG,
        CLOCKS,
        RESETS,
        PSM,
        IO_BANK0,
        IO_QSPI,
        PADS_BANK0,
        PADS_QSPI,
        XOSC,
        PLL_SYS,
        PLL_USB,
        BUSCTRL,
        UART0,
        UART1,
        SPI0,
        SPI1,
        I2C0,
        I2C1,
        ADC,
        PWM,
        TIMER,
        WATCHDOG,
        RTC,
        ROSC,
        VREG_AND_CHIP_RESET,
        TBMAN,
        DMA,
        USB_DPRAM,
        USB,
        PIO0,
        PIO1,
        SIO,
    ]
    .to_vec()
}

/// Definition ids whose block the chip data names otherwise.
const NAMED: [(&str, &str); 3] = [
    ("xip-ssi", "SSI"),
    ("usbctrl-dpram", "USB_DPRAM"),
    ("usbctrl-regs", "USB"),
];

/// Blocks the chip data has no peripheral for: the boot ROM, the XIP
/// stream FIFO's bus port and the Cortex-M0+ private peripheral bus
/// (RP2040 datasheet section 2.2, address map).
const NOT_IN_CHIP_DATA: [&str; 3] = ["rom", "xip-aux", "ppb"];

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
