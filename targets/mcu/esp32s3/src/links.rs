//! The link kinds this package offers, by direction, for ESP32-S3 (a node without downlinks):
//!
//! README.md, "Links", shows the matrix (generated from `LINK_KINDS`,
//! which the MCU definition's `[links]` gives).
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Link, LinkError, Reconnect, Support};

/// The MCU definition's `[links]`: the kinds it lists are `Yes`
/// (`dagon_mcu_common::link::kinds_of_definition`).
pub const LINK_KINDS: &[KindInfo] =
    &dagon_mcu_common::link::kinds_of_definition(crate::LINKS_UP, crate::LINKS_DOWN);

dagon_mcu_common::link_requirements!("esp32s3", LINK_KINDS);

#[cfg(all(target_arch = "xtensa", feature = "hal"))]
pub use node_links::{Uart, UartPort, UsbCdc, UsbPort};

use crate::framer::{FrameCutter, MAX_UART_FRAME};

/// A byte stream one of the node's uplinks runs over: the part of a link
/// that is the chip's (the UART, the USB Serial/JTAG), below
/// [`StreamLink`].
#[allow(async_fn_in_trait)]
pub trait Stream {
    const KIND: Kind;
    const RECONNECT: Reconnect;
    /// Waits for at least one byte and returns how many it read; `Err`
    /// when bytes were lost (a FIFO overflow, a line error).
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()>;
    /// Sends all of `frame` and returns once it has left the node; `Err`
    /// when it did not within the port's send time.
    async fn send(&mut self, frame: &[u8]) -> Result<(), ()>;
}

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

    /// A read that ended at `now_ms` filled `buf`, or lost bytes.
    fn filled(&mut self, read: Result<usize, ()>, now_ms: u32) {
        self.at = 0;
        match read {
            Ok(n) => {
                self.len = n;
                self.t = now_ms;
            }
            // FIFO overflow or line error: the partial frame is lost.
            Err(()) => {
                self.len = 0;
                self.cut.clear();
            }
        }
    }
}

/// An uplink over a byte stream as a [`Link`]: frames cut out of the
/// stream (SOF, CRC, a partial frame dropped after 20 ms without a byte;
/// the bytes of one read share its completion time), replies sent and
/// waited for until they have left the node. The stream and the clock are
/// handed in, so this runs on the host over a scripted stream.
pub struct StreamLink<S> {
    stream: S,
    rx: Rx,
    /// Milliseconds since boot, wrapping (the frame cutter's time base).
    now_ms: fn() -> u32,
}

impl<S: Stream> StreamLink<S> {
    pub fn over(stream: S, now_ms: fn() -> u32) -> Self {
        StreamLink {
            stream,
            rx: Rx::new(),
            now_ms,
        }
    }

    /// Drops a partial frame (after a synchronous section of 20 ms or
    /// more, its gaps are unknown).
    pub fn drop_partial(&mut self) {
        self.rx.cut.drop_partial();
    }
}

impl<S: Stream> Link for StreamLink<S> {
    /// The UART's limit on every stream, not `max_payload::USB` on the USB
    /// one: both share this receive path (frames up to `MAX_UART_FRAME`).
    /// Only a coordinator's usb-cdc carries `max_payload::USB` (dagon-host's
    /// serial Transport); the ESP32-S3 cannot be the coordinator (no
    /// `[coordinator]` in its definition), so nothing sends it frames that
    /// large.
    fn max_payload(&self) -> usize {
        dagon_proto::max_payload::UART
    }

    fn reconnect(&self) -> Reconnect {
        S::RECONNECT
    }

    fn kind(&self) -> Kind {
        S::KIND
    }

    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        loop {
            if let Some(n) = self.rx.next() {
                return Ok(&self.rx.frame[..n]);
            }
            let read = self.stream.read(&mut self.rx.buf).await;
            self.rx.filled(read, (self.now_ms)());
        }
    }

    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.stream
            .send(frame)
            .await
            .map_err(|()| LinkError::Closed)
    }
}

