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
//! I2C3 target; the app gets [`Io`]. An app that keeps the chip's
//! peripherals itself moves out only DAGON's with `dagon_pins!` and calls
//! `start`. This package binds the I2C3 and DMA1 stream 2/4 interrupts
//! itself.
#![no_std]

pub mod seg;

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
    pub use dagon_mcu_stm32f401re::node::DagonParts;
    use dagon_mcu_stm32f401re::node::{I2cTarget, LogError};
    use dagon_mcu_stm32f401re::watchdog::Guard;
    pub use dagon_mcu_stm32f401re::BoardDef;
    use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
    use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
    use embassy_stm32::peripherals::{
        ADC1, DMA1_CH2, DMA1_CH4, I2C1, I2C3, PA2, PA3, PA5, PA8, PB4, PB8, PB9, PC0, PC1, PC13,
        USART2,
    };
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

    /// Starts DAGON on this board (I2C target link; guard task
    /// `dagon_mcu_stm32f401re::node::DAGON_TASK` serves it): see
    /// `dagon_mcu_stm32f401re::node::start`.
    pub fn start<const N: usize>(
        spawner: embassy_executor::Spawner,
        parts: DagonParts<Link>,
        guard: &'static Guard<N>,
        tag: &'static str,
        self_check: fn() -> bool,
    ) -> Result<(), LogError> {
        dagon_mcu_stm32f401re::node::start::<Board, N>(spawner, parts, guard, tag, self_check)
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
        pub display: SegDisplay,
        pub spare: Spare,
    }

    /// A few common peripherals DAGON does not use: the ST-LINK virtual
    /// COM port (USART2 on PA2/PA3), I2C1 on the Arduino D15/D14 pins
    /// (PB8/PB9), the user button B1 (PC13), ADC1 with A4/A5 (PC1/PC0).
    pub struct Spare {
        pub usart2: Peri<'static, USART2>,
        pub pa2: Peri<'static, PA2>,
        pub pa3: Peri<'static, PA3>,
        pub i2c1: Peri<'static, I2C1>,
        pub pb8: Peri<'static, PB8>,
        pub pb9: Peri<'static, PB9>,
        pub pc13: Peri<'static, PC13>,
        pub adc1: Peri<'static, ADC1>,
        pub pc0: Peri<'static, PC0>,
        pub pc1: Peri<'static, PC1>,
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
                spare: Spare {
                    usart2: p.USART2,
                    pa2: p.PA2,
                    pa3: p.PA3,
                    i2c1: p.I2C1,
                    pb8: p.PB8,
                    pb9: p.PB9,
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
