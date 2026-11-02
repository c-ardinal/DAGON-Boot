//! DAGON hub app (RP2040-Touch-LCD-1.28).
//!
//! USB CDC carries DAGON frames from the PC. RELAY goes to the F401 on I2C0
//! or the watch on the PIO UART, SHOW drives the LCD progress screen, SUBCTL
//! and I2C_RAW drive the F401 ROM path; everything else (HELLO, STATUS,
//! XFER_*, SET_PENDING, REBOOT, CONFIRM) is the hub's own agent, which
//! updates the inactive app slot. While the PC is quiet, the hub polls each
//! node with a HELLO every 2 s and shows the result on the LCD home screen.
//! Tasks and priorities: see `main`.
#![no_std]
#![no_main]

mod hub;
mod lcd;
mod links;
mod nodes;
mod state;

use board::{hold_touch, Board, BoardDef};
use dagon_board_waveshare_rp2040_touch_lcd_1_28 as board;
use dagon_mcu_rp2040::arm::{led, step};
use dagon_mcu_rp2040::layout::HUB_TAG;
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::uart::UartLink;
use dagon_mcu_rp2040::watchdog::{feed_forever, now_ms, Guard, Level, Trial};
use dagon_proto::payload::role;
use dagon_proto::MAX_FRAME;
use embassy_executor::{Executor, InterruptExecutor};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIO0};
use embassy_rp::{bind_interrupts, dma, i2c, pio};
use static_cell::{ConstStaticCell, StaticCell};

use board::gc9a01::{self, Frame};

bind_interrupts!(pub struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

/// Watchdog guard tasks: PC, I2C link and UART link on the preempting
/// executor, the LCD in thread mode.
pub mod task {
    pub const PC: usize = 0;
    pub const I2C: usize = 1;
    pub const UART: usize = 2;
    pub const LCD: usize = 3;
}

/// Self-feeding window on trial; longer than the host's 45 s reconnect limit.
const TRIAL_FEED_MS: u64 = 60_000;

pub static GUARD: Guard<4> = Guard::new(
    [
        Level::Preempting,
        Level::Preempting,
        Level::Preempting,
        Level::Background,
    ],
    Trial::FeedUntilMs(TRIAL_FEED_MS),
    now_ms,
);

static EXECUTOR_PC: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();
/// The LCD frame buffer, placed statically (never built on the stack).
static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame([0; gc9a01::FRAME_BYTES]));

#[interrupt]
unsafe fn SWI_IRQ_0() {
    // SAFETY: the executor's own interrupt; started in main before use.
    unsafe { EXECUTOR_PC.on_interrupt() }
}

/// Task layout (each peripheral owned by exactly one task):
/// - interrupt executor on SWI_IRQ_0 (priority P1, preempts thread mode):
///   USB device; PC requests (owns flash/agent and the F401 control
///   lines); I2C0 link (owns I2C0); PIO UART link (owns PIO0 UART). The
///   links serve RELAY/I2C_RAW through channels and poll their nodes.
/// - thread executor (lowest): LCD (owns SPI1 + DMA_CH0 and the panel
///   pins); watchdog feeding gated on every task's liveness.
#[cortex_m_rt::entry]
fn main() -> ! {
    // Panic blink codes: 10 before HAL init, 11 after it, 12 at USB setup,
    // 13 polling nodes, 14 answering the PC, 15 drawing.
    led::select(board::DIAG_LED);
    step(10);
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    step(11);

    let mut agent = node::open::<Board, 4>(
        dagon.flash,
        dagon.watchdog,
        &GUARD,
        task::PC,
        HUB_TAG,
        role::HUB,
        node::always,
    );
    let serial = node::serial(&mut agent.store().flash);
    let sub = io.sub_ctl.lines();
    let _touch_rst = hold_touch(io.touch_rst);
    let uart = UartLink::new(io.sub_uart.stream(Irqs), now_ms as fn() -> u64);
    let i2c = io.sub_i2c.link(Irqs);
    let lcd_pins = io.display.pins(Irqs);

    step(12);
    let pc = hub::Pc {
        agent,
        sub,
        resp: [0; MAX_FRAME],
    };

    interrupt::SWI_IRQ_0.set_priority(Priority::P1);
    let high = EXECUTOR_PC.start(interrupt::SWI_IRQ_0);
    high.spawn(hub::task(dagon.usb, serial, pc).expect("task pool"));
    high.spawn(links::i2c_task(i2c).expect("task pool"));
    high.spawn(links::uart_task(uart).expect("task pool"));

    let frame = FRAME.take();
    EXECUTOR_LOW.init(Executor::new()).run(|low| {
        low.spawn(lcd::task(lcd_pins, frame).expect("task pool"));
        low.spawn(feed().expect("task pool"));
    })
}

#[embassy_executor::task]
async fn feed() -> ! {
    feed_forever(&GUARD).await
}
