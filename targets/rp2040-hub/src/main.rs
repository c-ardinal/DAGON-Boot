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

mod gc9a01;
mod hub;
mod lcd;
mod links;
mod nodes;
mod relay;
mod state;
mod store;
mod sub;
mod wdt;

use dagon_agent::{Agent, Config, DefaultHooks, SlotStore};
use dagon_format::header::{mcu_id, port_kind, target_tag};
use dagon_proto::payload::{mode, role, SlotState, StatusResp};
use dagon_proto::{max_payload, MAX_FRAME};
use embassy_executor::{Executor, InterruptExecutor};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::spi::{self, Spi};
use embassy_rp::usb;
use embassy_rp::watchdog::Watchdog;
use embassy_rp::{bind_interrupts, dma};
use rp2040_common::arm::step;
use rp2040_common::layout::HUB_TAG;
use rp2040_common::rp::RpFlash;
use static_cell::{ConstStaticCell, StaticCell};

use gc9a01::Frame;
use relay::{I2cLink, PioStream, RpI2c, UartLink};
use store::HubStore;

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

bind_interrupts!(pub struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

const PIO_UART_BAUD: u32 = 115_200;
const I2C_HZ: u32 = 400_000;
/// Conservative for the panel wiring (the controller allows more).
const LCD_SPI_HZ: u32 = 20_000_000;

static EXECUTOR_PC: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();
static SERIAL: StaticCell<[u8; 16]> = StaticCell::new();
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
    step(10);
    let p = embassy_rp::init(Default::default());
    step(11);

    let mut store = HubStore {
        flash: RpFlash::new(p.FLASH),
    };
    let st = store.status();
    wdt::install(Watchdog::new(p.WATCHDOG), !on_trial(&st));
    let serial = SERIAL.init(hex(&store.flash.unique_id()));
    let serial = core::str::from_utf8(serial).unwrap_or("0");

    let sub = sub::SubLines::new(p.PIN_18, p.PIN_28);
    // Touch controller held out of reset and otherwise left alone.
    let _touch_rst = Output::new(p.PIN_22, Level::High);

    let Pio {
        mut common,
        sm0,
        sm1,
        ..
    } = Pio::new(p.PIO0, Irqs);
    let tx_prog = PioUartTxProgram::new(&mut common);
    let rx_prog = PioUartRxProgram::new(&mut common);
    let tx = PioUartTx::new(PIO_UART_BAUD, &mut common, sm0, p.PIN_26, &tx_prog);
    let rx = PioUartRx::new(PIO_UART_BAUD, &mut common, sm1, p.PIN_27, &rx_prog);
    let mut i2c_cfg = i2c::Config::default();
    i2c_cfg.frequency = I2C_HZ;
    let i2c = I2c::new_async(p.I2C0, p.PIN_17, p.PIN_16, Irqs, i2c_cfg);

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = LCD_SPI_HZ;
    let lcd_pins = lcd::Pins {
        spi: Spi::new_txonly(p.SPI1, p.PIN_10, p.PIN_11, p.DMA_CH0, Irqs, spi_cfg),
        cs: Output::new(p.PIN_9, Level::High),
        dc: Output::new(p.PIN_8, Level::Low),
        rst: Output::new(p.PIN_13, Level::High),
        // Off until the first full frame is on the panel.
        backlight: Output::new(p.PIN_25, Level::Low),
    };

    step(12);
    let agent = Agent::new(
        Config {
            pubkey: PUBKEY,
            mcu_id: mcu_id::RP2040,
            port_kind: port_kind::SELF_BOOT,
            target_tag: target_tag(HUB_TAG),
            role: role::HUB,
            mode: mode::APP,
            max_payload: max_payload::USB as u16,
        },
        store,
        DefaultHooks,
    );
    let pc = hub::Pc {
        agent,
        sub,
        resp: [0; MAX_FRAME],
    };

    interrupt::SWI_IRQ_0.set_priority(Priority::P1);
    let high = EXECUTOR_PC.start(interrupt::SWI_IRQ_0);
    high.spawn(hub::task(p.USB, serial, pc).expect("task pool"));
    high.spawn(links::i2c_task(I2cLink::new(RpI2c(i2c))).expect("task pool"));
    high.spawn(links::uart_task(UartLink::new(PioStream { tx, rx })).expect("task pool"));

    let frame = FRAME.take();
    EXECUTOR_LOW.init(Executor::new()).run(|low| {
        low.spawn(lcd::task(lcd_pins, frame).expect("task pool"));
        low.spawn(wdt::task().expect("task pool"));
    })
}

fn on_trial(st: &StatusResp) -> bool {
    st.slots.get(st.active as usize).map(|s| s.state) == Some(SlotState::Trying)
}

fn hex(id: &[u8; 8]) -> [u8; 16] {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; 16];
    for (i, b) in id.iter().enumerate() {
        out[2 * i] = DIGITS[(b >> 4) as usize];
        out[2 * i + 1] = DIGITS[(b & 0xF) as usize];
    }
    out
}
