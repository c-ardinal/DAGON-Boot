//! What the boot core does around its session (design DES-RB-030): the
//! listen window, in which a STAY keeps it in update mode before any
//! session exists, and the update mode's exit after [`UPDATE_IDLE_MS`]
//! without a request. Every MCU's boot core runs these over its own
//! [`Link`]s; the session itself is the [`crate::Agent`] with the
//! [`crate::writer::BootCore`] writer, built only once the boot core
//! receives ([`Entry`]).

use dagon_port::{Link, LinkError, LinkKind, Reconnect};
use dagon_proto::payload::mode;
use dagon_proto::{flags, Cmd, Frame, Status, HEADER_LEN};

/// With no request for this long, a boot core that entered update mode by
/// STAY or by the app's hand-over restarts normally (design DES-RB-030).
pub const UPDATE_IDLE_MS: u64 = 60_000;

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

    /// Whether the boot core leaves after [`UPDATE_IDLE_MS`] without a
    /// request. Recovery mode has nothing to start, so it stays.
    pub const fn leaves_when_idle(self) -> bool {
        !matches!(self, Entry::Recovery)
    }
}

/// The listen window on `link`: until `window_ms` after this call (on the
/// clock `now`, ms), a STAY is answered OK and ends the window with `true`
/// (the boot core enters update mode); any other request gets no reply,
/// since no session exists yet. A window of 0 reads nothing. The window
/// ends within one read tick of the link after its time, so a link whose
/// read gives up every 10 ms or sooner adds at most the window and 10 ms
/// to the boot.
pub async fn listen<L: Link>(link: &mut L, window_ms: u16, now: impl Fn() -> u64) -> bool {
    if window_ms == 0 {
        return false;
    }
    let end = now() + u64::from(window_ms);
    let max = link.max_payload();
    let mut reply = [0u8; HEADER_LEN + 1 + dagon_proto::CRC_LEN];
    while now() < end {
        let stay = match link.read_frame().await {
            Ok(f) => match Frame::decode(f, max) {
                Ok((r, _)) if r.cmd == Cmd::Stay as u8 && r.payload.is_empty() => Some(r.seq),
                _ => None,
            },
            Err(LinkError::Timeout) => continue,
            Err(_) => return false,
        };
        let Some(seq) = stay else {
            if link.no_reply().await.is_err() {
                return false;
            }
            continue;
        };
        reply[HEADER_LEN] = Status::Ok as u8;
        let Ok(n) = dagon_proto::finish(&mut reply, flags::RESP, seq, Cmd::Stay as u8, 1) else {
            return false;
        };
        return link.write_frame(&reply[..n]).await.is_ok();
    }
    false
}

/// The link the boot core serves its session over: `L`, closed once
/// [`UPDATE_IDLE_MS`] passed without a frame when the entry leaves when
/// idle ([`Entry::leaves_when_idle`]), so the serving loop returns and the
/// boot core restarts normally ([`IdleExit::expired`]). Needs a link whose
/// read gives up from time to time (its tick).
pub struct IdleExit<L, C> {
    link: L,
    now: C,
    last: u64,
    leaves: bool,
    expired: bool,
}

impl<L: Link, C: Fn() -> u64> IdleExit<L, C> {
    pub fn new(link: L, now: C, entry: Entry) -> Self {
        let last = now();
        IdleExit {
            link,
            now,
            last,
            leaves: entry.leaves_when_idle(),
            expired: false,
        }
    }

    /// The link closed because no request came for [`UPDATE_IDLE_MS`].
    pub fn expired(&self) -> bool {
        self.expired
    }
}

impl<L: Link, C: Fn() -> u64> Link for IdleExit<L, C> {
    fn max_payload(&self) -> usize {
        self.link.max_payload()
    }
    fn reconnect(&self) -> Reconnect {
        self.link.reconnect()
    }
    fn kind(&self) -> LinkKind {
        self.link.kind()
    }
    fn addr(&self) -> u8 {
        self.link.addr()
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        let read = self.link.read_frame().await;
        let now = (self.now)();
        match read {
            Ok(f) => {
                self.last = now;
                Ok(f)
            }
            Err(LinkError::Timeout)
                if self.leaves && now.saturating_sub(self.last) >= UPDATE_IDLE_MS =>
            {
                self.expired = true;
                Err(LinkError::Closed)
            }
            Err(e) => Err(e),
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.link.write_frame(frame).await
    }
    async fn no_reply(&mut self) -> Result<(), LinkError> {
        self.link.no_reply().await
    }
    async fn flush(&mut self) -> Result<(), LinkError> {
        self.link.flush().await
    }
}
