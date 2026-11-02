//! Start-up written by `dagon new`: the board, the DAGON node (I2C link to
//! the hub, update receiver, trial and confirm) and your application in
//! app.rs. You do not need to change this file.
#![no_std]
#![no_main]

mod app;
#[allow(dead_code)]
mod wiring;

use dagon_board_nucleo_f401re::{self as board, Board, BoardDef};
use dagon_mcu_stm32f401re::clocks;
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Level, Trial};
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};

/// Watchdog guard: task 0 serves DAGON, task 1 is the app. A version on
/// trial is never fed: unless the hub confirms it, the watchdog resets the
/// chip and the boot core goes back to the previous version.
static GUARD: Guard<2> = Guard::new(
    [Level::Preempting, Level::Background],
    Trial::NeverFeed,
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(clocks()));
    // An unreadable state log: nothing starts and the watchdog resets.
    board::start(spawner, dagon, &GUARD, "{{tag}}", app::self_check).ok();
    app::start(spawner, io, GUARD.lease(1));
}
