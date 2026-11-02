//! Start-up written by `dagon new`: your board (board.rs), the DAGON node
//! (USB link, update receiver, trial and confirm) and your application in
//! app.rs. You do not need to change this file.
#![no_std]
#![no_main]

mod app;
mod board;
#[allow(dead_code)]
mod wiring;

use board::Board;
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Level, Trial};
use dagon_mcu_rp2040::BoardDef;
use embassy_executor::Spawner;
use panic_halt as _;

/// Watchdog guard: task 0 serves DAGON, task 1 is the app. A version on
/// trial keeps feeding for 60 s after boot; unless the host confirms it in
/// that time, the watchdog resets the chip and the previous version comes
/// back.
static GUARD: Guard<2> = Guard::new(
    [Level::Preempting, Level::Background],
    Trial::FeedUntilMs(60_000),
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    node::start::<Board, 2>(spawner, dagon, &GUARD, "{{tag}}", app::self_check);
    app::start(spawner, io, GUARD.lease(1));
}
