//! Control lines of a sub MCU (`dagon_port::SubControl`): NRST is only
//! pulled low or released (Hi-Z), never driven high; BOOT0 low boots from
//! flash. Starts released / low. The board package names the pins.

use dagon_port::{BusError, Nrst, SubControl};
use embassy_rp::gpio::{Flex, Level, Output, Pin};
use embassy_rp::Peri;
use embassy_time::{Duration, Timer};

const PULSE: Duration = Duration::from_millis(10);

pub struct SubLines<'d> {
    nrst: Flex<'d>,
    boot0: Output<'d>,
    /// The GPIO numbers of NRST and BOOT0.
    pins: (u8, u8),
}

impl<'d> SubLines<'d> {
    pub fn new(nrst: Peri<'d, impl Pin>, boot0: Peri<'d, impl Pin>) -> Self {
        let pins = (nrst.pin(), boot0.pin());
        let mut nrst = Flex::new(nrst);
        nrst.set_low();
        nrst.set_as_input();
        SubLines {
            nrst,
            boot0: Output::new(boot0, Level::Low),
            pins,
        }
    }

    /// The GPIO numbers of NRST and BOOT0.
    pub fn pins(&self) -> (u8, u8) {
        self.pins
    }

    /// BOOT0 as last set (I2C_RAW is only allowed while it is high).
    pub fn boot0_high(&self) -> bool {
        self.boot0.is_set_high()
    }
}

impl SubControl for SubLines<'_> {
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        self.boot0.set_level(Level::from(boot0_high));
        match nrst {
            Nrst::Release => self.nrst.set_as_input(),
            Nrst::Low => self.nrst.set_as_output(),
            Nrst::Pulse => {
                self.nrst.set_as_output();
                Timer::after(PULSE).await;
                self.nrst.set_as_input();
            }
        }
        Ok(())
    }
}
