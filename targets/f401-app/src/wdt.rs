//! Independent watchdog, fed by its own task only while no task has
//! overrun the deadline of the work it is in, so a stuck task resets the
//! node instead of hanging silently. Trial semantics: a trial image keeps
//! feeding for [`TRIAL_FEED`] only, so without CONFIRM it resets and the
//! boot core counts the attempt.

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_stm32::gpio::Output;
use embassy_stm32::peripherals;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};

/// How long a trial image keeps the watchdog fed while waiting for CONFIRM.
const TRIAL_FEED: Duration = Duration::from_secs(30);
const FEED_EVERY: Duration = Duration::from_millis(500);
/// Longest a task may stay inside one piece of work.
const BUSY_LIMIT: Duration = Duration::from_secs(5);

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
static BUSY: Mutex<Cs, Cell<[Option<Instant>; TASKS]>> = Mutex::new(Cell::new([None; TASKS]));

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
    let until = Instant::now() + BUSY_LIMIT;
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

/// Feeds unless a trial is past its window. Also called right before a
/// request and from long flash work, which is progress of its own.
pub fn keepalive() {
    let trial_over = Instant::now() >= Instant::from_ticks(0) + TRIAL_FEED;
    if ON_TRIAL.load(Ordering::Relaxed) && trial_over {
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
        if all_alive() {
            keepalive();
            led.toggle();
        }
        Timer::after(FEED_EVERY).await;
    }
}
