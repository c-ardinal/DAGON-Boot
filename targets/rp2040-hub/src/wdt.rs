//! Watchdog: the boot core armed it for 8 s. A dedicated task feeds it
//! every second, but only while no task has overrun the deadline of the
//! piece of work it is in (default [`BUSY_LIMIT`], or the bound it declared
//! for a long bus request); a stuck task therefore ends in a reset instead
//! of a silent hang. On trial the app feeds only during the
//! first 60 s after boot, so an unconfirmed app resets and the boot core
//! counts another attempt.

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_rp::watchdog::Watchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use rp2040_common::layout::WDT_MS;

/// Self-feeding window on trial; longer than the host's 45 s reconnect limit.
const TRIAL_FEED: Duration = Duration::from_secs(60);
const FEED_EVERY: Duration = Duration::from_secs(1);
/// Longest a task may stay inside one piece of work it did not bound.
const BUSY_LIMIT: Duration = Duration::from_secs(5);
/// Slack on a declared bound.
const BUSY_MARGIN: Duration = Duration::from_secs(2);

/// Tasks whose liveness gates the feeding.
#[derive(Clone, Copy)]
pub enum Task {
    Pc = 0,
    I2c = 1,
    Uart = 2,
    Lcd = 3,
}

const TASKS: usize = 4;

static WDT: Mutex<CriticalSectionRawMutex, RefCell<Option<Watchdog>>> =
    Mutex::new(RefCell::new(None));
static CONFIRMED: AtomicBool = AtomicBool::new(false);
/// By when each task must finish its current piece of work (`None`:
/// waiting for work).
static BUSY: Mutex<CriticalSectionRawMutex, Cell<[Option<Instant>; TASKS]>> =
    Mutex::new(Cell::new([None; TASKS]));

pub fn install(wdt: Watchdog, confirmed: bool) {
    CONFIRMED.store(confirmed, Ordering::Relaxed);
    WDT.lock(|w| *w.borrow_mut() = Some(wdt));
    keepalive();
}

pub fn set_confirmed() {
    CONFIRMED.store(true, Ordering::Relaxed);
}

/// `task` starts (or makes progress in) a piece of work.
pub fn busy(task: Task) {
    busy_for(task, BUSY_LIMIT);
}

/// `task` starts a piece of work bounded by `bound` (a bus request with
/// its own timeout).
pub fn busy_for(task: Task, bound: Duration) {
    let until = Instant::now() + bound + BUSY_MARGIN;
    BUSY.lock(|b| {
        let mut all = b.get();
        all[task as usize] = Some(until);
        b.set(all);
    });
}

/// `task` is waiting for its next piece of work.
pub fn idle(task: Task) {
    BUSY.lock(|b| {
        let mut all = b.get();
        all[task as usize] = None;
        b.set(all);
    });
}

fn all_alive() -> bool {
    let now = Instant::now();
    BUSY.lock(|b| b.get())
        .iter()
        .all(|t| t.is_none_or(|until| now < until))
}

/// Feeds unless the trial window is over. Also called between flash
/// sectors of a long erase, which is progress of its own.
pub fn keepalive() {
    let trial_open = Instant::now() < Instant::from_ticks(0) + TRIAL_FEED;
    if CONFIRMED.load(Ordering::Relaxed) || trial_open {
        WDT.lock(|w| {
            if let Some(w) = w.borrow_mut().as_mut() {
                w.feed(Duration::from_millis(WDT_MS as u64));
            }
        });
    }
}

/// Full chip reset (USB drops, the boot chain runs again).
pub fn reboot() -> ! {
    WDT.lock(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.trigger_reset();
        }
    });
    loop {
        cortex_m::asm::nop();
    }
}

#[embassy_executor::task]
pub async fn task() -> ! {
    loop {
        if all_alive() {
            keepalive();
        }
        Timer::after(FEED_EVERY).await;
    }
}
