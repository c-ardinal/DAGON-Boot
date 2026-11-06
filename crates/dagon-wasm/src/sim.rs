//! The simulator (dagon-sim) as the page's byte link: the sample hub
//! with the F401 on I2C (NRST/BOOT0 wired, ROM loader simulated) and the
//! watch on the UART, each on 1.0.0. Tests and `?sim` in development drive
//! dagon-host through the same `HostLink` as Web Serial, on the simulator's
//! clock. Answers arrive when the links and flash would have carried them
//! (dagon-sim's `TimedHub`, the model `dagon demo` uses too). Built with the
//! `sim` feature only.

use dagon_sim::{sample_hub, TimedHub, TrialApp};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Simulator(TimedHub);

#[wasm_bindgen]
impl Simulator {
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32) -> Simulator {
        Simulator(TimedHub::new(sample_hub(seed as u64)))
    }

    /// Bytes from the page (USB CDC).
    pub fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    /// Bytes that have arrived for the page; empty when none has yet.
    pub fn read(&mut self) -> Vec<u8> {
        self.0.read()
    }

    /// When the next answer arrives on the simulator's clock; -1 when none is on its way.
    #[wasm_bindgen(js_name = nextDue)]
    pub fn next_due(&self) -> f64 {
        self.0.next_due().map_or(-1.0, |t| t as f64)
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
        let Some(node) = self.0.hub_mut().i2c_node(addr) else {
            return false;
        };
        if node.flash().erase_all().is_err() {
            return false;
        }
        node.reset(now);
        true
    }

    /// The watch's next version crashes at boot, so its write goes back
    /// (what `dagon demo --sim-fail watch` sets up).
    #[wasm_bindgen(js_name = failWatch)]
    pub fn fail_watch(&mut self) -> bool {
        let Some((_, watch)) = self.0.hub_mut().bus.uart.as_mut() else {
            return false;
        };
        watch.trial_app = TrialApp::CrashesAtBoot;
        true
    }
}
