//! DAGON board package for the NUCLEO-F401RE (topology board id
//! `nucleo-f401re`): 512 KiB flash, LD2 on PA5, and DAGON's link to the hub
//! as an I2C target on I2C3. The 7-segment text ([`seg`]) is for the
//! 3-digit display of DAGON's demo node.
//!
//! | Use | Pins |
//! |---|---|
//! | DAGON link: I2C3 target (SCL, SDA), address [`I2C_ADDR`] | PA8 (D7), PB4 (D5) |
//! | User LED LD2 | PA5 |
//! | 7-segment a..g, dp (direct drive, high = on) | PA10, PB5, PA9, PC7, PB6, PA7, PA6, PA0 |
//! | 7-segment digit selects (low = on) | PA1, PA4, PB0 |
//!
//! The 7-segment board's part number and pin map are not confirmed yet:
//! the pins are Arduino header pins clear of I2C3, I2C2 (D6/D3), LD2 and
//! the ST-LINK UART. Adjust [`Board::split`] once the board is known.
//!
//! [`Board`] implements `BoardDef`: DAGON takes the flash, the IWDG and the
//! I2C3 target; the app gets [`Io`]. This package binds the I2C3 and DMA1
//! stream 2/4 interrupts itself.
#![no_std]

pub mod seg;

#[cfg(feature = "hal")]
pub use hal::*;

/// The node's 7-bit I2C target address (the STM32 ROM loader keeps 0x39).
pub const I2C_ADDR: u8 = 0x42;
pub const BOARD_ID: &str = "nucleo-f401re";
pub const FLASH_SIZE: usize = 512 * 1024;

#[cfg(feature = "hal")]
mod hal {
    use dagon_mcu_stm32f401re::node::{DagonParts, I2cTarget};
    pub use dagon_mcu_stm32f401re::BoardDef;
    use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
    use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
    use embassy_stm32::peripherals::{DMA1_CH2, DMA1_CH4, I2C3, PA5, PA8, PB4};
    use embassy_stm32::{bind_interrupts, dma, pac, peripherals, Peri, Peripherals};

    use super::{seg, BOARD_ID, FLASH_SIZE, I2C_ADDR};

    bind_interrupts!(struct Irqs {
        I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
        I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;
        DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
        DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
    });

    pub const SEG_ON: Level = Level::High;
    pub const SEG_OFF: Level = Level::Low;
    pub const DIGIT_ON: Level = Level::Low;
    pub const DIGIT_OFF: Level = Level::High;

    pub struct Board;

    /// The DAGON link's peripherals and pins.
    pub struct Link {
        i2c: Peri<'static, I2C3>,
        scl: Peri<'static, PA8>,
        sda: Peri<'static, PB4>,
        tx_dma: Peri<'static, DMA1_CH4>,
        rx_dma: Peri<'static, DMA1_CH2>,
    }

    /// What the app gets on this board.
    pub struct Io {
        pub led: Peri<'static, PA5>,
        pub display: SegDisplay,
    }

    /// The 7-segment display's pins: segments a..g and dp, digit selects.
    pub struct SegDisplay {
        pub segments: [Peri<'static, AnyPin>; 8],
        pub digits: [Peri<'static, AnyPin>; seg::DIGITS],
    }

    impl SegDisplay {
        /// The pins as outputs, everything off.
        #[allow(clippy::type_complexity)]
        pub fn outputs(self) -> ([Output<'static>; 8], [Output<'static>; seg::DIGITS]) {
            (
                self.segments.map(|p| Output::new(p, SEG_OFF, Speed::Low)),
                self.digits.map(|p| Output::new(p, DIGIT_OFF, Speed::Low)),
            )
        }
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
                display: SegDisplay {
                    segments: [
                        p.PA10.into(), // a  D2
                        p.PB5.into(),  // b  D4
                        p.PA9.into(),  // c  D8
                        p.PC7.into(),  // d  D9
                        p.PB6.into(),  // e  D10
                        p.PA7.into(),  // f  D11
                        p.PA6.into(),  // g  D12
                        p.PA0.into(),  // dp A0
                    ],
                    digits: [p.PA1.into(), p.PA4.into(), p.PB0.into()],
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
