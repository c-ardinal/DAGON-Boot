//! DAGON hub app (RP2040-Touch-LCD-1.28): the 親機 relay of the RP2040
//! package (`dagon_mcu_rp2040::relay`) on the Waveshare board package, with
//! the round display's home screen.
//!
//! USB CDC carries DAGON frames from the PC. RELAY goes to the F401 on I2C0
//! or the watch on the PIO UART, SHOW drives the LCD progress screen, SUBCTL
//! and I2C_RAW drive the F401 ROM path; everything else (HELLO, STATUS,
//! XFER_*, SET_PENDING, REBOOT, CONFIRM) is the hub's own agent, which
//! updates the inactive app slot. While the PC is quiet, the hub polls each
//! node with a HELLO every 2 s and shows the result on the LCD home screen.
//!
//! Task layout (each peripheral owned by exactly one task):
//! - DAGON's interrupt executor on SWI_IRQ_0 (priority P1, preempts thread
//!   mode): USB device; PC requests (owns flash/agent and the F401 control
//!   lines); I2C0 link (owns I2C0); PIO UART link (owns PIO0 UART). The
//!   links serve RELAY/I2C_RAW through channels and poll their nodes.
//! - thread executor (lowest): LCD (owns SPI1 + DMA_CH0 and the panel
//!   pins); watchdog feeding gated on every task's liveness.
#![no_std]
#![no_main]

use board::{Board, BoardDef};
use dagon_board_waveshare_rp2040_touch_lcd_1_28 as board;
use dagon_mcu_rp2040::arm::{led, step};
use dagon_mcu_rp2040::links::Kind;
use dagon_mcu_rp2040::node::{always, App};
use dagon_mcu_rp2040::relay::{self, named, RelayConfig, SubNode};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};
use dagon_proto::payload::link;
use embassy_executor::Executor;
use static_cell::StaticCell;

/// Self-feeding window on trial; longer than the host's 45 s reconnect limit.
const TRIAL_FEED_MS: u64 = 60_000;

/// Guard tasks: PC, I2C link, UART link (preempting), LCD (thread mode).
static GUARD: Guard<{ board::APP_TASKS }> = Guard::new(
    relay::levels::<{ board::APP_TASKS }>(),
    Trial::FeedUntilMs(TRIAL_FEED_MS),
    now_ms,
);

static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

/// The desk demo's wiring, shown and polled until a topology display copy
/// is stored.
static DESK_DEMO: [SubNode; 2] = [
    named(1, "seg", link::I2C0, 0x42),
    named(2, "watch", link::PIO_UART, link::UART_ADDR),
];

#[cortex_m_rt::entry]
fn main() -> ! {
    // Panic blink codes: 10 before HAL init, 11 after it, 12 at USB setup,
    // 13 polling nodes, 14 answering the PC, 15 drawing.
    led::select(board::DIAG_LED, board::DIAG_LED_ACTIVE_HIGH);
    step(10);
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    step(11);
    let cfg = RelayConfig {
        tag: dagon_mcu_rp2040::target_tag!(),
        name: "DAGON hub",
        fallback: &DESK_DEMO,
        app: App::new(always),
    };
    EXECUTOR_LOW.init(Executor::new()).run(|low| {
        step(12);
        board::start(
            low,
            dagon,
            io,
            &GUARD,
            cfg,
            Kind::UsbCdc,
            &[Kind::I2c, Kind::Uart],
        );
    })
}
