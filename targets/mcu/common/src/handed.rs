//! The start of a boot core whose stage0 starts the app on its request
//! (`dagon_core::bootcore::start_handed`: an MCU whose running boot core
//! cannot map the app over its own image, so it asks by reset through the
//! retained cell). Before it asks, the core settles its own trial and
//! decides whether it receives (design DES-RB-030), the same order as on
//! the MCUs whose boot core starts the app itself:
//!
//! 1. its trial (`settle_log` over the boot core log), confirmed once it
//!    runs: the core that reaches its start is fit to ask for the app;
//! 2. stage0's result in the cell (`hand_off`): stage0's decision started
//!    no app, so the core clears the cell and serves recovery mode;
//! 3. the app's hand-over (an UPDATE_STAY in the app log, consumed with
//!    UPDATE_DONE): update mode;
//! 4. a STAY within the listen window: update mode, the STAY answered
//!    first;
//! 5. otherwise [`START_APP`] (word 0, then word 1): the core resets and
//!    stage0 runs the app decision once.
//!
//! After serving (REBOOT, or update mode's idle exit) the core resets
//! without a request, so the next start comes back here: the listen window
//! again, then the request.

use dagon_agent::update_mode::{self, Entry, Port, Stay};
use dagon_core::bootcore::{
    cell_holds, hand_off, settle_log, Cell, HandOff, Settled, CLEARED, START_APP,
};
use dagon_core::log::{Log, LogLayout};
use dagon_events::Clock;
use dagon_port::Flash;

/// What the boot core does after [`begin`].
pub enum Next {
    /// Serves its session in this entry's mode (update mode answers the
    /// window's STAY first), then resets.
    Serve(Entry, Option<Stay>),
    /// [`START_APP`] is in the cell: reset, and stage0 starts the app.
    AskForApp,
    /// Its trial did not settle (`MustReset`): reset, and stage0 counts
    /// the start.
    Reset,
}

/// Where the boot core keeps its records and finds the app's, and how
/// many starts a pending core gets.
#[derive(Clone, Copy)]
pub struct Logs<'a> {
    /// The boot core log (its trial).
    pub core: &'a LogLayout,
    /// The app log (the app's UPDATE_STAY).
    pub app: &'a LogLayout,
    /// The boot core slot this core runs from.
    pub running: u8,
    /// Starts a pending core gets (`trial_attempts`).
    pub max: u8,
}

/// The retained cell: read whole, written one word per
/// `write(index, word)`.
pub struct CellIo<R, W> {
    pub read: R,
    pub write: W,
}

/// The listen window: on `port`, for `window_ms` of `clock` (0: none),
/// feeding at every wake and at least every `feed_every_ms`.
pub struct Window<'a, P, C, D> {
    pub port: &'a mut P,
    pub clock: &'a mut C,
    pub window_ms: u16,
    pub feed_every_ms: u32,
    pub feed: &'a mut D,
}

/// The boot core's start, steps 1 to 5 of this module.
pub fn begin<F, R, W, P, C, D>(
    flash: &mut F,
    logs: Logs<'_>,
    mut cell: CellIo<R, W>,
    w: Window<'_, P, C, D>,
) -> Next
where
    F: Flash,
    R: Fn() -> [u32; 2],
    W: FnMut(usize, u32),
    P: Port,
    C: Clock,
    D: FnMut(),
{
    let seen = (cell.read)();
    let holds = cell_holds(&cell.read, &mut cell.write);
    let mut put = |v: [u32; 2]| {
        v.into_iter()
            .enumerate()
            .for_each(|(i, x)| (cell.write)(i, x))
    };
    let at_start = Cell {
        value: seen,
        max: logs.max,
        holds,
    };
    match settle_log(flash, logs.core, logs.running, at_start, true) {
        Ok(Settled::Nothing) => {}
        // The count is spent; a result stage0 left is read from `seen`.
        Ok(Settled::Confirmed | Settled::RolledBack) => put(CLEARED),
        Err(_) => return Next::Reset,
    }
    if let HandOff::Stay(_) = hand_off(seen) {
        put(CLEARED);
        return Next::Serve(Entry::Recovery, None);
    }
    if Log::load(flash, logs.app).is_ok_and(|mut l| l.take_stay(flash) == Ok(true)) {
        return Next::Serve(Entry::HandOver, None);
    }
    if let Some(s) = update_mode::listen(w.port, w.clock, w.window_ms, w.feed_every_ms, w.feed) {
        return Next::Serve(Entry::Stay, Some(s));
    }
    put(START_APP);
    Next::AskForApp
}
