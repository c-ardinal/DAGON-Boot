//! UART0, the link the ROM leaves set up (its pins and bit rate), polled:
//! the boot core's `Port` for the shared update mode. Bytes are cut into
//! frames as the node's UART link cuts them (`framer`, a partial frame
//! dropped after 20 ms without a byte); a reply goes out whole before the
//! next request is read. Registers as the ESP32-S3 technical reference
//! gives them (UART_FIFO_REG, UART_STATUS_REG).

use crate::clock::Cycles;
use crate::rom::{reg_read, reg_write};
use dagon_agent::update_mode::Port;
use dagon_events::Clock;
use dagon_mcu_esp32s3::framer::{FrameCutter, MAX_UART_FRAME};

const UART0: u32 = 0x6000_0000;
const FIFO: u32 = UART0;
const STATUS: u32 = UART0 + 0x1C;
const RX_COUNT: u32 = 0x3FF;
const TX_COUNT_SHIFT: u32 = 16;
const TX_FIFO: u32 = 128;

fn rx_count() -> u32 {
    reg_read(STATUS) & RX_COUNT
}

fn tx_count() -> u32 {
    (reg_read(STATUS) >> TX_COUNT_SHIFT) & RX_COUNT
}

pub struct Uart0 {
    cut: FrameCutter,
    req: [u8; MAX_UART_FRAME],
    clock: Cycles,
    delivered: bool,
}

impl Uart0 {
    pub const fn new(clock: Cycles) -> Self {
        Uart0 {
            cut: FrameCutter::new(),
            req: [0; MAX_UART_FRAME],
            clock,
            delivered: false,
        }
    }

    fn send(&mut self, frame: &[u8]) {
        for &b in frame {
            while tx_count() >= TX_FIFO - 1 {}
            reg_write(FIFO, u32::from(b));
        }
    }

    fn drained(&self) -> bool {
        tx_count() == 0
    }
}

impl Port for Uart0 {
    fn poll(&mut self) -> Option<(usize, &[u8])> {
        while rx_count() > 0 {
            let b = reg_read(FIFO) as u8;
            let ms = (self.clock.now_us() / 1000) as u32;
            if let Some(f) = self.cut.push_at(b, ms) {
                let n = f.len();
                self.req[..n].copy_from_slice(f);
                return Some((0, &self.req[..n]));
            }
        }
        None
    }

    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool) {
        self.delivered = false;
        if let Some(f) = frame {
            self.send(f);
            self.delivered = reboot;
        }
    }

    /// The reply to REBOOT has left the FIFO.
    fn delivered(&mut self) -> bool {
        self.delivered && self.drained()
    }

    fn wait(&mut self, max_us: u32) {
        let end = self.clock.now_us() + u64::from(max_us);
        while rx_count() == 0 && !(self.delivered && self.drained()) && self.clock.now_us() < end {}
    }
}
