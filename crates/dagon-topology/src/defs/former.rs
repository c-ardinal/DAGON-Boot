//! The port registry, flash maps and board table the topology checks held
//! before they read the definitions; kept only so the parity tests pin the
//! builtin definitions to these values.

use crate::schema::{ControlKind, LinkKind, RomEntry, UartIface};

/// An MCU known to the port registry.
#[derive(Debug, Clone, Copy)]
pub struct Mcu {
    pub id: &'static str,
    /// Port kind the MCU must use (`self-boot` or `vendor-stage2`).
    pub port: &'static str,
    /// GPIO0..GPIOn-1 a node without a named board is checked against (P1);
    /// 0 when its pins are named by port (`PB4`), not numbered.
    pub gpio_count: u8,
}

pub const MCUS: &[Mcu] = &[
    Mcu {
        id: "rp2040",
        port: "self-boot",
        gpio_count: 30,
    },
    Mcu {
        id: "stm32f401re",
        port: "self-boot",
        gpio_count: 0,
    },
    Mcu {
        id: "esp32s3",
        port: "vendor-stage2",
        gpio_count: 49,
    },
];

/// A named flash layout and the flash size it needs.
#[derive(Debug, Clone, Copy)]
pub struct FlashMap {
    pub name: &'static str,
    pub mcu: &'static str,
    pub bytes: u32,
}

const KIB: u32 = 1024;
const MIB: u32 = 1024 * KIB;

/// The first entry for an MCU is its default.
pub const FLASH_MAPS: &[FlashMap] = &[
    FlashMap {
        name: "rp2040-4m",
        mcu: "rp2040",
        bytes: 4 * MIB,
    },
    FlashMap {
        name: "f401-default",
        mcu: "stm32f401re",
        bytes: 512 * KIB,
    },
    FlashMap {
        name: "esp32s3-16m",
        mcu: "esp32s3",
        bytes: 16 * MIB,
    },
];

/// One pin brought out on the board's header. `position` is the printed
/// header label; `None` means not yet confirmed on the real board.
#[derive(Debug, Clone, Copy)]
pub struct HeaderPin {
    pub pin: &'static str,
    pub position: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I2cPair {
    pub controller: &'static str,
    pub sda: &'static str,
    pub scl: &'static str,
}

/// How a node board is wired to the hub unless the topology says otherwise.
#[derive(Debug, Clone, Copy)]
pub struct DefaultLink {
    pub kind: LinkKind,
    pub iface: Option<UartIface>,
    pub addr: Option<u8>,
    pub baud: Option<u32>,
    pub control: ControlKind,
    pub rom_entry: Option<RomEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct Board {
    /// Key used in `nodes[].board`.
    pub id: &'static str,
    pub product: &'static str,
    pub mcu_name: &'static str,
    pub mcu: &'static str,
    pub header: &'static [HeaderPin],
    /// I2C pin pairs usable from the header (controllers used on the board
    /// itself are left out).
    pub i2c_pairs: &'static [I2cPair],
    /// I2C pin pairs the MCU's ROM bootloader listens on.
    pub rom_i2c_pairs: &'static [I2cPair],
    pub i2c_target: bool,
    pub flash_bytes: u32,
    /// `None` for the coordinator.
    pub default_link: Option<DefaultLink>,
    /// Which line drawing the UI uses for this board.
    pub drawing: &'static str,
}

const fn hp(pin: &'static str, position: Option<&'static str>) -> HeaderPin {
    HeaderPin { pin, position }
}

const fn pair(controller: &'static str, sda: &'static str, scl: &'static str) -> I2cPair {
    I2cPair {
        controller,
        sda,
        scl,
    }
}

const F401_ROM_I2C: &[I2cPair] = &[
    pair("i2c3", "PB4", "PA8"),
    pair("i2c1", "PB7", "PB6"),
    pair("i2c2", "PB3", "PB10"),
];

pub const BOARDS: &[Board] = &[
    Board {
        id: "waveshare-rp2040-touch-lcd-1.28",
        product: "Waveshare RP2040-Touch-LCD-1.28",
        mcu_name: "RP2040",
        mcu: "rp2040",
        header: &[
            hp("GPIO16", None),
            hp("GPIO17", None),
            hp("GPIO18", None),
            hp("GPIO26", None),
            hp("GPIO27", None),
            hp("GPIO28", None),
        ],
        // I2C1 is used on the board itself (GPIO6/7).
        i2c_pairs: &[pair("i2c0", "GPIO16", "GPIO17")],
        rom_i2c_pairs: &[],
        i2c_target: true,
        flash_bytes: 4 * MIB,
        default_link: None,
        drawing: "round-lcd",
    },
    Board {
        id: "nucleo-f401re",
        product: "NUCLEO-F401RE",
        mcu_name: "STM32F401RE",
        mcu: "stm32f401re",
        header: &[
            hp("PB4", Some("D5")),
            hp("PA8", Some("D7")),
            hp("NRST", None),
            hp("BOOT0", None),
        ],
        // D14/D15 (PB9/PB8) are I2C1 too but the ROM does not listen there.
        i2c_pairs: &[
            pair("i2c3", "PB4", "PA8"),
            pair("i2c1", "PB7", "PB6"),
            pair("i2c2", "PB3", "PB10"),
            pair("i2c1", "PB9", "PB8"),
        ],
        rom_i2c_pairs: F401_ROM_I2C,
        i2c_target: true,
        flash_bytes: 512 * KIB,
        default_link: Some(DefaultLink {
            kind: LinkKind::I2c,
            iface: None,
            addr: Some(0x42),
            baud: None,
            control: ControlKind::Full,
            rom_entry: Some(RomEntry::Stm32I2cRom),
        }),
        drawing: "nucleo-64",
    },
    Board {
        id: "m5stopwatch",
        product: "M5StopWatch",
        mcu_name: "ESP32-S3",
        mcu: "esp32s3",
        header: &[hp("GPIO43", Some("G43")), hp("GPIO44", Some("G44"))],
        i2c_pairs: &[],
        rom_i2c_pairs: &[],
        // esp-hal has no I2C target mode for the ESP32-S3.
        i2c_target: false,
        flash_bytes: 16 * MIB,
        default_link: Some(DefaultLink {
            kind: LinkKind::Uart,
            iface: Some(UartIface::PioUart),
            addr: None,
            baud: Some(115_200),
            control: ControlKind::Cooperative,
            rom_entry: Some(RomEntry::ForceDownloadBoot),
        }),
        drawing: "stopwatch",
    },
];

/// First board in the table for an MCU, used when `nodes[].board` is omitted.
pub fn default_board(mcu: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|b| b.mcu == mcu)
}

pub fn default_flash_map(mcu: &str) -> Option<&'static FlashMap> {
    FLASH_MAPS.iter().find(|m| m.mcu == mcu)
}
