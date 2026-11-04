//! DAGON board package for the NUCLEO-F401RE (topology board id
//! `nucleo-f401re`): 512 KiB flash, LD2, DAGON's link to the 親機 as an I2C
//! target on I2C3. Every board fact (pins, buses, the link address, the
//! boot LED) is read from `dagon-board.toml` when the package builds
//! ([`def`]); nothing here repeats one.
//!
//! | Use | Pins |
//! |---|---|
//! | DAGON link: I2C3 target (SCL, SDA), address [`I2C_ADDR`] | PA8 (D7), PB4 (D5) |
//! | User LED LD2 | PA5 |
//! | Arduino I2C1 (SDA, SCL), for what is stacked on the headers | PB9 (D14), PB8 (D15) |
//! | ST-LINK virtual COM port, USART2 (TX, RX) | PA2, PA3 |
//! | User button B1 | PC13 |
//! | A4, A5 (ADC1) | PC1, PC0 |
//!
//! [`Board`] implements `BoardDef`: DAGON takes the flash, the IWDG and the
//! I2C3 target; the app gets [`Io`], with the Arduino I2C as plain
//! peripherals (what is stacked on the headers is the app's: its driver
//! binds the bus's interrupts). An app that keeps the chip's peripherals
//! itself moves out only DAGON's with `dagon_pins!` and calls `start`.
//! This package binds the I2C3 interrupts and those of the link's DMA
//! streams itself; the streams come from the MCU definition's request
//! mapping ([`def::dma`]).
#![no_std]

/// The board's facts from `dagon-board.toml`.
#[macro_use]
pub mod def {
    include!(concat!(env!("OUT_DIR"), "/def.rs"));

    /// Pins and blocks by their definition names, and `take!`.
    #[cfg(feature = "hal")]
    #[macro_use]
    pub mod pins {
        include!(concat!(env!("OUT_DIR"), "/pins.rs"));
    }

    /// The DMA streams of the buses this package opens or hands out with
    /// DMA, `take_dma!` and `bind_<bus>_dma!`.
    #[cfg(feature = "hal")]
    #[macro_use]
    pub mod dma {
        include!(concat!(env!("OUT_DIR"), "/dma.rs"));
    }
}

#[cfg(feature = "hal")]
pub use hal::*;

/// The node's 7-bit I2C target address (`[default_link] addr`; the STM32
/// ROM loader keeps 0x39).
pub const I2C_ADDR: u8 = match def::DEFAULT_LINK_ADDR {
    Some(a) => a,
    None => panic!("dagon-board.toml: default_link.addr"),
};
pub const BOARD_ID: &str = def::BOARD_ID;
pub const FLASH_SIZE: usize = def::FLASH_BYTES as usize;

/// Moves DAGON's peripherals (flash, IWDG, the I2C3 link with its DMA
/// streams) out of `embassy_stm32::Peripherals`; the rest stays the app's.
#[cfg(feature = "hal")]
#[macro_export]
macro_rules! dagon_pins {
    ($p:ident) => {
        $crate::DagonParts {
            flash: $p.FLASH,
            iwdg: $p.IWDG,
            link: $crate::Link {
                i2c: $crate::take!($p.link),
                scl: $crate::take!($p.link_scl),
                sda: $crate::take!($p.link_sda),
                tx_dma: $crate::take_dma!($p.link_tx),
                rx_dma: $crate::take_dma!($p.link_rx),
            },
        }
    };
}

#[cfg(feature = "hal")]
mod hal {
    use dagon_mcu_stm32f401re::links::Kind;
    pub use dagon_mcu_stm32f401re::node::DagonParts;
    use dagon_mcu_stm32f401re::node::{App, AppEvents, I2cTarget, LogError};
    use dagon_mcu_stm32f401re::watchdog::Guard;
    pub use dagon_mcu_stm32f401re::BoardDef;
    use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
    use embassy_stm32::peripherals::ADC1;
    use embassy_stm32::{pac, peripherals, Peri, Peripherals};

    use super::def::dma::{ArduinoI2cRxDma, ArduinoI2cTxDma, LinkRxDma, LinkTxDma};
    use super::def::pins::{
        ArduinoI2c as ArduinoI2cBus, ArduinoI2cScl, ArduinoI2cSda, Button, Led, Link as LinkBus,
        LinkScl, LinkSda, Vcp, VcpRx, VcpTx, A4, A5,
    };
    use super::{BOARD_ID, FLASH_SIZE, I2C_ADDR};

    bind_link_dma!(pub(crate) struct Irqs {
        I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
        I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;
    });

    pub struct Board;

