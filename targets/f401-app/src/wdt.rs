//! Independent watchdog, fed only through [`feed`], which feeds only while
//! every task is within the deadline of its current work
//! ([`rp2040_common::liveness`], board-independent): mailbox serving runs on
//! the preempting interrupt executor, the display in thread mode, and the
//! display's deadline is frozen while serving works. A stuck task therefore
//! resets the node instead of hanging silently. A trial image is never fed:
//! without CONFIRM it resets when the trial watchdog (4 s) runs out, and
//! the boot core counts the attempt (rolling back after the third, about
//! 15 s). Feeding starts once the image is confirmed.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_stm32::gpio::Output;
use embassy_stm32::peripherals;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use rp2040_common::liveness::{Level, Liveness};

const FEED_EVERY: Duration = Duration::from_millis(500);
/// Longest a task may stay inside one piece of work.
const BUSY_LIMIT_MS: u64 = 5_000;

#[derive(Clone, Copy)]
pub enum Task {
    /// Mailbox serving (I2C target and agent).
    Serve = 0,
    /// Display multiplexing.
    Display = 1,
}

const TASKS: usize = 2;

type Wdt = IndependentWatchdog<'static, peripherals::IWDG>;

static WDT: Mutex<Cs, RefCell<Option<Wdt>>> = Mutex::new(RefCell::new(None));
static ON_TRIAL: AtomicBool = AtomicBool::new(false);
static LIVE: Mutex<Cs, RefCell<Liveness<TASKS>>> = Mutex::new(RefCell::new(Liveness::new([
    Level::Preempting,
    Level::Background,
])));

fn now_ms() -> u64 {
    Instant::now().as_millis()
}

pub fn install(mut wdt: Wdt, on_trial: bool) {
    ON_TRIAL.store(on_trial, Ordering::Relaxed);
    wdt.unleash();
    WDT.lock(|w| *w.borrow_mut() = Some(wdt));
}

/// The running image is (no longer) on trial.
pub fn set_trial(on_trial: bool) {
    ON_TRIAL.store(on_trial, Ordering::Relaxed);
}

/// `task` starts (or makes progress in) a piece of work.
pub fn busy(task: Task) {
    LIVE.lock(|l| l.borrow_mut().busy(task as usize, now_ms(), BUSY_LIMIT_MS));
}

/// `task` is waiting for its next piece of work.
pub fn idle(task: Task) {
    LIVE.lock(|l| l.borrow_mut().idle(task as usize, now_ms()));
}

/// Feeds if the image is not on trial and every task is alive. The only
/// way the watchdog is fed (also right before a request and from long
/// flash work, which renew their own deadline first).
pub fn feed() {
    if ON_TRIAL.load(Ordering::Relaxed) || !LIVE.lock(|l| l.borrow().alive(now_ms())) {
        return;
    }
    WDT.lock(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.pet();
        }
    });
}

/// Feeds while every task is alive; the LED blinks as a heartbeat.
#[embassy_executor::task]
pub async fn task(mut led: Output<'static>) -> ! {
    loop {
        feed();
        led.toggle();
        Timer::after(FEED_EVERY).await;
    }
}