/// The chip's two streams.
#[cfg(all(target_arch = "xtensa", feature = "hal"))]
mod node_links {
    use embassy_time::{with_timeout, Duration};
    use embedded_io_async::{Read, Write};
    use esp_hal::uart::Uart as HalUart;
    use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
    use esp_hal::Async;

    use super::{Kind, Reconnect, Stream, StreamLink};
    use crate::node::now_ms;

    /// Give up on a reply after this long (USB: host stopped reading; the
    /// largest UART frame takes about 90 ms).
    const SEND_TIMEOUT: Duration = Duration::from_millis(500);

    /// UART0, 8N1 at the board's link bit rate (`link::uart_config`).
    pub struct UartPort(HalUart<'static, Async>);

    impl Stream for UartPort {
        const KIND: Kind = Kind::Uart;
        const RECONNECT: Reconnect = Reconnect::Persistent;

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            self.0.read_async(buf).await.map_err(|_| ())
        }

        /// Returns once the transmitter is idle.
        async fn send(&mut self, frame: &[u8]) -> Result<(), ()> {
            let uart = &mut self.0;
            with_timeout(SEND_TIMEOUT, async {
                let mut sent = 0;
                while sent < frame.len() {
                    sent += uart.write_async(&frame[sent..]).await.map_err(|_| ())?;
                }
                uart.flush_async().await.map_err(|_| ())
            })
            .await
            .map_err(|_| ())?
        }
    }

    /// Up `uart`.
    pub type Uart = StreamLink<UartPort>;

    impl Uart {
        pub fn new(uart: HalUart<'static, Async>) -> Self {
            StreamLink::over(UartPort(uart), now_ms)
        }
    }

    /// The chip's USB Serial/JTAG.
    pub struct UsbPort(UsbSerialJtag<'static, Async>);

    impl Stream for UsbPort {
        const KIND: Kind = Kind::UsbCdc;
        const RECONNECT: Reconnect = Reconnect::Reenumerates;

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            self.0.read(buf).await.map_err(|_| ())
        }

        /// Returns once the data is written and flushed to the host.
        async fn send(&mut self, frame: &[u8]) -> Result<(), ()> {
            let usb = &mut self.0;
            with_timeout(SEND_TIMEOUT, async {
                usb.write_all(frame).await.map_err(|_| ())?;
                usb.flush().await.map_err(|_| ())
            })
            .await
            .map_err(|_| ())?
        }
    }

    /// Up `usb-cdc`.
    pub type UsbCdc = StreamLink<UsbPort>;

    impl UsbCdc {
        pub fn new(usb: UsbSerialJtag<'static, Async>) -> Self {
            StreamLink::over(UsbPort(usb), now_ms)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framer::INTER_BYTE_TIMEOUT_MS;
    use core::future::Future;
    use core::pin::pin;
    use core::sync::atomic::{AtomicU32, Ordering};
    use core::task::{Context, Poll, Waker};
    use dagon_proto::{finish, frame_len, HEADER_LEN};
    use std::collections::VecDeque;

    /// Runs a future that never waits for anything outside itself.
    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = pin!(f);
        let mut cx = Context::from_waker(Waker::noop());
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("a read past the script"),
        }
    }

    /// Whether the link, given everything in the script, still waits for
    /// a frame.
    fn waits(l: &mut StreamLink<Script>) -> bool {
        let mut f = pin!(l.read_frame());
        f.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    }

    fn frame(seq: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0; frame_len(payload.len())];
        out[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
        finish(&mut out, 0, seq, 0x01, payload.len()).unwrap();
        out
    }

    /// One read of the scripted stream: the bytes it gives (or lost
    /// bytes), and the clock when it ends.
    type Read = (Result<Vec<u8>, ()>, u32);

    /// A stream that gives the scripted reads in order and keeps what is
    /// sent; `closed` makes every send fail.
    struct Script {
        reads: VecDeque<Read>,
        clock: &'static AtomicU32,
        sent: Vec<Vec<u8>>,
        closed: bool,
    }

    impl Stream for Script {
        const KIND: Kind = Kind::Uart;
        const RECONNECT: Reconnect = Reconnect::Persistent;

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let Some((bytes, at)) = self.reads.pop_front() else {
                return core::future::pending().await;
            };
            self.clock.store(at, Ordering::Relaxed);
            let bytes = bytes?;
            assert!(
                bytes.len() <= buf.len(),
                "a scripted read larger than the buffer"
            );
            buf[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }

