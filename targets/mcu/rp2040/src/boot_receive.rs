//! The boot core's receiving (design DES-RB-030), over whichever links the
//! node's boot core opens: the I2C target and the UART from the board's
//! DAGON buses, and USB, which is enumerated and so opened only once the
//! boot core receives. The links are async drivers, read as one
//! ([`Links`]) by a [`pump`] that hands each request to the shared
//! `dagon_agent::update_mode` through its `Port` ([`PumpPort`]) and sends
//! the answer back on the link it came on. The port's wait polls the pump
//! between sleeps (the boot core sleeps on WFE until an interrupt, [`Sleep`]).
//! [`receive_step`] runs before the boot decision (the app's hand-over,
//! else the listen window); [`serve`] runs update or recovery mode. The
//! links and the sleep are traits, so the host tests run this very code.

use core::cell::{Cell, RefCell};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use dagon_agent::update_mode::{self, Exit, Port, Stay};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_events::Clock;
use dagon_format::SigVerifier;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_port::Flash;
use dagon_proto::MAX_FRAME;
use embassy_futures::select::{select3, Either3};

pub use dagon_agent::update_mode::Entry;

/// The address the boot core's I2C target answers at: the node's from
/// its end in the topology (`topology`, its boot wiring's `LINK_ADDR`),
/// else the board's `[default_link] addr` (`board`); `None`: no I2C link.
pub const fn link_addr(topology: Option<u8>, board: Option<u8>) -> Option<u8> {
    match topology {
        Some(a) => Some(a),
        None => board,
    }
}

/// A link kind the node does not open (its [`Links`] entry stays `None`).
pub enum NoLink {}

impl Link for NoLink {
    fn max_payload(&self) -> usize {
        match *self {}
    }
    fn reconnect(&self) -> Reconnect {
        match *self {}
    }
    fn kind(&self) -> Kind {
        match *self {}
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        match *self {}
    }
    async fn write_frame(&mut self, _: &[u8]) -> Result<(), LinkError> {
        match *self {}
    }
}

/// The links of a boot core as one: a request is read from whichever
/// link brings one first, and its reply goes back on that link.
pub struct Links<U, I, T> {
    pub usb: Option<U>,
    pub i2c: Option<I>,
    pub uart: Option<T>,
    from: Kind,
}

impl<U: Link, I: Link, T: Link> Links<U, I, T> {
    pub fn new(usb: Option<U>, i2c: Option<I>, uart: Option<T>) -> Self {
        Links {
            usb,
            i2c,
            uart,
            from: Kind::UsbCdc,
        }
    }

    /// Whether any link is open.
    pub fn any(&self) -> bool {
        self.usb.is_some() || self.i2c.is_some() || self.uart.is_some()
    }

    /// The largest payload any open link carries (the session's limit; a
    /// request names its own link's).
    pub fn max_payload(&self) -> usize {
        let p = |l: Option<usize>| l.unwrap_or(0);
        p(self.usb.as_ref().map(Link::max_payload))
            .max(p(self.i2c.as_ref().map(Link::max_payload)))
            .max(p(self.uart.as_ref().map(Link::max_payload)))
    }
}

async fn read<L: Link>(l: &mut Option<L>) -> Result<&[u8], LinkError> {
    match l {
        Some(l) => l.read_frame().await,
        None => core::future::pending().await,
    }
}

/// Runs `$f` on the link the last request came from.
macro_rules! on_from {
    ($s:ident, $l:ident => $f:expr) => {
        match $s.from {
            Kind::I2c => match $s.i2c.as_mut() {
                Some($l) => $f,
                None => Err(LinkError::Closed),
            },
            Kind::Uart => match $s.uart.as_mut() {
                Some($l) => $f,
                None => Err(LinkError::Closed),
            },
            _ => match $s.usb.as_mut() {
                Some($l) => $f,
                None => Err(LinkError::Closed),
            },
        }
    };
}

