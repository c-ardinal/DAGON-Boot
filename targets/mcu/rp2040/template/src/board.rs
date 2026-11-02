//! Your board, for DAGON: its flash size and which peripherals the app
//! gets. Written once by `dagon new`; edit it for your board (dagon gen
//! never rewrites it).

use dagon_mcu_rp2040::node::{BoardDef, DagonParts};
use embassy_rp::peripherals::PIN_25;
use embassy_rp::{Peri, Peripherals};

pub struct Board;

/// What the app gets. Add the pins and peripherals your app uses.
pub struct Io {
    /// GPIO25 (the LED on Raspberry Pi Pico-style boards).
    pub led: Peri<'static, PIN_25>,
}

impl BoardDef for Board {
    const BOARD_ID: &'static str = "{{node}}";
    /// Your board's flash; DAGON's layout needs at least 4 MiB.
    const FLASH_SIZE: usize = 4 * 1024 * 1024;
    /// An LED for DAGON's diagnostic blink codes, if any.
    const DIAG_LED: Option<u8> = None;
    type UserIo = Io;

    fn split(p: Peripherals) -> (DagonParts, Io) {
        let dagon = DagonParts {
            flash: p.FLASH,
            watchdog: p.WATCHDOG,
            usb: p.USB,
        };
        (dagon, Io { led: p.PIN_25 })
    }
}
