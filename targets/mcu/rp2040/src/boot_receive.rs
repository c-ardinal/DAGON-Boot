//! The boot core's receiving (design DES-RB-030), over whichever links the
//! node's boot core opens: the I2C target and the UART from the board's
//! DAGON buses, and USB, which is enumerated and so opened only once the
//! boot core receives. [`receive_step`] runs before the boot decision (the
//! app's hand-over, else the listen window); [`serve`] runs update or
//! recovery mode on every link at once. The links are any [`Link`], so the
//! host tests run this very code over simulated ones.

use dagon_agent::bootcore::{listen, IdleExit};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_format::SigVerifier;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_mcu_common::serve::{serve as serve_loop, Exit, ServeHost};
use dagon_port::Flash;
use embassy_futures::select::{select3, Either3};

pub use dagon_agent::bootcore::Entry;

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
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        if !self.any() {
            return Err(LinkError::Closed);
        }
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

/// The step before the boot decision, on `links` (the links that take
/// part in the listen window; USB is not among them): the app handed over
/// (an UPDATE_STAY, consumed here), else a STAY within `window_ms` (the
/// MCU definition's window for those links, 0: none) on `now`'s clock.
/// With no link the boot core receives nothing and the step reads no
/// flash. `receives`: some link (USB included) will serve the session.
pub async fn receive_step<F: Flash, L: Link>(
    flash: &mut F,
    links: &mut L,
    receives: bool,
    window_ms: u16,
    now: impl Fn() -> u64,
) -> Option<Entry> {
    if !receives {
        return None;
    }
    if crate::bl::handed_over(flash) {
        return Some(Entry::HandOver);
    }
    listen(links, window_ms, now).await.then_some(Entry::Stay)
}

/// Update or recovery mode (`entry`) on `links`: serves `agent` until the
/// reply to REBOOT has left (`Exit::Reboot`), or the links close
/// (`Exit::Closed`); `true` with it when they closed because no request
/// came for 60 s in update mode. Either way but `(Closed, false)` the boot
/// core restarts.
pub async fn serve<L, S, X, V, H>(
    links: L,
    agent: &mut Agent<S, X, V, BootCore>,
    host: &mut H,
    entry: Entry,
    now: impl Fn() -> u64,
) -> (Exit, bool)
where
    L: Link,
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    H: ServeHost<S, X, V, BootCore>,
{
    let mut link = IdleExit::new(links, now, entry);
    let exit = serve_loop(&mut link, agent, host).await;
    (exit, link.expired())
}