impl<U: Link, I: Link, T: Link> Link for Links<U, I, T> {
    fn max_payload(&self) -> usize {
        Links::max_payload(self)
    }
    fn reconnect(&self) -> Reconnect {
        Reconnect::Persistent
    }
    fn kind(&self) -> Kind {
        self.from
    }
    fn addr(&self) -> u8 {
        match self.from {
            Kind::I2c => self.i2c.as_ref().map_or(dagon_port::NO_ADDR, Link::addr),
            _ => dagon_port::NO_ADDR,
        }
    }
    /// The next frame on any open link; with none open, never.
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        let read = select3(
            read(&mut self.usb),
            read(&mut self.i2c),
            read(&mut self.uart),
        )
        .await;
        let (from, r) = match read {
            Either3::First(r) => (Kind::UsbCdc, r),
            Either3::Second(r) => (Kind::I2c, r),
            Either3::Third(r) => (Kind::Uart, r),
        };
        self.from = from;
        r
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        on_from!(self, l => l.write_frame(frame).await)
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        on_from!(self, l => l.no_reply().await)
    }
    async fn flush(&mut self) -> Result<(), LinkError> {
        on_from!(self, l => l.flush().await)
    }
}

/// Where the request the pump read stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// The pump is reading.
    Reading,
    /// A request waits for the port to take it.
    Arrived,
    /// The port took it; the pump waits for the answer.
    Taken,
    /// The answer is in `reply` (`None`: no answer).
    Answered,
}

/// What passes between the pump and the port, on one thread.
pub struct Mailbox {
    turn: Cell<Turn>,
    req: RefCell<[u8; MAX_FRAME]>,
    req_len: Cell<usize>,
    reply: RefCell<[u8; MAX_FRAME]>,
    reply_len: Cell<Option<usize>>,
    reboot: Cell<bool>,
    delivered: Cell<bool>,
}

impl Mailbox {
    pub const fn new() -> Self {
        Mailbox {
            turn: Cell::new(Turn::Reading),
            req: RefCell::new([0; MAX_FRAME]),
            req_len: Cell::new(0),
            reply: RefCell::new([0; MAX_FRAME]),
            reply_len: Cell::new(None),
            reboot: Cell::new(false),
            delivered: Cell::new(false),
        }
    }
}

impl Mailbox {
    /// A mailbox whose request was already taken by the session (the
    /// listen window's STAY, which update mode answers first).
    pub fn taken() -> Self {
        let mb = Mailbox::new();
        mb.turn.set(Turn::Taken);
        mb
    }
}

impl Default for Mailbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads requests from `link` into `mb` and sends back the answers the
/// port puts there, on the link each came on; after an answer marked as
/// the reply to REBOOT, flushes it and marks it delivered.
pub async fn pump<L: Link>(link: &mut L, mb: &Mailbox) {
    loop {
        // A request taken before this pump started (the window's STAY)
        // is answered first, on the link it came on.
        if !matches!(mb.turn.get(), Turn::Taken | Turn::Answered) {
            let Ok(f) = link.read_frame().await else {
                continue;
            };
            let n = f.len().min(MAX_FRAME);
            mb.req.borrow_mut()[..n].copy_from_slice(&f[..n]);
            mb.req_len.set(n);
            mb.turn.set(Turn::Arrived);
        }
        core::future::poll_fn(|_| match mb.turn.get() {
            Turn::Answered => Poll::Ready(()),
            _ => Poll::Pending,
        })
        .await;
        let sent = match mb.reply_len.get() {
            Some(n) => {
                let reply = *mb.reply.borrow();
                link.write_frame(&reply[..n]).await.is_ok()
            }
            None => {
                let _ = link.no_reply().await;
                false
            }
        };
        if sent && mb.reboot.get() && link.flush().await.is_ok() {
            mb.delivered.set(true);
        }
        mb.turn.set(Turn::Reading);
    }
}

/// How the port sleeps between polls of the pump.
pub trait Sleep {
    /// A wait of at most `max_us` starts.
    fn arm(&mut self, max_us: u32);
    /// The wait's time is up.
    fn done(&mut self) -> bool;
    /// Sleeps until something may have changed (an interrupt).
    fn idle(&mut self);
}

