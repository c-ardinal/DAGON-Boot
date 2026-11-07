//! Your board, for DAGON: the 親機's links to the 子機. Written once by
//! `dagon new` with this node's links in the 構成 (an I2C bus and a PIO
//! UART, on the pins the 構成 gives); edit it for your board (dagon gen
//! never rewrites it). The flash size is the one given for this board in
//! dagon.toml, else the least the MCU's layout needs.

use dagon_mcu_rp2040::layout::MIN_FLASH_SIZE;
use dagon_mcu_rp2040::links::Kind;
#[cfg({{if_down_i2c}})]
use dagon_mcu_rp2040::links::{I2cLink, RpI2c};
#[cfg({{if_down_uart}})]
use dagon_mcu_rp2040::links::PioStream;
use dagon_mcu_rp2040::node::{BoardDef, DagonParts};
use dagon_mcu_rp2040::relay::Links;
#[cfg({{if_down_i2c}})]
use embassy_rp::i2c::{self, I2c};
#[cfg({{if_down_i2c}})]
use embassy_rp::peripherals::{{{down_i2c}}, PIN_{{down_i2c_scl}}, PIN_{{down_i2c_sda}}};
#[cfg({{if_down_uart}})]
use embassy_rp::peripherals::{PIN_{{down_uart_rx}}, PIN_{{down_uart_tx}}, PIO0};
#[cfg({{if_down_uart}})]
use embassy_rp::pio::{self, Pio};
#[cfg({{if_down_uart}})]
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::{bind_interrupts, Peri, Peripherals};

use crate::wiring;

#[cfg({{if_down_i2c}})]
bind_interrupts!(struct I2cIrqs {
    {{down_i2c}}_IRQ => i2c::InterruptHandler<{{down_i2c}}>;
});
#[cfg({{if_down_uart}})]
bind_interrupts!(struct PioIrqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

/// I2C speed to the 子機 (from the 構成).
#[cfg({{if_down_i2c}})]
const I2C_HZ: u32 = {{down_i2c_hz}};
/// UART speed to the 子機 (from the 構成).
#[cfg({{if_down_uart}})]
const UART_BAUD: u32 = {{down_uart_baud}};

pub struct Board;

/// What DAGON's relay and your app get besides DAGON's own peripherals.
pub struct Io {
    #[cfg({{if_down_i2c}})]
    pub i2c: (Peri<'static, {{down_i2c}}>, Peri<'static, PIN_{{down_i2c_sda}}>, Peri<'static, PIN_{{down_i2c_scl}}>),
    #[cfg({{if_down_uart}})]
    pub uart: (Peri<'static, PIO0>, Peri<'static, PIN_{{down_uart_tx}}>, Peri<'static, PIN_{{down_uart_rx}}>),
    pub spare: Spare,
}

/// What your app gets. Add the pins and peripherals your app uses (move
/// them out of `p` in `split`).
pub struct Spare {}

impl BoardDef for Board {
    const BOARD_ID: &'static str = "{{node}}";
    const FLASH_SIZE: usize = match wiring::NODE.flash_bytes {
        Some(bytes) => bytes as usize,
        None => MIN_FLASH_SIZE,
    };
    /// An LED for DAGON's diagnostic blink codes, if any.
    const DIAG_LED: Option<u8> = None;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        let io = Io {
            #[cfg({{if_down_i2c}})]
            i2c: (p.{{down_i2c}}, p.PIN_{{down_i2c_sda}}, p.PIN_{{down_i2c_scl}}),
            #[cfg({{if_down_uart}})]
            uart: (p.PIO0, p.PIN_{{down_uart_tx}}, p.PIN_{{down_uart_rx}}),
            spare: Spare {},
        };
        (dagon, io)
    }
}

/// The relay's links for the 構成's hops (the uplink is the USB CDC
/// device, built by the relay; no STM32 recovery lines on this wiring),
/// and what is left for the app.
pub fn links(io: Io, uplink: Kind, downlinks: &[Kind]) -> (Links, Spare) {
    assert!(uplink == Kind::UsbCdc, "uplink: only usb-cdc on this board");
    let _ = downlinks;
    #[cfg({{if_down_i2c}})]
    let i2c = downlinks.contains(&Kind::I2c).then(|| {
        let (bus, sda, scl) = io.i2c;
        let mut cfg = i2c::Config::default();
        cfg.frequency = I2C_HZ;
        I2cLink::new(RpI2c(I2c::new_async(bus, scl, sda, I2cIrqs, cfg)))
    });
    #[cfg(not({{if_down_i2c}}))]
    let i2c = None;
    #[cfg({{if_down_uart}})]
    let uart = downlinks.contains(&Kind::Uart).then(|| {
        let (pio, tx, rx) = io.uart;
        let Pio {
            mut common,
            sm0,
            sm1,
            ..
        } = Pio::new(pio, PioIrqs);
        let tx_prog = PioUartTxProgram::new(&mut common);
        let rx_prog = PioUartRxProgram::new(&mut common);
        let tx = PioUartTx::new(UART_BAUD, &mut common, sm0, tx, &tx_prog);
        let rx = PioUartRx::new(UART_BAUD, &mut common, sm1, rx, &rx_prog);
        // The UART runs for the life of the app: never release the PIO.
        core::mem::forget(common);
        PioStream { tx, rx }
    });
    #[cfg(not({{if_down_uart}}))]
    let uart = None;
    let links = Links {
        i2c,
        uart,
        sub: None,
    };
    (links, io.spare)
}
