//! F401 control lines. NRST is only pulled low or released (Hi-Z), never
//! driven high; BOOT0 low boots from flash. Starts released / low.

use dagon_port::{BusError, Nrst, SubControl};
use embassy_rp::gpio::{Flex, Level, Output};
use embassy_rp::peripherals::{PIN_18, PIN_28};
use embassy_rp::Peri;
use embassy_time::{Duration, Timer};

const PULSE: Duration = Duration::from_millis(10);

pub struct SubLines<'d> {
    nrst: Flex<'d>,
    boot0: Output<'d>,
}

impl<'d> SubLines<'d> {
    pub fn new(nrst: Peri<'d, PIN_18>, boot0: Peri<'d, PIN_28>) -> Self {
        let mut nrst = Flex::new(nrst);
        nrst.set_low();
        nrst.set_as_input();
        SubLines {
            nrst,
            boot0: Output::new(boot0, Level::Low),
        }
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
