//! The DAGON UART link: its settings, and a polling `Link` for simple
//! blocking use. `recv_frame` polls the RX FIFO, so it completes without a
//! real await point and runs under any executor (or `block_on`). The node's
//! own link task (`node`) uses the async driver instead.

use dagon_port::{Link, LinkError, LinkKind};
use esp_hal::{
    time::{Duration, Instant},
    uart::{Config as UartConfig, Uart},
    Blocking,
};

/// Link speed: 115200 8N1 on every DAGON UART link.
pub const LINK_BAUD: u32 = 115_200;

/// UART settings for the DAGON link; the board crate applies them to the
/// UART on its link pins.
pub fn uart_config() -> UartConfig {
    UartConfig::default().with_baudrate(LINK_BAUD)
}

use crate::framer::{FrameCutter, MAX_UART_FRAME};

pub struct UartLink<'d> {
    uart: Uart<'d, Blocking>,
    cutter: FrameCutter,
}

impl<'d> UartLink<'d> {
    /// `uart` must already be configured with [`uart_config`].
    pub fn new(uart: Uart<'d, Blocking>) -> Self {
        Self {
            uart,
            cutter: FrameCutter::new(),
        }
    }
}

impl Link for UartLink<'_> {
    async fn recv_frame(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, LinkError> {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms.into());
        let mut byte = [0u8; 1];
        loop {
            match self.uart.read_buffered(&mut byte) {
                Ok(0) => {}
                Ok(_) => {
                    if let Some(frame) = self.cutter.push(byte[0]) {
                        let out = buf.get_mut(..frame.len()).ok_or(LinkError::Overflow)?;
                        out.copy_from_slice(frame);
                        return Ok(frame.len());
                    }
                }
                Err(_) => {
                    // FIFO overflow or line error: the partial frame is lost.
                    self.cutter.clear();
                    return Err(LinkError::Io);
                }
            }
            if start.elapsed() >= timeout {
                return Err(LinkError::Timeout);
            }
        }
    }

    async fn send_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        if frame.len() > MAX_UART_FRAME {
            return Err(LinkError::Overflow);
        }
        let mut sent = 0;
        while sent < frame.len() {
            sent += self.uart.write(&frame[sent..]).map_err(|_| LinkError::Io)?;
        }
        self.uart.flush().map_err(|_| LinkError::Io)
    }

    fn kind(&self) -> LinkKind {
        LinkKind::Uart
    }
}
