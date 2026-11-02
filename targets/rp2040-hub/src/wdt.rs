//! Watchdog: the boot core armed it for 8 s. It is fed only through
//! [`feed`], which feeds only while every task is within the deadline of
//! its current work ([`rp2040_common::liveness`]): the PC and link tasks
//! run on the preempting interrupt executor, the LCD in thread mode, and
//! background deadlines are frozen while the preempting level works (an
//! erase of a whole slot runs there without yielding). A stuck task
//! therefore ends in a reset instead of a silent hang. On trial the app
//! feeds only during the first 60 s after boot, so an unconfirmed app
//! resets and the boot core counts another attempt.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_rp::watchdog::Watchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use rp2040_common::layout::WDT_MS;
use rp2040_common::liveness::{Level, Liveness};

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
static LIVE: Mutex<CriticalSectionRawMutex, RefCell<Liveness<TASKS>>> =
    Mutex::new(RefCell::new(Liveness::new([
        Level::Preempting,
        Level::Preempting,
        Level::Preempting,
        Level::Background,
    ])));

fn now_ms() -> u64 {
    Instant::now().as_millis()
}

pub fn install(wdt: Watchdog, confirmed: bool) {
    CONFIRMED.store(confirmed, Ordering::Relaxed);
    WDT.lock(|w| *w.borrow_mut() = Some(wdt));
    feed();
}

pub fn set_confirmed() {
    CONFIRMED.store(true, Ordering::Relaxed);
}

/// `task` starts (or makes progress in) a piece of work.
pub fn busy(task: Task) {
    busy_for(task, BUSY_LIMIT - BUSY_MARGIN);
}

/// `task` starts a piece of work bounded by `bound` (a bus request with
/// its own timeout).
pub fn busy_for(task: Task, bound: Duration) {
    let ms = (bound + BUSY_MARGIN).as_millis();
    LIVE.lock(|l| l.borrow_mut().busy(task as usize, now_ms(), ms));
}

/// `task` is waiting for its next piece of work.
pub fn idle(task: Task) {
    LIVE.lock(|l| l.borrow_mut().idle(task as usize, now_ms()));
}

/// Feeds if every task is alive and the trial window (if on trial) is
/// open. The only way the watchdog is fed.
pub fn feed() {
    let now = Instant::now();
    let trial_open = now < Instant::from_ticks(0) + TRIAL_FEED;
    let allowed = CONFIRMED.load(Ordering::Relaxed) || trial_open;
    if !allowed || !LIVE.lock(|l| l.borrow().alive(now.as_millis())) {
        return;
    }
    WDT.lock(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.feed(Duration::from_millis(WDT_MS as u64));
        }
    });
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
        feed();
        Timer::after(FEED_EVERY).await;
    }
}
