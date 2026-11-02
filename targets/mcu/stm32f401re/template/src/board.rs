//! Your board, for DAGON: its flash size, the I2C target that links it to
//! the hub, and which peripherals the app gets. Written once by `dagon new`
//! with NUCLEO-F401RE-style wiring; edit it for your board (dagon gen never
//! rewrites it).

use dagon_mcu_stm32f401re::node::{BoardDef, DagonParts, I2cTarget};
use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
use embassy_stm32::peripherals::{DMA1_CH2, DMA1_CH4, I2C3, PA5, PA8, PB4};
use embassy_stm32::{bind_interrupts, dma, pac, peripherals, Peri, Peripherals};

/// The node's 7-bit I2C address on the hub's bus (as in the topology).
const I2C_ADDR: u8 = 0x42;

bind_interrupts!(struct Irqs {
    I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
    I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
});

pub struct Board;

/// The link to the hub: I2C3 on PA8 (SCL) and PB4 (SDA) with DMA.
pub struct Link {
    i2c: Peri<'static, I2C3>,
    scl: Peri<'static, PA8>,
    sda: Peri<'static, PB4>,
    tx_dma: Peri<'static, DMA1_CH4>,
    rx_dma: Peri<'static, DMA1_CH2>,
}

/// What the app gets. Add the pins and peripherals your app uses.
pub struct Io {
    pub led: Peri<'static, PA5>,
}

impl BoardDef for Board {
    const BOARD_ID: &'static str = "{{node}}";
    /// Your board's flash; DAGON's layout needs at least 512 KiB.
    const FLASH_SIZE: usize = 512 * 1024;
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
        let dagon = DagonParts {
            flash: p.FLASH,
            iwdg: p.IWDG,
            link,
        };
        (dagon, Io { led: p.PA5 })
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
