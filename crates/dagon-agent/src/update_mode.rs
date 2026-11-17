//! A boot core's receiving (DES-RB-030): the listen window right after its
//! start, and update mode, where the boot core's session ([`Agent`] with
//! [`crate::writer::BootCore`]) answers every request until REBOOT or
//! [`EXIT_IDLE_MS`] without one, or recovery mode, which only REBOOT ends
//! ([`Entry`] says which, and why). Between events both sleep in the link's
//! wait ([`Port::wait`]: the MCU package wakes it on the link's interrupt
//! or event and on a timer), never longer than the next deadline or the
//! feed interval, and feed the watchdog at every wake; the MCU package
//! gives the port, the clock and the feed.

use dagon_events::Clock;
use dagon_format::SigVerifier;
use dagon_proto::payload::mode;
use dagon_proto::{flags, frame_len, Cmd, Frame};

use crate::{Agent, AppChain, SlotStore, Writer, IDLE_TIMEOUT_MS};

/// Update mode ends after this long without a request (a normal restart).
pub const EXIT_IDLE_MS: u32 = 60_000;

const US_PER_MS: u64 = 1_000;
const STAY_LEN: usize = frame_len(0);

/// Why the boot core serves its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A STAY came in the listen window.
    Stay,
    /// The app handed over (REBOOT mode 1, an UPDATE_STAY the boot core
    /// consumed with UPDATE_DONE).
    HandOver,
    /// Nothing can be started (the state log is unreadable or no slot
    /// passes its check).
    Recovery,
}

impl Entry {
    /// HELLO's mode in this entry (`payload::mode`).
    pub const fn mode(self) -> u8 {
        match self {
            Entry::Stay | Entry::HandOver => mode::UPDATE,
            Entry::Recovery => mode::RECOVERY,
        }
    }

    /// Whether the boot core leaves after [`EXIT_IDLE_MS`] without a
    /// request. Recovery mode has nothing to start, so it stays.
    pub const fn leaves_when_idle(self) -> bool {
        !matches!(self, Entry::Recovery)
    }
}

/// A link a boot core serves between sleeps.
pub trait Port {
    /// Serves what the link has pending; returns a request frame that has
    /// arrived whole. Further requests wait (NACKed or unread) until
    /// [`Port::reply`].
    fn poll(&mut self) -> Option<&[u8]>;
    /// Holds the reply to the last request (`None`: no answer) for the
    /// peer to read; `reboot`: [`Port::delivered`] tells when it was read.
    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool);
    /// The reply held with `reboot` has been read whole.
    fn delivered(&mut self) -> bool;
    /// Sleeps until the link has something to serve or `max_us` have
    /// passed (it may wake earlier; the caller checks the time again).
    fn wait(&mut self, max_us: u32);
}

/// The STAY the listen window took, for update mode to answer.
pub struct Stay {
    frame: [u8; STAY_LEN],
}

impl Stay {
    pub fn frame(&self) -> &[u8] {
        &self.frame
    }
}

/// Why [`serve`] returned; either way the boot core restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The reply to REBOOT has been read.
    Reboot,
    /// No request for [`EXIT_IDLE_MS`].
    Idle,
}

fn stay(request: &[u8]) -> Option<Stay> {
    let (f, _) = Frame::decode(request, request.len()).ok()?;
    let is_stay = f.cmd == Cmd::Stay as u8 && f.flags & flags::RESP == 0 && f.payload.is_empty();
    let frame = request.get(..STAY_LEN)?.try_into().ok()?;
    is_stay.then_some(Stay { frame })
}

/// Sleeps on `port` for the `left_us` to the next deadline, at most the
/// feed interval `feed_every_ms`.
fn sleep<P: Port>(port: &mut P, left_us: u64, feed_every_ms: u32) {
    let most = u64::from(feed_every_ms) * US_PER_MS;
    // Both bounds fit: `most` comes from a u32 of ms below 4,295 s.
    port.wait(u32::try_from(left_us.min(most)).unwrap_or(u32::MAX));
}

/// The listen window of `window_ms` from now: the first STAY that arrives
/// in it keeps the boot core (update mode answers it); any other request
/// gets no answer. A window of 0 opens none. `feed` runs at every wake,
/// at least every `feed_every_ms`.
pub fn listen<P: Port, C: Clock>(
    port: &mut P,
    clock: &mut C,
    window_ms: u16,
    feed_every_ms: u32,
    feed: &mut impl FnMut(),
) -> Option<Stay> {
    if window_ms == 0 {
        return None;
    }
    let end = clock.now_us() + u64::from(window_ms) * US_PER_MS;
    loop {
        feed();
        if let Some(request) = port.poll() {
            if let Some(s) = stay(request) {
                return Some(s);
            }
            port.reply(None, false);
            continue;
        }
        let now = clock.now_us();
        if now >= end {
            return None;
        }
        sleep(port, end - now, feed_every_ms);
    }
}

/// Update mode: `agent` answers `first` (the window's STAY), then every
/// request `port` brings, until the reply to REBOOT has been read or
/// [`EXIT_IDLE_MS`] pass without a request. A transfer left for
/// [`IDLE_TIMEOUT_MS`] is dropped. `feed` runs at every wake, at least
/// every `feed_every_ms`.
pub fn serve<P, S, X, V, W, C>(
    port: &mut P,
    agent: &mut Agent<S, X, V, W>,
    clock: &mut C,
    feed_every_ms: u32,
    feed: &mut impl FnMut(),
    first: Option<&[u8]>,
) -> Exit
where
    P: Port,
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    W: Writer,
    C: Clock,
{
    serve_in(port, agent, clock, feed_every_ms, feed, first, Entry::Stay)
}

/// [`serve`] in `entry`'s mode: recovery mode does not leave after
/// [`EXIT_IDLE_MS`], only on REBOOT.
pub fn serve_in<P, S, X, V, W, C>(
    port: &mut P,
    agent: &mut Agent<S, X, V, W>,
    clock: &mut C,
    feed_every_ms: u32,
    feed: &mut impl FnMut(),
    mut first: Option<&[u8]>,
    entry: Entry,
) -> Exit
where
    P: Port,
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    W: Writer,
    C: Clock,
{
    let abort_us = u64::from(IDLE_TIMEOUT_MS) * US_PER_MS;
    let exit_us = u64::from(EXIT_IDLE_MS) * US_PER_MS;
    let mut last = clock.now_us();
    let mut transfer = true;
    let mut reboot = false;
    loop {
        feed();
        let request = match first.take() {
            Some(r) => Some(r),
            None => port.poll(),
        };
        if let Some(request) = request {
            last = clock.now_us();
            transfer = true;
            let reply = agent.handle(request);
            reboot = reply.as_ref().is_some_and(|r| r.reboot);
            port.reply(reply.map(|r| r.frame), reboot);
            continue;
        }
        if reboot && port.delivered() {
            agent.before_reboot();
            return Exit::Reboot;
        }
        let quiet = clock.now_us().saturating_sub(last);
        if transfer && quiet >= abort_us {
            agent.abort();
            transfer = false;
        }
        if entry.leaves_when_idle() && quiet >= exit_us {
            return Exit::Idle;
        }
        let next = match (transfer, entry.leaves_when_idle()) {
            (true, _) => abort_us,
            (false, true) => exit_us,
            // Nothing to wait for but the next request.
            (false, false) => quiet + u64::from(feed_every_ms) * US_PER_MS,
        };
        sleep(port, next - quiet, feed_every_ms);
    }
}
