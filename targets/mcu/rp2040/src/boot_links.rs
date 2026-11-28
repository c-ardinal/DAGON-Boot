//! The links a boot core receives on besides USB, interrupt-driven (the
//! boot core sleeps between their events):
//! - [`I2cTarget`]: an I2C target at the node's address running the DAGON
//!   mailbox. It NACKs its address from a request's arrival until its
//!   reply is held (the flash work in between never stretches the clock),
//!   then answers the state reads (`BUSY`, then `READY` and the length) and
//!   the read of the reply.
//! - [`UartTarget`]: a UART (hardware or PIO); a partial frame is dropped when the
//!   next byte comes `frames::GAP_MS` or more after it.
//!
//! The boot core binds their interrupts and builds the HAL drivers from
//! the board's DAGON buses.

use dagon_mcu_common::frames::FrameReader;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_port::{BusError, StreamPort};
use dagon_proto::mailbox::State;
use dagon_proto::{frame_len, max_payload};
use embassy_rp::i2c::Instance;
use embassy_rp::i2c_slave::{Command, I2cSlave};
use embassy_rp::uart::BufferedUart;
use embassy_time::{with_timeout, Duration, Instant, Timer};

const I2C_FRAME: usize = frame_len(max_payload::I2C);
const UART_FRAME: usize = frame_len(max_payload::UART);
/// How long a held reply waits to be read before a REBOOT is given up.
const DELIVER_TIMEOUT: Duration = Duration::from_millis(500);
/// Bound on sending one frame on the UART.
const SEND_TIMEOUT: Duration = Duration::from_millis(500);
/// What a read past the end of a reply gets.
const FILL: u8 = 0xFF;

/// Where the reply to the last request is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    /// None held: state reads answer BUSY.
    None,
    /// Held, its state not read yet.
    Ready,
    /// Its state was read: the next read takes the reply.
    StateRead,
}

/// The DAGON mailbox as an I2C target ([`I2cSlave`] at the node's address).
pub struct I2cTarget<'d, T: Instance> {
    bus: I2cSlave<'d, T>,
    regs: embassy_rp::pac::i2c::I2c,
    addr: u8,
    req: [u8; I2C_FRAME],
    reply: [u8; I2C_FRAME],
    reply_len: usize,
    held: Held,
    /// A read was being answered when the wait for it was cut (the link's
    /// read is bounded by a timer): the block is reset before the next
    /// wait, so the bus is not left stretched.
    responding: bool,
}

impl<'d, T: Instance> I2cTarget<'d, T> {
    /// The target `bus` at `addr`; `regs` its block (`pac::I2C0` or
    /// `pac::I2C1`), to NACK the address while a request is handled.
    pub fn new(bus: I2cSlave<'d, T>, regs: embassy_rp::pac::i2c::I2c, addr: u8) -> Self {
        I2cTarget {
            bus,
            regs,
            addr,
            req: [0; I2C_FRAME],
            reply: [0; I2C_FRAME],
            reply_len: 0,
            held: Held::None,
            responding: false,
        }
    }

    fn answer(&mut self, on: bool) {
        if core::mem::take(&mut self.responding) {
            self.bus.reset();
        }
        self.regs.ic_enable().write(|w| w.set_enable(on));
    }

    async fn respond(&mut self, data: &[u8]) {
        self.responding = true;
        let _ = self.bus.respond_and_fill(data, FILL).await;
        self.responding = false;
    }

    /// Answers one controller read; `true` once the reply was read.
    async fn serve_read(&mut self) -> bool {
        let state = match self.held {
            Held::StateRead => {
                let reply = self.reply;
                self.respond(&reply[..self.reply_len]).await;
                self.held = Held::None;
                return true;
            }
            Held::Ready => {
                self.held = Held::StateRead;
                State::Ready(self.reply_len as u16)
            }
            Held::None => State::Busy,
        };
        self.respond(&state.to_bytes()).await;
        false
    }
}

