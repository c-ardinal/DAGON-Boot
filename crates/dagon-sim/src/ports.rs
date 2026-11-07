//! Hub-side port traits over the simulated bus, so hub logic written against
//! `dagon-port` runs unchanged on the simulator. All futures complete at once;
//! drive them with [`crate::block_on`].

use crate::hub::SimBus;
use dagon_port::{BusError, I2cBus, Nrst, StreamPort, SubControl};

impl I2cBus for SimBus {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
        self.i2c_write(addr, data)
    }

    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        self.i2c_read(addr, buf)
    }

    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
        self.i2c_write(addr, data)?;
        self.i2c_read(addr, buf)
    }
}

/// The UART node's byte stream (PIO UART on the hub).
impl StreamPort for SimBus {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        if self.uart_pending() == 0 {
            self.advance_ms(timeout_ms as u64);
            return Err(BusError::Timeout);
        }
        Ok(self.uart_take(buf))
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        self.uart_write(data);
        Ok(())
    }
}

/// NRST and BOOT0 of the wired I2C node.
impl SubControl for SimBus {
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        SimBus::set_lines(self, nrst, boot0_high)
    }
}
