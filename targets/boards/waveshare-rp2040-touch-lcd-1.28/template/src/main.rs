//! Start-up written by `dagon new`: the board, the DAGON node (USB link,
//! update receiver, trial and confirm) and your application in app.rs. You
//! do not need to change this file.
#![no_std]
#![no_main]

mod app;
#[allow(dead_code)]
mod wiring;

use dagon_board_waveshare_rp2040_touch_lcd_1_28::{self as board, Board, BoardDef};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Level, Trial};
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
    board::start(spawner, dagon, &GUARD, "{{tag}}", app::self_check);
    app::start(spawner, io, GUARD.lease(1));
}