impl<T: Instance> Link for I2cTarget<'_, T> {
    fn max_payload(&self) -> usize {
        max_payload::I2C
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }
    fn kind(&self) -> Kind {
        Kind::I2c
    }
    fn addr(&self) -> u8 {
        self.addr
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        self.answer(true);
        loop {
            match self.bus.listen(&mut self.req).await {
                Ok(Command::Write(n)) if n > 0 => {
                    self.held = Held::None;
                    self.answer(false);
                    return Ok(&self.req[..n]);
                }
                Ok(Command::Read | Command::WriteRead(_)) => {
                    self.serve_read().await;
                }
                Ok(_) => {}
                // A broken transfer: the controller resends.
                Err(_) => self.bus.reset(),
            }
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        let n = frame.len();
        if n > I2C_FRAME {
            return Err(LinkError::Overflow);
        }
        self.reply[..n].copy_from_slice(frame);
        self.reply_len = n;
        self.held = Held::Ready;
        self.answer(true);
        Ok(())
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.held = Held::None;
        self.answer(true);
        Ok(())
    }
    /// Returns once the controller has read the held reply.
    async fn flush(&mut self) -> Result<(), LinkError> {
        self.answer(true);
        let read = async {
            loop {
                if let Ok(Command::Read | Command::WriteRead(_)) =
                    self.bus.listen(&mut self.req).await
                {
                    if self.serve_read().await {
                        return;
                    }
                }
            }
        };
        with_timeout(DELIVER_TIMEOUT, read)
            .await
            .map_err(|_| LinkError::Timeout)
    }
}

/// A UART carrying DAGON frames to the node, over any byte stream: the
/// board's DAGON UART bus on a hardware UART ([`HwUart`]) or a PIO
/// (`crate::links::PioStream`).
pub struct UartTarget<S> {
    port: S,
    rx: FrameReader<{ UART_FRAME * 2 }>,
    last: usize,
}

impl<S: StreamPort> UartTarget<S> {
    pub fn new(port: S) -> Self {
        UartTarget {
            port,
            rx: FrameReader::new(max_payload::UART),
            last: 0,
        }
    }
}

impl<S: StreamPort> Link for UartTarget<S> {
    fn max_payload(&self) -> usize {
        max_payload::UART
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }
    fn kind(&self) -> Kind {
        Kind::Uart
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        if self.last > 0 {
            self.rx.consume(self.last);
            self.last = 0;
        }
        loop {
            if let Some(n) = self.rx.poll() {
                self.last = n;
                return Ok(self.rx.frame(n));
            }
            // Bytes after a gap drop the partial frame before them, so the
            // read waits for the next byte without a deadline of its own.
            let mut b = [0u8; 1];
            match self.port.read(&mut b, u32::MAX).await {
                Ok(n) => {
                    self.rx.push_at(&b[..n], Instant::now().as_millis());
                }
                // Line error: the partial frame is lost.
                Err(_) => self.rx.clear(),
            }
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        match with_timeout(SEND_TIMEOUT, self.port.write(frame)).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(LinkError::Closed),
        }
    }
    /// Returns once the reply has left the line (a REBOOT's reset comes
    /// right after it).
    async fn flush(&mut self) -> Result<(), LinkError> {
        match with_timeout(DRAIN_TIMEOUT, self.port.drain()).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(LinkError::Closed),
        }
    }
}

/// Bound on a UART's last bytes leaving the line: the 32-byte FIFO at 9600
/// bit/s takes 34 ms.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

/// A hardware UART as a byte stream.
pub struct HwUart(pub BufferedUart);

impl StreamPort for HwUart {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        use embedded_io_async::Read;
        let wait = Duration::from_millis(timeout_ms.into());
        match with_timeout(wait, self.0.read(buf)).await {
            Ok(Ok(n)) => Ok(n),
            Ok(Err(_)) => Err(BusError::Io),
            Err(_) => Err(BusError::Timeout),
        }
    }
    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        use embedded_io_async::Write;
        self.0.write_all(data).await.map_err(|_| BusError::Io)?;
        self.0.flush().await.map_err(|_| BusError::Io)
    }
    /// The write leaves the bytes in the UART's FIFO: waits until the UART
    /// no longer sends (its BUSY flag, FIFO and shift register).
    async fn drain(&mut self) -> Result<(), BusError> {
        while self.0.busy() {
            Timer::after_micros(50).await;
        }
        Ok(())
    }
}

/// Stops both I2C blocks answering their address (the boot core's target
/// takes it up again when it next reads), so no controller is held by a
/// stretched clock while the boot core works without reading.
pub fn pause_i2c() {
    for regs in [embassy_rp::pac::I2C0, embassy_rp::pac::I2C1] {
        regs.ic_enable().write(|w| w.set_enable(false));
    }
}

/// Puts the blocks the links ran on back to their reset state before the
/// app starts, so none of them is still serving the bus.
pub fn release() {
    embassy_rp::pac::RESETS.reset().modify(|w| {
        w.set_i2c0(true);
        w.set_i2c1(true);
        w.set_uart0(true);
        w.set_uart1(true);
        w.set_pio0(true);
        w.set_pio1(true);
    });
}
