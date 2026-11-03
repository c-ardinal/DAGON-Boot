//! DAGON board package for the NUCLEO-F401RE (topology board id
//! `nucleo-f401re`) with an X-NUCLEO-53L0A1 stacked on its Arduino headers:
//! 512 KiB flash, LD2 on PA5, DAGON's link to the hub as an I2C target on
//! I2C3, and the shield's 4-digit 7-segment display ([`seg`] text,
//! [`shield`] expanders) on I2C1, which belongs to the display.
//!
//! | Use | Pins |
//! |---|---|
//! | DAGON link: I2C3 target (SCL, SDA), address [`I2C_ADDR`] (NC on the shield) | PA8 (D7), PB4 (D5) |
//! | User LED LD2 | PA5 |
//! | Shield I2C1 (SCL, SDA): STMPE1600 U19 at 0x42 and U21 at 0x43, VL53L0X at 0x29 | PB8 (D15), PB9 (D14) |
//! | Shield centre VL53L0X GPIO1 (input only, not handed out) | PA4 (A2) |
//!
//! The I2C1 bus is the board's display bus, not DAGON's: I2C3's address
//! 0x42 is a different bus from U19's. PA4 is driven by the centre sensor,
//! so the package never makes it an output.
//!
//! [`Board`] implements `BoardDef`: DAGON takes the flash, the IWDG and the
//! I2C3 target; the app gets [`Io`]. An app that keeps the chip's
//! peripherals itself moves out only DAGON's with `dagon_pins!` and calls
//! `start`. This package binds the I2C1/I2C3 and DMA1 stream 0/2/4/6
//! interrupts itself.
#![no_std]

pub mod seg;
pub mod shield;

#[cfg(feature = "hal")]
mod display;
#[cfg(feature = "hal")]
pub use display::{running_version, Display, SegDisplay};

#[cfg(feature = "hal")]
pub use hal::*;

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
                i2c: $p.I2C3,
                scl: $p.PA8,
                sda: $p.PB4,
                tx_dma: $p.DMA1_CH4,
                rx_dma: $p.DMA1_CH2,
            },
        }
    };
}

/// The node's 7-bit I2C target address (the STM32 ROM loader keeps 0x39).
pub const I2C_ADDR: u8 = 0x42;
pub const BOARD_ID: &str = "nucleo-f401re";
pub const FLASH_SIZE: usize = 512 * 1024;

#[cfg(feature = "hal")]
mod hal {
    use dagon_mcu_stm32f401re::links::Kind;
    pub use dagon_mcu_stm32f401re::node::DagonParts;
    use dagon_mcu_stm32f401re::node::{I2cTarget, LogError};
    use dagon_mcu_stm32f401re::watchdog::Guard;
    pub use dagon_mcu_stm32f401re::BoardDef;
    use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
    use embassy_stm32::peripherals::{
        ADC1, DMA1_CH2, DMA1_CH4, I2C3, PA2, PA3, PA5, PA8, PB4, PC0, PC1, PC13, USART2,
    };
    use embassy_stm32::{bind_interrupts, dma, pac, peripherals, Peri, Peripherals};

    use super::{Display, BOARD_ID, FLASH_SIZE, I2C_ADDR};

    bind_interrupts!(pub(crate) struct Irqs {
        I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
        I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;
        DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
        DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
        I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
        I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
        DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
        DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
    });

    pub struct Board;

    /// Starts DAGON on this board (I2C target link; guard task
    /// `dagon_mcu_stm32f401re::node::DAGON_TASK` serves it): see
    /// `dagon_mcu_stm32f401re::node::start`.
    pub fn start<const N: usize>(
        spawner: embassy_executor::Spawner,
        parts: DagonParts<Link>,
        guard: &'static Guard<N>,
        tag: &'static str,
        self_check: fn() -> bool,
        uplink: Kind,
    ) -> Result<(), LogError> {
        dagon_mcu_stm32f401re::node::start::<Board, N>(
            spawner, parts, guard, tag, self_check, uplink,
        )
    }

    /// Up `i2c` on this board: I2C3 on PA8/PB4 as a target at
    /// [`I2C_ADDR`](super::I2C_ADDR) with the DAGON mailbox.
    pub fn uplink_i2c(link: Link) -> dagon_mcu_stm32f401re::links::I2c {
        let (target, regs) = Board::open_link(link);
        dagon_mcu_stm32f401re::links::I2c::new(target, regs)
    }

    /// The DAGON link's peripherals and pins.
    pub struct Link {
        pub i2c: Peri<'static, I2C3>,
        pub scl: Peri<'static, PA8>,
        pub sda: Peri<'static, PB4>,
        pub tx_dma: Peri<'static, DMA1_CH4>,
        pub rx_dma: Peri<'static, DMA1_CH2>,
    }

    /// What the app gets on this board.
    pub struct Io {
        pub led: Peri<'static, PA5>,
        /// The shield's 7-segment display (I2C1): [`Display::open`].
        pub display: Display,
        pub spare: Spare,
    }

    /// A few common peripherals DAGON and the display do not use: the
    /// ST-LINK virtual COM port (USART2 on PA2/PA3), the user button B1
    /// (PC13), ADC1 with A4/A5 (PC1/PC0; NC on the shield unless its U17
    /// is fitted, which routes the centre GPIO1 to PC1). Not here: I2C1
    /// with PB8/PB9 (the display's) and PA4 (A2, wired to the shield's
    /// centre VL53L0X GPIO1 output; left an input).
    pub struct Spare {
        pub usart2: Peri<'static, USART2>,
        pub pa2: Peri<'static, PA2>,
        pub pa3: Peri<'static, PA3>,
        pub pc13: Peri<'static, PC13>,
        pub adc1: Peri<'static, ADC1>,
        pub pc0: Peri<'static, PC0>,
        pub pc1: Peri<'static, PC1>,
    }

    impl BoardDef for Board {
        const BOARD_ID: &'static str = BOARD_ID;
        const FLASH_SIZE: usize = FLASH_SIZE;
        type Link = Link;
        type UserIo = Io;

        fn split(p: Peripherals) -> (DagonParts<Link>, Io) {
            let link = Link {
                i2c: p.I2C3,
                scl: p.PA8,
                sda: p.PB4,
                tx_dma: p.DMA1_CH4,
                rx_dma: p.DMA1_CH2,
            };
            let io = Io {
                led: p.PA5,
                display: Display {
                    i2c: p.I2C1,
                    scl: p.PB8,
                    sda: p.PB9,
                    tx_dma: p.DMA1_CH6,
                    rx_dma: p.DMA1_CH0,
                },
                spare: Spare {
                    usart2: p.USART2,
                    pa2: p.PA2,
                    pa3: p.PA3,
                    pc13: p.PC13,
                    adc1: p.ADC1,
                    pc0: p.PC0,
                    pc1: p.PC1,
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
