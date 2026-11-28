//! Request/response with a node on an I2C bus through its mailbox,
//! independent of the board so the exchange runs in host tests: the
//! controller writes the request frame in one write transaction, polls
//! the 3-byte state every [`MAILBOX_POLL_MS`] until the response is ready,
//! then reads it in one read transaction.
//! - a NACKed write (an absent node, or one busy with its last request) is
//!   tried again until the request's deadline, which starts at the write;
//! - every bus transaction is given up at that deadline ([`Pace::until`]),
//!   so a stuck bus ends in a timeout, never a wait without end (a pause
//!   between two tries may end one poll past it);
//! - the tries end at the deadline by the clock too: a transaction that
//!   fails at once, without waiting for the bus (a driver left in an
//!   aborted state), would otherwise be tried again for good, as
//!   [`Pace::until`] gives up only a transaction still waiting;
//! - a response that is no whole frame is refused; the start of a frame of
//!   another wire version is handed over (the node's refusal).

use core::future::Future;

use dagon_port::{I2cBus, NoHello, NodeLink};
use dagon_proto::mailbox::{self, STATE_LEN};
use dagon_proto::payload::HelloResp;
use dagon_proto::{frame_len, max_payload, whole_or_refused, Status};

use crate::link::{Kind, Link, LinkError, Reconnect};

/// Pause before each read of the mailbox state, and after a NACKed write.
pub const MAILBOX_POLL_MS: u32 = 2;
pub const I2C_FRAME: usize = frame_len(max_payload::I2C);
/// Timeout of the coordinator's own display HELLO on I2C.
pub const HELLO_TIMEOUT_MS: u16 = 50;
/// Bound on one I2C_RAW transaction (the PC's ROM frame deadline is 500 ms).
pub const ROM_TIMEOUT_MS: u64 = 500;

/// The link's time: the MCU package's timer, or a test's clock. Used on
/// one executor: its futures need not be `Send`.
#[allow(async_fn_in_trait)]
pub trait Pace {
    /// Milliseconds on a monotonic clock.
    fn now_ms(&self) -> u64;
    /// Waits `ms`.
    async fn pause(&mut self, ms: u32);
    /// `f`, given up when the clock reaches `until_ms` while it still waits
    /// (`None`); an `f` that is done when polled is taken whatever the
    /// clock reads (embassy-time's `with_deadline`).
    async fn until<T>(&mut self, until_ms: u64, f: impl Future<Output = T>) -> Option<T>;
}

/// I2C controller link to other nodes on the bus: mailbox exchanges and
/// raw ROM transactions. As a [`Link`], it talks to the node at the address set
/// by [`I2cLink::target`]: `write_frame` writes the request (a NACK, an
/// absent or busy node, is retried until the deadline, which starts
/// there) and `read_frame` polls the 3-byte state every 2 ms until the
/// response is ready, then reads it.
pub struct I2cLink<I, P> {
    bus: I,
    pace: P,
    /// Its relay link id (the definition's `RELAY_LINK_<PORT>` of its port).
    id: u8,
    addr: u8,
    timeout_ms: u16,
    deadline: u64,
    buf: [u8; I2C_FRAME],
}

impl<I: I2cBus, P: Pace + Default> I2cLink<I, P> {
    /// The link on `bus`, relay link `id` (its port's generated
    /// `RELAY_LINK_<PORT>`).
    pub fn new(bus: I, id: u8) -> Self {
        Self::paced(bus, P::default(), id)
    }
}

impl<I: I2cBus, P: Pace> I2cLink<I, P> {
    /// [`I2cLink::new`], timed by `pace`.
    pub fn paced(bus: I, pace: P, id: u8) -> Self {
        I2cLink {
            bus,
            pace,
            id,
            addr: 0,
            timeout_ms: 0,
            deadline: 0,
            buf: [0; I2C_FRAME],
        }
    }

    /// The node the next request goes to, and its deadline from the start
    /// of the write.
    pub fn target(&mut self, addr: u8, timeout_ms: u16) {
        self.addr = addr;
        self.timeout_ms = timeout_ms;
    }

    /// Sends the request frame `inner` to `addr` and stores the node's
    /// response frame in `out`; returns its length.
    pub async fn exchange(
        &mut self,
        addr: u8,
        inner: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        self.target(addr, timeout_ms);
        self.write_frame(inner).await.map_err(status)?;
        let f = self.read_frame().await.map_err(status)?;
        let n = f.len();
        if n > out.len() {
            return Err(Status::Frame);
        }
        out[..n].copy_from_slice(f);
        Ok(n)
    }

