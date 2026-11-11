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
mod wiring;

use board::Board;
use dagon_mcu_rp2040::links::{self, Kind};
use dagon_mcu_rp2040::relay::{self, Setup};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
use dagon_mcu_rp2040::BoardDef;
use embassy_executor::Spawner;
use panic_halt as _;

/// The hops of this node, from the 構成 (dagon gen writes them): the
/// build stops here, naming the hop, if a kind is not available yet.
#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);

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
    let setup = Setup {
        // DAGON_TARGET_TAG in .cargo/config.toml (the boot core uses it too).
        tag: dagon_mcu_rp2040::target_tag!(),
        // This node's index in the 構成 (wiring.rs).
        index: wiring::NODE.index,
        // The role the 構成 gives this node (wiring.rs).
        role: if wiring::NODE.hub {
            relay::role::HUB
        } else {
            relay::role::NODE
        },
        name: "{{node}}",
        app: app::dagon(),
        state: relay::new_state(),
        relay: Default::default(),
    };
    let (links, spare) = board::links(io, UPLINK, DOWNLINKS);
    relay::start::<Board, TASKS>(spawner, dagon, links, &GUARD, setup);
    app::start(spawner, spare, &GUARD, relay::TASKS);
}
