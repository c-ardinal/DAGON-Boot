//! Your board, for DAGON: the I2C target that links it to the 親機, and
//! which peripherals the app gets. Written once by `dagon new` with the
//! link of this node in the 構成 (its controller and pins); edit it for
//! your board (dagon gen never rewrites it). The flash size is the one
//! given for this board in dagon.toml, else the least the MCU's layout
//! needs.

use dagon_mcu_stm32f401re::layout::MIN_FLASH_SIZE;
use dagon_mcu_stm32f401re::node::{BoardDef, DagonParts, I2cTarget};
use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig};
use embassy_stm32::peripherals::{DMA1_CH2, DMA1_CH4, {{link_i2c}}, {{link_scl}}, {{link_sda}}};
use embassy_stm32::{bind_interrupts, dma, pac, peripherals, Peri, Peripherals};

use crate::wiring;

/// The node's 7-bit I2C address on the 親機's bus (from the 構成).
const I2C_ADDR: u8 = match wiring::NODE.link {
    wiring::Link::I2c { addr, .. } => addr,
    _ => panic!("this start-up serves an I2C link to the 親機"),
};

bind_interrupts!(struct Irqs {
    {{link_i2c}}_EV => i2c::EventInterruptHandler<peripherals::{{link_i2c}}>;
    {{link_i2c}}_ER => i2c::ErrorInterruptHandler<peripherals::{{link_i2c}}>;
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
});

pub struct Board;

/// The link to the 親機: {{link_i2c}} on {{link_scl}} (SCL) and {{link_sda}} (SDA) with DMA
/// (DMA1 streams 4 and 2 serve I2C3; another controller takes its own
/// streams, see the reference manual).
pub struct Link {
    i2c: Peri<'static, {{link_i2c}}>,
    scl: Peri<'static, {{link_scl}}>,
    sda: Peri<'static, {{link_sda}}>,
    tx_dma: Peri<'static, DMA1_CH4>,
    rx_dma: Peri<'static, DMA1_CH2>,
}

/// What the app gets. Add the pins and peripherals your app uses (move
/// them out of `p` in `split`).
pub struct Io {}

impl BoardDef for Board {
    const BOARD_ID: &'static str = "{{node}}";
    const FLASH_SIZE: usize = match wiring::NODE.flash_bytes {
        Some(bytes) => bytes as usize,
        None => MIN_FLASH_SIZE,
    };
    type Link = Link;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts<Link>, Io) {
        let link = Link {
            i2c: p.{{link_i2c}},
            scl: p.{{link_scl}},
            sda: p.{{link_sda}},
            tx_dma: p.DMA1_CH4,
            rx_dma: p.DMA1_CH2,
        };
        let dagon = DagonParts {
            flash: p.FLASH,
            iwdg: p.IWDG,
            link,
        };
        (dagon, Io {})
    }

    fn open_link(l: Link) -> (I2cTarget, pac::i2c::I2c) {
        let mut cfg = i2c::Config::default();
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        let target = I2c::new(l.i2c, l.scl, l.sda, l.tx_dma, l.rx_dma, Irqs, cfg)
            .into_slave_multimaster(SlaveAddrConfig::basic(I2C_ADDR));
        (target, pac::{{link_i2c}})
    }
}
