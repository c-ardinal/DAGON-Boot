//! Start-up written by `dagon new`: your board (board.rs), the DAGON 親機
//! (PC link, relaying to the 子機) and your application in app.rs. You do
//! not need to change this file.
//!
//! DAGON's relay runs on a high-priority interrupt executor; your tasks in
//! app.rs run on the main executor below it, so they never delay the relay.
#![no_std]
#![no_main]

mod app;
mod board;
#[allow(dead_code)]
mod wiring;

use board::Board;
use dagon_mcu_rp2040::relay::{self, RelayConfig};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
use dagon_mcu_rp2040::BoardDef;
use embassy_executor::Spawner;
use panic_halt as _;

/// Watchdog guard tasks: the relay's, then app.rs's.
const TASKS: usize = relay::TASKS + app::TASKS;

/// A version on trial keeps feeding for 60 s after boot; unless the PC
/// confirms it in that time, the watchdog resets the chip and the previous
/// version comes back.
static GUARD: Guard<TASKS> =
    Guard::new(relay::levels::<TASKS>(), Trial::FeedUntilMs(60_000), now_ms);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    let cfg = RelayConfig {
        // DAGON_TARGET_TAG in .cargo/config.toml (the boot core uses it too).
        tag: dagon_mcu_rp2040::TARGET_TAG,
        name: "{{node}}",
        fallback: &[],
        self_check: app::self_check,
    };
    let (links, spare) = board::links(io);
    relay::start::<Board, TASKS>(spawner, dagon, links, &GUARD, cfg);
    app::start(spawner, spare, &GUARD, relay::TASKS);
}
