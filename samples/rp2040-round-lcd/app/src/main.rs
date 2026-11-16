//! Sample app on the Waveshare RP2040-Touch-LCD-1.28: the relaying node of
//! the sample topology (`wiring.rs`, written by `dagon gen`), which draws
//! its own round LCD.
//!
//! DAGON runs the relay (`dagon_mcu_rp2040::relay`, through the board
//! package's `start`): USB CDC carries DAGON frames from the PC; RELAY goes
//! to the nodes on I2C0 or the PIO UART, SUBCTL and I2C_RAW drive an STM32
//! node's ROM path, everything else (SHOW included) is this node's own
//! agent. The app gets the display's pins and the node's events back and
//! draws the home screen (`display`) from those events and the nodes
//! wiring.rs says its links reach.
//!
//! Task layout (each peripheral owned by exactly one task):
//! - DAGON's interrupt executor on SWI_IRQ_0 (priority P1, preempts thread
//!   mode): USB device; PC requests; one task per link.
//! - thread executor (lowest): the LCD (owns SPI1, DMA_CH0 and the panel
//!   pins); watchdog feeding gated on every task's liveness.
#![no_std]
#![no_main]

mod display;
// Written by `dagon gen` from the sample topology; kept as it writes it.
#[rustfmt::skip]
mod wiring;

use board::{Board, BoardDef};
use dagon_board_waveshare_rp2040_touch_lcd_1_28 as board;
use dagon_mcu_rp2040::arm::{led, step};
use dagon_mcu_rp2040::clock::Setup as Clock;
use dagon_mcu_rp2040::links::Kind;
use dagon_mcu_rp2040::node::{always, App};
use dagon_mcu_rp2040::relay::{self, Setup};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
use embassy_executor::Executor;
use static_cell::StaticCell;

/// Self-feeding window on trial; longer than the host's 45 s reconnect limit.
const TRIAL_FEED_MS: u64 = 60_000;

/// The display's guard task, after the relay's.
const DISPLAY_TASK: usize = relay::TASKS;
/// Guard tasks: the relay's (preempting), the display's (thread mode).
const TASKS: usize = DISPLAY_TASK + 1;

static GUARD: Guard<TASKS> = Guard::new(
    relay::levels::<TASKS>(),
    Trial::FeedUntilMs(TRIAL_FEED_MS),
    now_ms,
);

static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

/// The clock this node runs on (wiring.rs, from its board's `[clock]` and
/// the MCU's `[[app.clock]]`); the PC link needs its USB clock.
const CLOCK: Clock = dagon_mcu_rp2040::clock_setup!(wiring::clock);
dagon_mcu_rp2040::require_uplink_clock!(CLOCK, Kind::UsbCdc, "rp2040-round-lcd");

#[cortex_m_rt::entry]
fn main() -> ! {
    // Panic blink codes: 10 before HAL init, 11 after it, 12 at USB setup,
    // 14 answering the PC, 15 drawing.
    led::select(board::DIAG_LED, board::DIAG_LED_ACTIVE_HIGH);
    step(10);
    let (dagon, io) = Board::split(embassy_rp::init(CLOCK.hal_config()));
    step(11);
    let setup = Setup {
        tag: dagon_mcu_rp2040::target_tag!(),
        role: if wiring::NODE.hub {
            relay::role::HUB
        } else {
            relay::role::NODE
        },
        // The USB product name the CLI finds the connected node by.
        name: "DAGON hub",
        app: App::new(always),
        relay: Default::default(),
    };
    EXECUTOR_LOW.init(Executor::new()).run(|low| {
        step(12);
        let (spare, events) = board::start(
            low,
            dagon,
            io,
            &GUARD,
            setup,
            Kind::UsbCdc,
            &[Kind::I2c, Kind::Uart],
        );
        let pins = spare.display.pins(board::Irqs);
        low.spawn(display::task(pins, events, GUARD.lease(DISPLAY_TASK)).expect("task pool"));
    })
}
