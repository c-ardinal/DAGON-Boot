//! Port traits over the simulated ports, so relay logic written against
//! `dagon-port` runs unchanged on the simulator. All futures complete at
//! once; drive them with [`crate::block_on`].

use crate::relay::{SimBus, SimPort};
use dagon_port::{BusError, I2cBus, Nrst, StreamPort, SubControl};

/// An I2C port.
impl I2cBus for SimPort<'_> {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
        self.bus.i2c_write(self.port, addr, data)
    }

    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        self.bus.i2c_read(self.port, addr, buf)
    }

    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
        self.bus.i2c_write(self.port, addr, data)?;
        self.bus.i2c_read(self.port, addr, buf)
    }
}

/// A UART port's byte stream.
impl StreamPort for SimPort<'_> {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        if self.bus.uart_pending(self.port) == 0 {
            self.bus.advance_ms(timeout_ms as u64);
            return Err(BusError::Timeout);
        }
        Ok(self.bus.uart_take(self.port, buf))
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        self.bus.uart_write(self.port, data);
        Ok(())
    }
}

/// NRST and BOOT0 of the wired I2C node.
impl SubControl for SimBus {
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        SimBus::set_lines(self, nrst, boot0_high)
    }
}
