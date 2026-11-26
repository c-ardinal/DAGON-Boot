//! The node's side of the I2C mailbox link, independent of the I2C driver
//! so it runs in host tests: the hub writes a request frame, polls the
//! mailbox state and reads the response ([`crate::mailbox`]).
//! - a request is handed out with the own address NACKed, so the hub's
//!   polls while the request is handled (the flash may stall the CPU) are
//!   refused by the peripheral, not stretched;
//! - [`Link::write_frame`] holds the response and ACKs again;
//! - [`Link::flush`] serves the hub's reads until it has the whole
//!   response; a new request first is kept for the next read.

use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_proto::{frame_len, max_payload};

use crate::mailbox::Mailbox;

const RX: usize = frame_len(max_payload::I2C);

/// What the hub started on the bus, addressed to this node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// A write transaction: [`TargetBus::take_write`] takes its bytes.
    Write,
    /// A read transaction: [`TargetBus::give_read`] offers its bytes.
    Read,
}

/// The I2C target (peripheral role) as the link drives it: the MCU's
/// driver, or a test's hub.
#[allow(async_fn_in_trait)]
pub trait TargetBus {
    /// The next transaction addressed to this node. `Err(Timeout)`: none
    /// within the driver's tick (the serving loop then drops an abandoned
    /// transfer); `Err(Corrupt)`: a bus error.
    async fn listen(&mut self) -> Result<Asked, LinkError>;
    /// Takes the bytes of the write under way; returns how many.
    async fn take_write(&mut self, buf: &mut [u8]) -> Result<usize, LinkError>;
    /// Offers `data` to the read under way; returns how many bytes the
    /// hub clocked.
    async fn give_read(&mut self, data: &[u8]) -> Result<usize, LinkError>;
    /// ACK (true) or NACK (false) the own address and data from now on.
    fn acknowledge(&mut self, on: bool);
    /// The own 7-bit address.
    fn addr(&self) -> u8;
}

/// The node's I2C target link over `B`.
pub struct MailboxLink<B> {
    bus: B,
    mailbox: Mailbox,
    rx: [u8; RX],
    /// A request that arrived while flushing, handed out next.
    pending: Option<usize>,
}

impl<B: TargetBus> MailboxLink<B> {
    /// The link on `bus`, which listens at the node's address.
    pub fn over(bus: B) -> Self {
        MailboxLink {
            bus,
            mailbox: Mailbox::default(),
            rx: [0; RX],
            pending: None,
        }
    }

    /// One bus event: `Ok(Some(n))` a request of `n` bytes, `Ok(None)`
    /// anything else (a served read reports whether the whole
    /// response left in `done`).
    async fn event(&mut self, done: &mut bool) -> Result<Option<usize>, LinkError> {
        match self.bus.listen().await? {
            Asked::Write => {
                let n = self.bus.take_write(&mut self.rx).await?;
                // Zero bytes: an address probe.
                if n == 0 {
                    return Ok(None);
                }
                self.mailbox.request();
                self.bus.acknowledge(false);
                Ok(Some(n))
            }
            Asked::Read => {
                let sent = self.bus.give_read(self.mailbox.read_data()).await?;
                *done = self.mailbox.read_done(sent);
                Ok(None)
            }
        }
    }
}

