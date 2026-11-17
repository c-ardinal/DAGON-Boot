//! A boot core's receiving (DES-RB-030): the listen window right after its
//! start, and update mode, where the boot core's session ([`Agent`] with
//! [`crate::writer::BootCore`]) answers every request until REBOOT or
//! [`EXIT_IDLE_MS`] without one. A boot core runs without interrupts, so
//! both poll the link ([`Port`]) and feed the watchdog at every step; the
//! MCU package gives the port, the clock and the feed.

use dagon_events::Clock;
use dagon_format::SigVerifier;
use dagon_proto::{flags, frame_len, Cmd, Frame};

use crate::{Agent, AppChain, SlotStore, Writer, IDLE_TIMEOUT_MS};

/// Update mode ends after this long without a request (a normal restart).
pub const EXIT_IDLE_MS: u32 = 60_000;

const US_PER_MS: u64 = 1_000;
const STAY_LEN: usize = frame_len(0);

/// A link a boot core polls.
pub trait Port {
    /// Serves the bus once; returns a request frame that has arrived whole.
    /// Further requests wait (NACKed or unread) until [`Port::reply`].
    fn poll(&mut self) -> Option<&[u8]>;
    /// Holds the reply to the last request (`None`: no answer) for the
    /// peer to read; `reboot`: [`Port::delivered`] tells when it was read.
    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool);
    /// The reply held with `reboot` has been read whole.
    fn delivered(&mut self) -> bool;
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

/// The listen window of `window_ms` from now: the first STAY that arrives
/// in it keeps the boot core (update mode answers it); any other request
/// gets no answer. A window of 0 opens none.
pub fn listen<P: Port, C: Clock>(
    port: &mut P,
    clock: &mut C,
    window_ms: u16,
    feed: &mut impl FnMut(),
) -> Option<Stay> {
    if window_ms == 0 {
        return None;
    }
    let end = clock.now_us() + u64::from(window_ms) * US_PER_MS;
    while clock.now_us() < end {
        feed();
        let Some(request) = port.poll() else {
            continue;
        };
        if let Some(s) = stay(request) {
            return Some(s);
        }
        port.reply(None, false);
    }
    None
}

/// Update mode: `agent` answers `first` (the window's STAY), then every
/// request `port` brings, until the reply to REBOOT has been read or
/// [`EXIT_IDLE_MS`] pass without a request. A transfer left for
/// [`IDLE_TIMEOUT_MS`] is dropped.
pub fn serve<P, S, X, V, W, C>(
    port: &mut P,
    agent: &mut Agent<S, X, V, W>,
    clock: &mut C,
    feed: &mut impl FnMut(),
    mut first: Option<&[u8]>,
) -> Exit
where
    P: Port,
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    W: Writer,
    C: Clock,
{
    let mut last = clock.now_us();
    let mut transfer = true;
    let mut reboot = false;
    loop {
        feed();
        if reboot && port.delivered() {
            agent.before_reboot();
            return Exit::Reboot;
        }
        let now = clock.now_us();
        let request = match first.take() {
            Some(r) => Some(r),
            None => port.poll(),
        };
        let Some(request) = request else {
            let quiet = now.saturating_sub(last);
            if transfer && quiet >= u64::from(IDLE_TIMEOUT_MS) * US_PER_MS {
                agent.abort();
                transfer = false;
            }
            if quiet >= u64::from(EXIT_IDLE_MS) * US_PER_MS {
                return Exit::Idle;
            }
            continue;
        };
        last = now;
        transfer = true;
        let reply = agent.handle(request);
        reboot = reply.as_ref().is_some_and(|r| r.reboot);
        port.reply(reply.map(|r| r.frame), reboot);
    }
}
