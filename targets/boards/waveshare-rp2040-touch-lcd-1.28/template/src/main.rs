//! Start-up written by `dagon new`: the board, the DAGON node (PC link,
//! relaying to the downstream nodes, recovery) and your application in app.rs, which
//! gets the display's pins with the other spare ones. You do not need to change this file.
//!
//! DAGON's relay runs on a high-priority interrupt executor; your tasks in
//! app.rs run on the main executor below it, so they never delay the relay.
#![no_std]
#![no_main]

mod app;
mod wiring;

use dagon_board_waveshare_rp2040_touch_lcd_1_28::{self as board, Board, BoardDef};
use dagon_mcu_rp2040::clock::Setup as Clock;
use dagon_mcu_rp2040::links::{self, Kind};
use dagon_mcu_rp2040::relay::{self, Setup};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
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

/// The clock this node runs on, from its board's `[clock]` and the MCU's
/// `[[app.clock]]` (wiring.rs): the build stops here if the uplink needs a
/// clock the board does not give.
const CLOCK: Clock = dagon_mcu_rp2040::clock_setup!(wiring::clock);
dagon_mcu_rp2040::require_uplink_clock!(CLOCK, UPLINK, "{{node}}");

/// Watchdog guard tasks: the relay's, then app.rs's.
const TASKS: usize = relay::TASKS + app::TASKS;

/// A version on trial keeps feeding for 60 s after boot; unless the PC
/// confirms it in that time, the watchdog resets the chip and the previous
/// version comes back.
static GUARD: Guard<TASKS> =
    Guard::new(relay::levels::<TASKS>(), Trial::FeedUntilMs(60_000), now_ms);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(CLOCK.hal_config()));
    let setup = Setup {
        // DAGON_TARGET_TAG in .cargo/config.toml (the boot core uses it too).
        tag: dagon_mcu_rp2040::target_tag!(),
        // The role the 構成 gives this node (wiring.rs).
        role: if wiring::NODE.hub {
            relay::role::HUB
        } else {
            relay::role::NODE
        },
        name: "{{node}}",
        app: app::dagon(),
        relay: Default::default(),
    };
    let (spare, _events) = board::start(spawner, dagon, io, &GUARD, setup, UPLINK, DOWNLINKS);
    app::start(spawner, spare, &GUARD, relay::TASKS);
}