impl<B: TargetBus> Link for MailboxLink<B> {
    fn max_payload(&self) -> usize {
        max_payload::I2C
    }

    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }

    fn kind(&self) -> Kind {
        Kind::I2c
    }

    /// The own 7-bit address, as the board's link configured it.
    fn addr(&self) -> u8 {
        self.bus.addr()
    }

    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        if let Some(n) = self.pending.take() {
            return Ok(&self.rx[..n]);
        }
        loop {
            let mut done = false;
            if let Some(n) = self.event(&mut done).await? {
                return Ok(&self.rx[..n]);
            }
        }
    }

    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.mailbox.respond(Some(frame), false);
        self.bus.acknowledge(true);
        Ok(())
    }

    /// No answer: reads stay BUSY (the hub times out and resends) and
    /// the own address is ACKed again for the next request.
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.mailbox.respond(None, false);
        self.bus.acknowledge(true);
        Ok(())
    }

    /// Serves the hub's reads until it has the whole response. A new
    /// request first: kept for the next read, and this fails (the
    /// caller then does not reset).
    async fn flush(&mut self) -> Result<(), LinkError> {
        self.mailbox.reboot_after_read();
        loop {
            let mut done = false;
            match self.event(&mut done).await {
                Ok(Some(n)) => {
                    self.pending = Some(n);
                    return Err(LinkError::Closed);
                }
                Ok(None) if done => return Ok(()),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::collections::VecDeque;
    use std::vec;
    use std::vec::Vec;

    use dagon_proto::mailbox::{State, STATE_LEN};

    use super::*;

    /// The link's futures never wait here: the test's hub answers at once.
    fn block_on<F: core::future::Future>(f: F) -> F::Output {
        let mut f = core::pin::pin!(f);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(v) => v,
            core::task::Poll::Pending => panic!("the test's hub left the link waiting"),
        }
    }

    /// What the hub does next on the bus.
    enum Hub {
        /// Writes these bytes to the node.
        Writes(Vec<u8>),
        /// Reads this many bytes from the node.
        Reads(usize),
        /// Nothing within the driver's tick.
        Quiet,
        /// A bus error.
        Breaks,
    }

    struct Bus {
        hub: VecDeque<Hub>,
        /// The transaction [`TargetBus::listen`] announced.
        now: Option<Hub>,
        /// What the hub read, per read transaction.
        read: Vec<Vec<u8>>,
        /// Every change of the ACK bit, in order.
        acks: Vec<bool>,
    }

    impl Bus {
        fn new(hub: impl IntoIterator<Item = Hub>) -> Self {
            Bus {
                hub: hub.into_iter().collect(),
                now: None,
                read: Vec::new(),
                acks: Vec::new(),
            }
        }
    }

    impl TargetBus for Bus {
        async fn listen(&mut self) -> Result<Asked, LinkError> {
            let next = self.hub.pop_front().expect("the test's hub has more to do");
            let asked = match next {
                Hub::Writes(_) => Asked::Write,
                Hub::Reads(_) => Asked::Read,
                Hub::Quiet => return Err(LinkError::Timeout),
                Hub::Breaks => return Err(LinkError::Corrupt),
            };
            self.now = Some(next);
            Ok(asked)
        }
        async fn take_write(&mut self, buf: &mut [u8]) -> Result<usize, LinkError> {
            let Some(Hub::Writes(data)) = self.now.take() else {
                panic!("no write under way")
            };
            buf[..data.len()].copy_from_slice(&data);
            Ok(data.len())
        }
        async fn give_read(&mut self, data: &[u8]) -> Result<usize, LinkError> {
            let Some(Hub::Reads(n)) = self.now.take() else {
                panic!("no read under way")
            };
            let n = n.min(data.len());
            self.read.push(data[..n].to_vec());
            Ok(n)
        }
        fn acknowledge(&mut self, on: bool) {
            self.acks.push(on);
        }
        fn addr(&self) -> u8 {
            0x42
        }
    }

    const REQUEST: [u8; 5] = [0xDA, 0x6F, 1, 2, 3];
    const REPLY: [u8; 7] = [9, 8, 7, 6, 5, 4, 3];

    fn ready(n: usize) -> Vec<u8> {
        State::Ready(n as u16).to_bytes().to_vec()
    }

    #[test]
    fn a_request_is_handed_out_with_the_address_nacked_until_it_is_answered() {
        // A probe (an empty write) and a state poll come first.
        let mut l = MailboxLink::over(Bus::new([
            Hub::Writes(vec![]),
            Hub::Reads(STATE_LEN),
            Hub::Writes(REQUEST.to_vec()),
        ]));
        assert_eq!(block_on(l.read_frame()), Ok(&REQUEST[..]));
        assert_eq!(l.bus.read, [State::Busy.to_bytes().to_vec()]);
        assert_eq!(l.bus.acks, [false]);
        block_on(l.write_frame(&REPLY)).unwrap();
        assert_eq!(l.bus.acks, [false, true]);
        assert_eq!(
            (l.addr(), l.kind(), l.max_payload()),
            (0x42, Kind::I2c, max_payload::I2C)
        );
    }

    #[test]
    fn the_response_is_read_as_its_state_then_its_frame() {
        let mut l = MailboxLink::over(Bus::new([
            Hub::Writes(REQUEST.to_vec()),
            Hub::Reads(STATE_LEN),
            Hub::Reads(REPLY.len()),
            // Read in full: the mailbox is empty again.
            Hub::Reads(STATE_LEN),
            Hub::Writes(vec![0xAB]),
        ]));
        block_on(l.read_frame()).unwrap();
        block_on(l.write_frame(&REPLY)).unwrap();
        assert_eq!(block_on(l.read_frame()), Ok(&[0xAB][..]));
        assert_eq!(
            l.bus.read,
            [
                ready(REPLY.len()),
                REPLY.to_vec(),
                State::Busy.to_bytes().to_vec()
            ]
        );
    }

    #[test]
    fn a_request_left_unanswered_keeps_reads_busy_and_takes_the_next_request() {
        let mut l = MailboxLink::over(Bus::new([
            Hub::Writes(REQUEST.to_vec()),
            Hub::Reads(STATE_LEN),
            Hub::Writes(REQUEST.to_vec()),
        ]));
        block_on(l.read_frame()).unwrap();
        block_on(l.no_reply()).unwrap();
        assert_eq!(l.bus.acks, [false, true]);
        assert_eq!(block_on(l.read_frame()), Ok(&REQUEST[..]));
        assert_eq!(l.bus.read, [State::Busy.to_bytes().to_vec()]);
    }

    #[test]
    fn a_flush_ends_when_the_hub_has_the_whole_response() {
        let mut l = MailboxLink::over(Bus::new([
            Hub::Writes(REQUEST.to_vec()),
            Hub::Quiet,
            Hub::Reads(STATE_LEN),
            Hub::Breaks,
            // A read cut short: the state is offered again.
            Hub::Reads(REPLY.len() - 1),
            Hub::Reads(STATE_LEN),
            Hub::Reads(REPLY.len()),
        ]));
        block_on(l.read_frame()).unwrap();
        block_on(l.write_frame(&REPLY)).unwrap();
        assert_eq!(block_on(l.flush()), Ok(()));
        assert!(l.bus.hub.is_empty());
        assert_eq!(l.bus.read.last().unwrap(), &REPLY);
    }

    #[test]
    fn a_request_during_a_flush_fails_it_and_is_handed_out_next() {
        let mut l = MailboxLink::over(Bus::new([
            Hub::Writes(REQUEST.to_vec()),
            Hub::Reads(STATE_LEN),
            Hub::Writes(vec![0xCD, 0xEF]),
        ]));
        block_on(l.read_frame()).unwrap();
        block_on(l.write_frame(&REPLY)).unwrap();
        assert_eq!(block_on(l.flush()), Err(LinkError::Closed));
        assert_eq!(block_on(l.read_frame()), Ok(&[0xCD, 0xEF][..]));
        // The new request NACKs the address again.
        assert_eq!(l.bus.acks, [false, true, false]);
    }

    #[test]
    fn a_quiet_tick_and_a_bus_error_end_a_read_as_they_are() {
        let mut l = MailboxLink::over(Bus::new([Hub::Quiet, Hub::Breaks]));
        assert_eq!(block_on(l.read_frame()), Err(LinkError::Timeout));
        assert_eq!(block_on(l.read_frame()), Err(LinkError::Corrupt));
    }
}
