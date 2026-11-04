//! NUCLEO-F401RE demo app with DAGON: DAGON starts through the board
//! package's public `start` (the I2C target link to the 親機, update
//! receiver, trial and confirm), and the app shows its own version on the
//! X-NUCLEO-53L0A1's 4-digit 7-segment display (`110` for 1.1.0: the
//! shield has no points), stacked on the board's Arduino I2C. While DAGON
//! updates the node, the display follows the node's published events (the
//! `AppEvents` the start returns): the pattern of each update phase
//! (`f401_app::update`), then the version again. Built for
//! slot A or B (`slot-b`); the boot core picks which one runs.
//!
//! Tasks: DAGON's link runs on its own high-priority executor (`start`);
//! the display (async on the Arduino I2C, so it never holds up the link)
//! and the LED heartbeat run on the main executor below it. The display
//! announces its deadline before each update: the bound its own write
//! timeouts give (`display::SHOW_BOUND_MS`), plus the guard's margin. A
//! display stuck past it stops the feeding, and an image on trial is then
//! rolled back.
//!
//! `confirm`: the start confirms a version on trial itself (bench use
//! without a 親機), through the public start's `Confirm::Own`.
//! `no-confirm`: the self-check always fails, so CONFIRM is refused and the
//! trial rolls back (hardware tests).
#![no_std]
#![no_main]

use board::{Board, BoardDef};
use dagon_agent::snapshot::AppSnapshot;
use dagon_board_nucleo_f401re as board;
use dagon_mcu_stm32f401re::links::Kind;
use dagon_mcu_stm32f401re::node::{self, App, AppEvents, Confirm};
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Lease, Trial};
use defmt::{error, info, Display2Format};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use f401_app::{seg, update};
use panic_probe as _;

mod display;

/// How long "TEST KEY" (`tESt`) shows before the version.
const TEST_KEY_SPLASH: Duration = Duration::from_millis(1500);
/// How often the update pattern is redrawn without a new event: the
/// trial blink and the results' hold time step at this rate.
/// Demo readability value, pending owner confirmation.
const TICK: Duration = Duration::from_millis(250);

/// Watchdog guard tasks: DAGON's link, then the display.
const DISPLAY_TASK: usize = node::APP_TASKS;
const TASKS: usize = DISPLAY_TASK + 1;

/// A version on trial is never fed: unless the 親機 confirms it, the
/// watchdog resets the chip and the boot core goes back to the previous
/// version.
static GUARD: Guard<TASKS> = Guard::new(node::levels::<TASKS>(), Trial::NeverFeed, now_ms);

/// Healthy unless built with `no-confirm`.
fn self_check() -> bool {
    !cfg!(feature = "no-confirm")
}

/// Who confirms a version on trial: the 親機, or this app at start with
/// `confirm`.
const CONFIRM: Confirm = if cfg!(feature = "confirm") {
    Confirm::Own
} else {
    Confirm::ByHost
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(dagon_mcu_stm32f401re::clocks()));
    let led = Output::new(io.led, Level::Low, Speed::Low);
    let version = display::running_version();
    info!("app {} slot {}", Display2Format(&version), node::ACTIVE);
    let tag = dagon_mcu_stm32f401re::target_tag!();
    let app = App {
        confirm: CONFIRM,
        ..App::new(self_check)
    };
    let (first, then, events) = match board::start(spawner, dagon, &GUARD, tag, app, Kind::I2c) {
        Ok(events) if dagon_mcu_stm32f401re::TEST_KEY => {
            info!("TEST KEY: built with the public test verifying key");
            (seg::TEST_KEY, seg::version(version), Some(events))
        }
        Ok(events) => (seg::version(version), seg::version(version), Some(events)),
        // DAGON does not serve: the display shows dashes.
        Err(e) => {
            error!("state log unreadable: {}", defmt::Debug2Format(&e));
            (seg::DASHES, seg::DASHES, None)
        }
    };
    let lease = GUARD.lease(DISPLAY_TASK);
    spawner.spawn(display(io.arduino_i2c, first, then, events, lease).unwrap());
    spawner.spawn(heartbeat(led).unwrap());
}

/// The LED blinks as a heartbeat.
#[embassy_executor::task]
async fn heartbeat(mut led: Output<'static>) -> ! {
    loop {
        led.toggle();
        Timer::after(node::FEED_EVERY).await;
    }
}

/// Shows `first`, then `then` after the splash time (sensors kept in
/// XSHUT); then, with DAGON's `events`, each update phase's pattern
/// instead of `then` while it lasts.
#[embassy_executor::task]
async fn display(
    bus: board::ArduinoI2c,
    first: [u8; seg::DIGITS],
    then: [u8; seg::DIGITS],
    events: Option<AppEvents>,
    lease: Lease,
) {
    let mut display = display::SegDisplay::open(bus);
    show(&mut display, first, &lease).await;
    if then != first {
        Timer::after(TEST_KEY_SPLASH).await;
        show(&mut display, then, &lease).await;
    }
    let Some(mut rx) = events.and_then(|e| e.receiver()) else {
        return;
    };
    let (mut shown, mut since) = (then, Instant::now());
    let mut state: Option<AppSnapshot> = None;
    loop {
        if let Ok(latest) = with_timeout(TICK, rx.changed()).await {
            let s = latest.state;
            if state.map(|old| old.phase) != Some(s.phase) {
                since = Instant::now();
            }
            state = Some(s);
        }
        let codes = state
            .and_then(|s| update::pattern(s.phase, s.progress, since.elapsed().as_millis() as u32))
            .unwrap_or(then);
        if codes != shown {
            show(&mut display, codes, &lease).await;
            shown = codes;
        }
    }
}

async fn show(display: &mut display::SegDisplay, codes: [u8; seg::DIGITS], lease: &Lease) {
    lease.busy(display::SHOW_BOUND_MS);
    if let Err(e) = display.show(codes).await {
        error!("display: {}", defmt::Debug2Format(&e));
    }
    lease.idle();
}
