//! NUCLEO-F401RE demo app with DAGON: DAGON starts through the board
//! package's public `start` (the I2C target link to the 親機, update
//! receiver, trial and confirm), and the app shows its own version on the
//! X-NUCLEO-53L0A1's 4-digit 7-segment display (`110` for 1.1.0: the
//! shield has no points), stacked on the board's Arduino I2C. Built for
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
//! `no-confirm`: the self-check always fails, so CONFIRM is refused and the
//! trial rolls back (hardware tests).
#![no_std]
#![no_main]

use board::{Board, BoardDef};
use dagon_board_nucleo_f401re as board;
use dagon_mcu_stm32f401re::links::Kind;
use dagon_mcu_stm32f401re::node;
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Lease, Trial};
use defmt::{error, info, Display2Format};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Duration, Timer};
use f401_app::seg;
use panic_probe as _;

mod display;

/// How long "TEST KEY" (`tESt`) shows before the version.
const TEST_KEY_SPLASH: Duration = Duration::from_millis(1500);

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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(dagon_mcu_stm32f401re::clocks()));
    let led = Output::new(io.led, Level::Low, Speed::Low);
    let version = display::running_version();
    info!("app {} slot {}", Display2Format(&version), node::ACTIVE);
    let tag = dagon_mcu_stm32f401re::target_tag!();
    let (first, then) = match board::start(spawner, dagon, &GUARD, tag, self_check, Kind::I2c) {
        Ok(()) if dagon_mcu_stm32f401re::TEST_KEY => {
            info!("TEST KEY: built with the public test verifying key");
            (seg::TEST_KEY, seg::version(version))
        }
        Ok(()) => (seg::version(version), seg::version(version)),
        // DAGON does not serve: the display shows dashes.
        Err(e) => {
            error!("state log unreadable: {}", defmt::Debug2Format(&e));
            (seg::DASHES, seg::DASHES)
        }
    };
    spawner.spawn(display(io.arduino_i2c, first, then, GUARD.lease(DISPLAY_TASK)).unwrap());
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
/// XSHUT).
#[embassy_executor::task]
async fn display(
    bus: board::ArduinoI2c,
    first: [u8; seg::DIGITS],
    then: [u8; seg::DIGITS],
    lease: Lease,
) {
    let mut display = display::SegDisplay::open(bus);
    show(&mut display, first, &lease).await;
    if then != first {
        Timer::after(TEST_KEY_SPLASH).await;
        show(&mut display, then, &lease).await;
    }
}

async fn show(display: &mut display::SegDisplay, codes: [u8; seg::DIGITS], lease: &Lease) {
    lease.busy(display::SHOW_BOUND_MS);
    if let Err(e) = display.show(codes).await {
        error!("display: {}", defmt::Debug2Format(&e));
    }
    lease.idle();
}