        async fn send(&mut self, frame: &[u8]) -> Result<(), ()> {
            if self.closed {
                return Err(());
            }
            self.sent.push(frame.to_vec());
            Ok(())
        }
    }

    macro_rules! link {
        ($reads:expr) => {{
            static CLOCK: AtomicU32 = AtomicU32::new(0);
            StreamLink::over(
                Script {
                    reads: VecDeque::from($reads),
                    clock: &CLOCK,
                    sent: Vec::new(),
                    closed: false,
                },
                || CLOCK.load(Ordering::Relaxed),
            )
        }};
    }

    fn next(l: &mut StreamLink<Script>) -> Vec<u8> {
        block_on(l.read_frame()).unwrap().to_vec()
    }

    #[test]
    fn a_frame_split_over_reads_and_two_frames_in_one_read_all_arrive() {
        let (a, b, c) = (frame(1, b"one"), frame(2, b"two"), frame(3, &[7; 40]));
        let (c1, c2) = c.split_at(9);
        let both = [a.clone(), b.clone()].concat();
        let mut l = link!([
            (Ok(c1.to_vec()), 10),
            (Ok(c2.to_vec()), 12),
            (Ok(both), 30),
            (Ok(c.clone()), 50),
        ]);
        assert_eq!(next(&mut l), c);
        assert_eq!(next(&mut l), a);
        // The second frame of that read needs no further read.
        assert_eq!(l.stream.reads.len(), 1);
        assert_eq!(next(&mut l), b);
        assert_eq!(l.stream.reads.len(), 1);
        assert_eq!(next(&mut l), c);
    }

    #[test]
    fn lost_bytes_drop_the_partial_frame_and_the_next_whole_one_arrives() {
        let (a, b) = (frame(1, &[5; 30]), frame(2, b"next"));
        let (a1, _) = a.split_at(12);
        let mut l = link!([(Ok(a1.to_vec()), 1), (Err(()), 2), (Ok(b.clone()), 3)]);
        assert_eq!(next(&mut l), b);
    }

    #[test]
    fn drop_partial_forgets_what_was_read_of_a_frame() {
        let (a, b) = (frame(1, &[5; 30]), frame(2, b"next"));
        let (a1, _) = a.split_at(12);
        // The part of `a` left behind would swallow the frame after it.
        let mut l = link!([(Ok(a1.to_vec()), 1)]);
        assert!(waits(&mut l));
        l.drop_partial();
        l.stream.reads.push_back((Ok(b.clone()), 2));
        assert_eq!(next(&mut l), b);
    }

    #[test]
    fn a_partial_frame_with_no_byte_for_the_gap_time_is_dropped() {
        let (a, b) = (frame(1, &[5; 30]), frame(2, b"next"));
        let (a1, a2) = a.split_at(12);
        let mut l = link!([
            (Ok(a1.to_vec()), 1),
            (Ok(b.clone()), 1 + INTER_BYTE_TIMEOUT_MS)
        ]);
        assert_eq!(next(&mut l), b);
        // Within the gap the two parts are one frame.
        let mut l = link!([
            (Ok(a1.to_vec()), 1),
            (Ok(a2.to_vec()), INTER_BYTE_TIMEOUT_MS)
        ]);
        assert_eq!(next(&mut l), a);
    }

    #[test]
    fn a_reply_goes_out_whole_and_a_stream_that_cannot_send_closes_the_link() {
        let mut l = link!([]);
        let reply = frame(9, b"reply");
        assert_eq!(block_on(l.write_frame(&reply)), Ok(()));
        assert_eq!(l.stream.sent, core::slice::from_ref(&reply));
        l.stream.closed = true;
        assert_eq!(block_on(l.write_frame(&reply)), Err(LinkError::Closed));
        assert_eq!(
            (l.kind(), l.reconnect(), l.max_payload()),
            (
                Kind::Uart,
                Reconnect::Persistent,
                dagon_proto::max_payload::UART
            )
        );
    }
}