/// The boot core's `Port`: polls the pump `F` (reading `mb`) and sleeps
/// with `S` between polls.
pub struct PumpPort<'a, F, S> {
    pump: Pin<&'a mut F>,
    mb: &'a Mailbox,
    sleep: S,
    req: [u8; MAX_FRAME],
}

impl<'a, F: Future<Output = ()>, S: Sleep> PumpPort<'a, F, S> {
    pub fn new(pump: Pin<&'a mut F>, mb: &'a Mailbox, sleep: S) -> Self {
        PumpPort {
            pump,
            mb,
            sleep,
            req: [0; MAX_FRAME],
        }
    }

    fn turn(&mut self) {
        let mut cx = Context::from_waker(Waker::noop());
        let _ = self.pump.as_mut().poll(&mut cx);
    }

    fn ready(&self) -> bool {
        self.mb.turn.get() == Turn::Arrived || self.mb.delivered.get()
    }
}

impl<F: Future<Output = ()>, S: Sleep> Port for PumpPort<'_, F, S> {
    fn poll(&mut self) -> Option<&[u8]> {
        self.turn();
        if self.mb.turn.get() != Turn::Arrived {
            return None;
        }
        self.mb.turn.set(Turn::Taken);
        let n = self.mb.req_len.get();
        self.req[..n].copy_from_slice(&self.mb.req.borrow()[..n]);
        Some(&self.req[..n])
    }
    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool) {
        let n = frame.map(|f| {
            let n = f.len().min(MAX_FRAME);
            self.mb.reply.borrow_mut()[..n].copy_from_slice(&f[..n]);
            n
        });
        self.mb.reply_len.set(n);
        self.mb.reboot.set(reboot);
        self.mb.delivered.set(false);
        self.mb.turn.set(Turn::Answered);
        self.turn();
    }
    fn delivered(&mut self) -> bool {
        self.turn();
        self.mb.delivered.get()
    }
    fn wait(&mut self, max_us: u32) {
        self.sleep.arm(max_us);
        loop {
            self.turn();
            if self.ready() || self.sleep.done() {
                return;
            }
            self.sleep.idle();
        }
    }
}

/// The step before the boot decision on `port` (the links that take part
/// in the listen window; USB is not among them): the app handed over (an
/// UPDATE_STAY, consumed here), else a STAY within `window_ms` (the MCU
/// definition's window for those links, 0: none) on `clock`. `receives`:
/// some link (USB included) will serve the session; without one the step
/// reads no flash and takes nothing.
pub fn receive_step<F: Flash, P: Port, C: Clock>(
    flash: &mut F,
    port: &mut P,
    receives: bool,
    clock: &mut C,
    window_ms: u16,
    feed_every_ms: u32,
    feed: &mut impl FnMut(),
) -> Option<(Entry, Option<Stay>)> {
    if !receives {
        return None;
    }
    if crate::bl::handed_over(flash) {
        return Some((Entry::HandOver, None));
    }
    let stay = update_mode::listen(port, clock, window_ms, feed_every_ms, feed)?;
    Some((Entry::Stay, Some(stay)))
}

/// Update or recovery mode (`entry`) on `port`, the shared session loop:
/// `agent` answers `first` (the window's STAY), then every request, until
/// the reply to REBOOT has been read or, in update mode, no request came
/// for `update_mode::EXIT_IDLE_MS`. Either way the boot core restarts.
pub fn serve<P, S, X, V, C>(
    port: &mut P,
    agent: &mut Agent<S, X, V, BootCore>,
    clock: &mut C,
    feed_every_ms: u32,
    feed: &mut impl FnMut(),
    entry: Entry,
    first: Option<&[u8]>,
) -> Exit
where
    P: Port,
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    C: Clock,
{
    update_mode::serve_in(port, agent, clock, feed_every_ms, feed, first, entry)
}
