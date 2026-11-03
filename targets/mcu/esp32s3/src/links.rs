//! The link kinds this package offers, by direction, for ESP32-S3 (a 子機: no downlinks):
//!
//! | Kind | Up | Down |
//! |---|---|---|
//! | `usb-cdc` | Yes | No |
//! | `uart` | Yes | No |
//! | `i2c` | NotYet | No |
//! | `ble` | NotYet | No |
//! | `wifi-tcp` | NotYet | No |
//! | `ethernet` | NotYet | No |
//! | `can` | NotYet | No |
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Link, LinkError, Reconnect, Support};

pub const LINK_KINDS: &[KindInfo] = &[
    KindInfo {
        kind: Kind::UsbCdc,
        up: Support::Yes,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Uart,
        up: Support::Yes,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::I2c,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ble,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::WifiTcp,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ethernet,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Can,
        up: Support::NotYet,
        down: Support::No,
    },
];

dagon_mcu_common::link_requirements!("esp32s3", LINK_KINDS);

#[cfg(target_arch = "xtensa")]
pub use node_links::{Uart, UsbCdc};

/// The node's two uplinks as [`Link`]s: frames cut out of the byte stream
/// (SOF, CRC, a partial frame dropped after 20 ms without a byte; the
/// bytes of one read share its completion time), replies sent and waited
/// for until they have left the node.
#[cfg(target_arch = "xtensa")]
mod node_links {
    use embassy_time::{with_timeout, Duration};
    use embedded_io_async::{Read, Write};
    use esp_hal::uart::Uart as HalUart;
    use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
    use esp_hal::Async;

    use super::{Link, LinkError, Reconnect};
    use crate::framer::{FrameCutter, MAX_UART_FRAME};
    use crate::node::now_ms;

    /// Give up on a reply after this long (USB: host stopped reading; the
    /// largest UART frame takes about 90 ms).
    const SEND_TIMEOUT: Duration = Duration::from_millis(500);

    /// Received bytes and the frame cut out of them.
    struct Rx {
        cut: FrameCutter,
        buf: [u8; 64],
        at: usize,
        len: usize,
        /// Completion time of the read that filled `buf`.
        t: u32,
        frame: [u8; MAX_UART_FRAME],
    }

    impl Rx {
        const fn new() -> Self {
            Rx {
                cut: FrameCutter::new(),
                buf: [0; 64],
                at: 0,
                len: 0,
                t: 0,
                frame: [0; MAX_UART_FRAME],
            }
        }

        /// The next frame from the bytes already read, if any.
        fn next(&mut self) -> Option<usize> {
            while self.at < self.len {
                let b = self.buf[self.at];
                self.at += 1;
                if let Some(f) = self.cut.push_at(b, self.t) {
                    let n = f.len();
                    self.frame[..n].copy_from_slice(f);
                    return Some(n);
                }
            }
            None
        }

        fn filled(&mut self, read: Result<usize, ()>) {
            self.at = 0;
            match read {
                Ok(n) => {
                    self.len = n;
                    self.t = now_ms();
                }
                // FIFO overflow or line error: the partial frame is lost.
                Err(()) => {
                    self.len = 0;
                    self.cut.clear();
                }
            }
        }
    }

    /// Up `uart`: UART0 at 115200 8N1 (`link::uart_config`).
    pub struct Uart {
        uart: HalUart<'static, Async>,
        rx: Rx,
    }

    impl Uart {
        pub fn new(uart: HalUart<'static, Async>) -> Self {
            Uart {
                uart,
                rx: Rx::new(),
            }
        }

        /// Drops a partial frame (after a synchronous section of 20 ms or
        /// more, its gaps are unknown).
        pub fn drop_partial(&mut self) {
            self.rx.cut.drop_partial();
        }
    }

    impl Link for Uart {
        fn max_payload(&self) -> usize {
            dagon_proto::max_payload::UART
        }

        fn reconnect(&self) -> Reconnect {
            Reconnect::Persistent
        }

        async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
            loop {
                if let Some(n) = self.rx.next() {
                    return Ok(&self.rx.frame[..n]);
                }
                let read = self.uart.read_async(&mut self.rx.buf).await.map_err(|_| ());
                self.rx.filled(read);
            }
        }

        /// Returns once the transmitter is idle.
        async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
            let uart = &mut self.uart;
            let done = with_timeout(SEND_TIMEOUT, async {
                let mut sent = 0;
                while sent < frame.len() {
                    sent += uart.write_async(&frame[sent..]).await.map_err(|_| ())?;
                }
                uart.flush_async().await.map_err(|_| ())
            })
            .await;
            match done {
                Ok(Ok(())) => Ok(()),
                _ => Err(LinkError::Closed),
            }
        }
    }

    /// Up `usb-cdc`: the chip's USB Serial/JTAG.
    pub struct UsbCdc {
        usb: UsbSerialJtag<'static, Async>,
        rx: Rx,
    }

    impl UsbCdc {
        pub fn new(usb: UsbSerialJtag<'static, Async>) -> Self {
            UsbCdc { usb, rx: Rx::new() }
        }

        /// As [`Uart::drop_partial`].
        pub fn drop_partial(&mut self) {
            self.rx.cut.drop_partial();
        }
    }

    impl Link for UsbCdc {
        fn max_payload(&self) -> usize {
            dagon_proto::max_payload::UART
        }

        fn reconnect(&self) -> Reconnect {
            Reconnect::Reenumerates
        }

        async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
            loop {
                if let Some(n) = self.rx.next() {
                    return Ok(&self.rx.frame[..n]);
                }
                let read = self.usb.read(&mut self.rx.buf).await.map_err(|_| ());
                self.rx.filled(read);
            }
        }

        /// Returns once the data is written and flushed to the host.
        async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
            let usb = &mut self.usb;
            let done = with_timeout(SEND_TIMEOUT, async {
                usb.write_all(frame).await.map_err(|_| ())?;
                usb.flush().await.map_err(|_| ())
            })
            .await;
            match done {
                Ok(Ok(())) => Ok(()),
                _ => Err(LinkError::Closed),
            }
        }
    }
}