    /// I2C_RAW: one write transaction (if `data` is not empty, closed by
    /// STOP), then one read transaction filling `read` (if not empty). A
    /// NACK or a stuck bus answers E_TIMEOUT.
    pub async fn rom(&mut self, addr: u8, data: &[u8], read: &mut [u8]) -> Result<(), Status> {
        if !data.is_empty() {
            let until = self.pace.now_ms() + ROM_TIMEOUT_MS;
            self.pace
                .until(until, self.bus.write(addr, data))
                .await
                .ok_or(Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        if !read.is_empty() {
            let until = self.pace.now_ms() + ROM_TIMEOUT_MS;
            self.pace
                .until(until, self.bus.read(addr, read))
                .await
                .ok_or(Status::Timeout)?
                .map_err(|_| Status::Timeout)?;
        }
        Ok(())
    }

    pub async fn hello(&mut self, addr: u8, seq: u8) -> Option<HelloResp> {
        NodeLink::hello(self, addr, seq).await
    }

    /// [`I2cLink::hello`], telling a node of another wire version apart
    /// from one that does not answer.
    pub async fn ask_hello(&mut self, addr: u8, seq: u8) -> Result<HelloResp, NoHello> {
        NodeLink::ask_hello(self, addr, seq).await
    }

    /// E_TIMEOUT once the clock reached the request's deadline: the bound
    /// on the tries, whether or not a transaction ever waited.
    fn within_deadline(&self) -> Result<(), LinkError> {
        if self.pace.now_ms() >= self.deadline {
            return Err(LinkError::Timeout);
        }
        Ok(())
    }
}

/// `raw` is the I2C_RAW transfer ([`I2cLink::rom`]).
impl<I: I2cBus, P: Pace> NodeLink for I2cLink<I, P> {
    fn link_id(&self) -> u8 {
        self.id
    }

    fn max_payload(&self) -> usize {
        max_payload::I2C
    }

    fn hello_timeout_ms(&self) -> u16 {
        HELLO_TIMEOUT_MS
    }

    async fn exchange(
        &mut self,
        addr: u8,
        frame: &[u8],
        timeout_ms: u16,
        out: &mut [u8],
    ) -> Result<usize, Status> {
        I2cLink::exchange(self, addr, frame, timeout_ms, out).await
    }

    async fn raw(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), Status> {
        self.rom(addr, write, read).await
    }
}

/// The Status a relayed request answers for a link failure.
fn status(e: LinkError) -> Status {
    match e {
        LinkError::Timeout | LinkError::Closed => Status::Timeout,
        LinkError::Overflow | LinkError::Corrupt => Status::Frame,
    }
}

impl<I: I2cBus, P: Pace> Link for I2cLink<I, P> {
    fn max_payload(&self) -> usize {
        max_payload::I2C
    }

    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }

    fn kind(&self) -> Kind {
        Kind::I2c
    }

    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        loop {
            self.pace.pause(MAILBOX_POLL_MS).await;
            self.within_deadline()?;
            let mut st = [0u8; STATE_LEN];
            if self
                .pace
                .until(self.deadline, self.bus.read(self.addr, &mut st))
                .await
                .ok_or(LinkError::Timeout)?
                .is_err()
            {
                continue;
            }
            let Some(mailbox::State::Ready(n)) = mailbox::State::parse(st) else {
                continue;
            };
            let n = n as usize;
            if n > I2C_FRAME {
                return Err(LinkError::Corrupt);
            }
            self.pace
                .until(self.deadline, self.bus.read(self.addr, &mut self.buf[..n]))
                .await
                .ok_or(LinkError::Timeout)?
                .map_err(|_| LinkError::Corrupt)?;
            // A frame of another wire version is handed over as its start:
            // the node's refusal, read by the requester.
            return match whole_or_refused(&self.buf[..n], max_payload::I2C) {
                Some(len) => Ok(&self.buf[..len]),
                None => Err(LinkError::Corrupt),
            };
        }
    }

    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.deadline = self.pace.now_ms() + u64::from(self.timeout_ms);
        // A NACK (absent or busy node) is retried until the deadline.
        while self
            .pace
            .until(self.deadline, self.bus.write(self.addr, frame))
            .await
            .ok_or(LinkError::Timeout)?
            .is_err()
        {
            self.pace.pause(MAILBOX_POLL_MS).await;
            self.within_deadline()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::cell::Cell;
    use core::future::poll_fn;
    use core::pin::pin;
    use core::task::Poll;
    use std::rc::Rc;
    use std::vec;
    use std::vec::Vec;

    use dagon_port::BusError;
    use dagon_proto::{flags, Frame, PROTO_VERSION, REFUSED_LEN};
    use dagon_sim::block_on;

    use super::*;

    const NODE: u8 = 0x42;
    /// Time one bus transaction takes.
    const TRANSACTION_MS: u64 = 1;

    /// A node behind the bus, with the faults a bus and a node have.
    #[derive(Default)]
    struct Bus {
        clock: Rc<Cell<u64>>,
        /// Request writes NACKed before one is taken.
        nacks: usize,
        /// State reads answered BUSY before READY.
        busy: usize,
        /// State reads that fail (a NACK) before any is answered.
        deaf: usize,
        /// The state reads READY with this length instead of the response's.
        ready_len: Option<u16>,
        response: Vec<u8>,
        /// No transaction ever ends (SCL held low).
        stuck: bool,
        /// The response read fails.
        read_fails: bool,
        requests: Vec<Vec<u8>>,
        state_reads: usize,
    }

    impl Bus {
        async fn transaction(&self) {
            if self.stuck {
                core::future::pending::<()>().await;
            }
            self.clock.set(self.clock.get() + TRANSACTION_MS);
        }
    }

    impl I2cBus for Bus {
        async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
            self.transaction().await;
            if addr != NODE || self.nacks > 0 {
                self.nacks = self.nacks.saturating_sub(1);
                return Err(BusError::Nack);
            }
            self.requests.push(data.to_vec());
            Ok(())
        }
        async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
            self.transaction().await;
            if addr != NODE {
                return Err(BusError::Nack);
            }
            if buf.len() == STATE_LEN && self.state_reads <= self.deaf + self.busy {
                self.state_reads += 1;
                if self.state_reads <= self.deaf {
                    return Err(BusError::Nack);
                }
                let ready = self.state_reads > self.deaf + self.busy;
                let len = self.ready_len.unwrap_or(self.response.len() as u16);
                let state = if ready {
                    mailbox::State::Ready(len)
                } else {
                    mailbox::State::Busy
                };
                buf.copy_from_slice(&state.to_bytes());
                return Ok(());
            }
            if self.read_fails {
                return Err(BusError::Nack);
            }
            let n = buf.len().min(self.response.len());
            buf[..n].copy_from_slice(&self.response[..n]);
            Ok(())
        }
        async fn write_read(&mut self, _: u8, _: &[u8], _: &mut [u8]) -> Result<(), BusError> {
            unreachable!("the mailbox link makes no combined transaction")
        }
    }

    /// A clock the test's bus advances. As embassy-time's `with_deadline`
    /// on the boards: a transaction still waiting at the deadline (a stuck
    /// one) is given up, one that is done when polled is taken, past the
    /// deadline too (this bus's transactions are all done at once).
    struct Clock(Rc<Cell<u64>>);

    impl Pace for Clock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
        async fn pause(&mut self, ms: u32) {
            self.0.set(self.0.get() + u64::from(ms));
        }
        async fn until<T>(&mut self, until_ms: u64, f: impl Future<Output = T>) -> Option<T> {
            let mut f = pin!(f);
            let clock = &self.0;
            poll_fn(|cx| match f.as_mut().poll(cx) {
                Poll::Ready(v) => Poll::Ready(Some(v)),
                Poll::Pending => {
                    clock.set(clock.get().max(until_ms));
                    Poll::Ready(None)
                }
            })
            .await
        }
    }

    fn frame(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0; frame_len(payload.len())];
        Frame {
            flags,
            seq: 7,
            cmd: 0x10,
            payload,
        }
        .encode(&mut out)
        .unwrap();
        out
    }

    fn link(bus: Bus) -> (I2cLink<Bus, Clock>, Rc<Cell<u64>>) {
        let clock = bus.clock.clone();
        (I2cLink::paced(bus, Clock(clock.clone()), 3), clock)
    }

    fn exchange(l: &mut I2cLink<Bus, Clock>, timeout_ms: u16) -> Result<Vec<u8>, Status> {
        let mut out = [0u8; I2C_FRAME];
        let request = frame(0, &[1, 2, 3]);
        block_on(l.exchange(NODE, &request, timeout_ms, &mut out)).map(|n| out[..n].to_vec())
    }

    #[test]
    fn a_request_is_written_once_and_its_response_read_when_ready() {
        let response = frame(flags::RESP, &[0, 9, 9]);
        let (mut l, clock) = link(Bus {
            busy: 3,
            response: response.clone(),
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 100), Ok(response));
        assert_eq!(l.bus.requests, [frame(0, &[1, 2, 3])]);
        // BUSY three times, then READY: four polls, 2 ms before each.
        assert_eq!(l.bus.state_reads, 4);
        assert_eq!(
            clock.get(),
            TRANSACTION_MS + 4 * (u64::from(MAILBOX_POLL_MS) + TRANSACTION_MS) + TRANSACTION_MS
        );
        assert_eq!((l.link_id(), Link::kind(&l)), (3, Kind::I2c));
    }

    #[test]
    fn a_nacked_write_is_tried_again_until_the_node_takes_it() {
        let response = frame(flags::RESP, &[0]);
        let (mut l, _) = link(Bus {
            nacks: 5,
            response: response.clone(),
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 100), Ok(response));
        assert_eq!(l.bus.requests.len(), 1);
    }

    #[test]
    fn a_node_that_never_takes_the_request_times_out_at_the_deadline() {
        let (mut l, clock) = link(Bus {
            nacks: usize::MAX,
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 50), Err(Status::Timeout));
        assert!(l.bus.requests.is_empty());
        // The pause before the next try may end past the deadline.
        assert!((50..=50 + u64::from(MAILBOX_POLL_MS)).contains(&clock.get()));
    }

    /// Every try fails at once, as a driver left in an aborted state does
    /// (no transaction waits, so `until` gives none up): the tries still
    /// end at the deadline, by the clock, for the write and for the polls.
    #[test]
    fn tries_that_fail_at_once_end_at_the_deadline() {
        let (mut l, clock) = link(Bus {
            nacks: usize::MAX,
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 40), Err(Status::Timeout));
        assert!((40..=40 + u64::from(MAILBOX_POLL_MS)).contains(&clock.get()));
        let (mut l, clock) = link(Bus {
            deaf: usize::MAX / 2,
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 40), Err(Status::Timeout));
        assert!((40..=40 + u64::from(MAILBOX_POLL_MS)).contains(&clock.get()));
    }

    #[test]
    fn a_node_that_stays_busy_times_out_at_the_deadline() {
        let (mut l, clock) = link(Bus {
            busy: usize::MAX / 2,
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 50), Err(Status::Timeout));
        assert!((50..=50 + u64::from(MAILBOX_POLL_MS)).contains(&clock.get()));
    }

    #[test]
    fn failed_state_reads_are_polled_again() {
        let response = frame(flags::RESP, &[0]);
        let (mut l, _) = link(Bus {
            deaf: 4,
            busy: 1,
            response: response.clone(),
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 100), Ok(response));
    }

    #[test]
    fn a_stuck_bus_ends_every_wait_at_its_deadline() {
        let (mut l, clock) = link(Bus {
            stuck: true,
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 30), Err(Status::Timeout));
        assert_eq!(clock.get(), 30);
        // I2C_RAW: its own bound, for the write and for the read.
        let mut read = [0u8; 2];
        assert_eq!(block_on(l.rom(NODE, &[1], &mut read)), Err(Status::Timeout));
        assert_eq!(clock.get(), 30 + ROM_TIMEOUT_MS);
        assert_eq!(block_on(l.rom(NODE, &[], &mut read)), Err(Status::Timeout));
        assert_eq!(clock.get(), 30 + 2 * ROM_TIMEOUT_MS);
    }

    #[test]
    fn what_is_no_whole_frame_is_refused() {
        let good = frame(flags::RESP, &[0, 1]);
        let mut bad_crc = good.clone();
        *bad_crc.last_mut().unwrap() ^= 1;
        for (bus, why) in [
            (
                Bus {
                    ready_len: Some(I2C_FRAME as u16 + 1),
                    ..Bus::default()
                },
                "a length beyond the link's frame",
            ),
            (
                Bus {
                    response: bad_crc,
                    ..Bus::default()
                },
                "a broken frame",
            ),
            (
                Bus {
                    ready_len: Some(good.len() as u16 - 1),
                    response: good.clone(),
                    ..Bus::default()
                },
                "a frame cut short",
            ),
            (
                Bus {
                    read_fails: true,
                    response: good.clone(),
                    ..Bus::default()
                },
                "a failed read of the response",
            ),
        ] {
            let (mut l, _) = link(bus);
            assert_eq!(exchange(&mut l, 100), Err(Status::Frame), "{why}");
        }
    }

    #[test]
    fn a_frame_of_another_wire_version_is_handed_over_as_its_start() {
        let mut other = frame(flags::RESP, &[0, 1, 2, 3]);
        other[2] = PROTO_VERSION + 1;
        let (mut l, _) = link(Bus {
            response: other.clone(),
            ..Bus::default()
        });
        assert_eq!(exchange(&mut l, 100), Ok(other[..REFUSED_LEN].to_vec()));
    }

    #[test]
    fn raw_transactions_reach_the_bus_as_asked() {
        let (mut l, _) = link(Bus {
            response: vec![0xAA, 0xBB],
            ..Bus::default()
        });
        let mut read = [0u8; 2];
        assert_eq!(block_on(l.rom(NODE, &[5, 6], &mut read)), Ok(()));
        assert_eq!((l.bus.requests.len(), read), (1, [0xAA, 0xBB]));
        // A NACK is a timeout to the requester.
        assert_eq!(
            block_on(l.rom(NODE + 1, &[5], &mut [])),
            Err(Status::Timeout)
        );
    }
}