    /// Starts DAGON on this board (I2C target link; guard task
    /// `dagon_mcu_stm32f401re::node::DAGON_TASK` serves it) with the app's
    /// self-check, extensions and confirm choice; returns the node's events.
    /// The board has no bring-up, so no `app.bring_up` is emitted. See
    /// `dagon_mcu_stm32f401re::node::start`.
    pub fn start<const N: usize>(
        spawner: embassy_executor::Spawner,
        parts: DagonParts<Link>,
        guard: &'static Guard<N>,
        tag: &'static str,
        app: App,
        uplink: Kind,
    ) -> Result<AppEvents, LogError> {
        dagon_mcu_stm32f401re::node::start::<Board, N>(spawner, parts, guard, tag, app, uplink)
    }

    /// Up `i2c` on this board: I2C3 as a target at
    /// [`I2C_ADDR`](super::I2C_ADDR) with the DAGON mailbox.
    pub fn uplink_i2c(link: Link) -> dagon_mcu_stm32f401re::links::I2c {
        let (target, regs) = Board::open_link(link);
        dagon_mcu_stm32f401re::links::I2c::new(target, regs)
    }

    /// The DAGON link's peripherals and pins (bus `link`).
    pub struct Link {
        pub i2c: Peri<'static, LinkBus>,
        pub scl: Peri<'static, LinkScl>,
        pub sda: Peri<'static, LinkSda>,
        pub tx_dma: Peri<'static, LinkTxDma>,
        pub rx_dma: Peri<'static, LinkRxDma>,
    }

    /// The Arduino I2C1 (bus `arduino-i2c`) with the DMA streams that
    /// serve it, as plain peripherals: what is stacked on the headers is
    /// the app's, and its driver binds the interrupts.
    pub struct ArduinoI2c {
        pub i2c: Peri<'static, ArduinoI2cBus>,
        pub scl: Peri<'static, ArduinoI2cScl>,
        pub sda: Peri<'static, ArduinoI2cSda>,
        pub tx_dma: Peri<'static, ArduinoI2cTxDma>,
        pub rx_dma: Peri<'static, ArduinoI2cRxDma>,
    }

    /// What the app gets on this board.
    pub struct Io {
        pub led: Peri<'static, Led>,
        pub arduino_i2c: ArduinoI2c,
        pub spare: Spare,
    }

    /// A few common peripherals DAGON does not use: the ST-LINK virtual
    /// COM port (USART2), the user button B1, ADC1 with A4/A5.
    pub struct Spare {
        pub usart2: Peri<'static, Vcp>,
        pub pa2: Peri<'static, VcpTx>,
        pub pa3: Peri<'static, VcpRx>,
        pub pc13: Peri<'static, Button>,
        pub adc1: Peri<'static, ADC1>,
        pub pc0: Peri<'static, A5>,
        pub pc1: Peri<'static, A4>,
    }

    impl BoardDef for Board {
        const BOARD_ID: &'static str = BOARD_ID;
        const FLASH_SIZE: usize = FLASH_SIZE;
        type Link = Link;
        type UserIo = Io;

        fn split(p: Peripherals) -> (DagonParts<Link>, Io) {
            let link = Link {
                i2c: take!(p.link),
                scl: take!(p.link_scl),
                sda: take!(p.link_sda),
                tx_dma: take_dma!(p.link_tx),
                rx_dma: take_dma!(p.link_rx),
            };
            let io = Io {
                led: take!(p.led),
                arduino_i2c: ArduinoI2c {
                    i2c: take!(p.arduino_i2c),
                    scl: take!(p.arduino_i2c_scl),
                    sda: take!(p.arduino_i2c_sda),
                    tx_dma: take_dma!(p.arduino_i2c_tx),
                    rx_dma: take_dma!(p.arduino_i2c_rx),
                },
                spare: Spare {
                    usart2: take!(p.vcp),
                    pa2: take!(p.vcp_tx),
                    pa3: take!(p.vcp_rx),
                    pc13: take!(p.button),
                    adc1: p.ADC1,
                    pc0: take!(p.a5),
                    pc1: take!(p.a4),
                },
            };
            let dagon = DagonParts {
                flash: p.FLASH,
                iwdg: p.IWDG,
                link,
            };
            (dagon, io)
        }

        fn open_link(l: Link) -> (I2cTarget, pac::i2c::I2c) {
            let mut cfg = i2c::Config::default();
            cfg.sda_pullup = true;
            cfg.scl_pullup = true;
            let target = I2c::new(l.i2c, l.scl, l.sda, l.tx_dma, l.rx_dma, Irqs, cfg)
                .into_slave_multimaster(SlaveAddrConfig::basic(I2C_ADDR));
            (target, pac::I2C3)
        }
    }
}
