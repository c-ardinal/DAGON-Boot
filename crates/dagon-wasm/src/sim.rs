//! The simulator (dagon-sim) as the page's byte link: the sample hub
//! with the F401 on I2C (NRST/BOOT0 wired, ROM loader simulated) and the
//! watch on the UART, each on 1.0.0. Tests and `?sim` in development drive
//! dagon-host through the same `HostLink` as Web Serial, on the simulator's
//! clock. Built with the `sim` feature only.

use dagon_sim::{sample_hub, SimHub};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Simulator(SimHub);

#[wasm_bindgen]
impl Simulator {
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32) -> Simulator {
        Simulator(sample_hub(seed as u64))
    }

    /// Bytes from the page (USB CDC).
    pub fn write(&mut self, bytes: &[u8]) {
        self.0.usb_write(bytes);
    }

    /// Bytes for the page; empty when nothing is waiting.
    pub fn read(&mut self) -> Vec<u8> {
        self.0.usb_read()
    }

    /// The simulator's clock.
    pub fn now(&self) -> f64 {
        self.0.now_ms() as f64
    }

    pub fn advance(&mut self, ms: u32) {
        self.0.advance_ms(ms as u64);
    }

    /// Erases the whole flash of the I2C node at `addr` and resets it: it
    /// answers nothing any more, only its ROM loader is left.
    pub fn wipe(&mut self, addr: u8) -> bool {
        let now = self.0.now_ms();
        let Some(node) = self.0.i2c_node(addr) else {
            return false;
        };
        if node.flash().erase_all().is_err() {
            return false;
        }
        node.reset(now);
        true
    }
}
