//! Start-up written by `dagon new`: your board (board.rs), the DAGON node
//! (I2C link to the 親機, update receiver, trial and confirm) and your
//! application in app.rs. You do not need to change this file.
//!
//! DAGON's link runs on a high-priority interrupt executor; your tasks in
//! app.rs run on the main executor below it, so the 親機 is answered
//! whatever they do.
#![no_std]
#![no_main]

mod app;
mod board;
mod wiring;

use board::Board;
use dagon_mcu_stm32f401re::clock::Setup;
use dagon_mcu_stm32f401re::links::{self, Kind};
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Trial};
use dagon_mcu_stm32f401re::{node, BoardDef};
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};

/// The hops of this node, from the 構成 (dagon gen writes them): the
/// build stops here, naming the hop, if a kind is not available yet.
#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);

/// The clock this node runs on, from its board's `[clock]` and the MCU's
/// `[[app.clock]]` (wiring.rs).
const CLOCK: Setup = dagon_mcu_stm32f401re::clock_setup!(wiring::clock);

/// The node's 7-bit I2C address on the 親機's bus (from the 構成; a node
/// without a board has no default).
const I2C_ADDR: u8 = match wiring::LINK_ADDR {
    Some(addr) => addr,
    None => panic!("the 構成 gives this node no I2C address"),
};

/// Watchdog guard tasks: DAGON's link, then app.rs's.
const TASKS: usize = node::APP_TASKS + app::TASKS;

/// A version on trial is never fed: unless the 親機 confirms it, the
/// watchdog resets the chip and the boot core goes back to the previous
/// version.
static GUARD: Guard<TASKS> = Guard::new(node::levels::<TASKS>(), Trial::NeverFeed, now_ms);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(CLOCK.hal_config()));
    // DAGON_TARGET_TAG in .cargo/config.toml (the boot core uses it too).
    let tag = dagon_mcu_stm32f401re::target_tag!();
    // An unreadable state log: nothing starts and the watchdog resets.
    node::start::<Board, TASKS>(spawner, dagon, &GUARD, tag, app::dagon(), UPLINK, I2C_ADDR).ok();
    app::start(spawner, io, &GUARD, node::APP_TASKS);
}
