//! Hub bring-up: backlight blink, USB CDC console, PIO UART echo, and an
//! I2C `Get` to the STM32 ROM bootloader.
//!
//! Console keys: `h` help, `g` enter F401 ROM + Get, `r` reset F401 to flash,
//! `t` send a test line on the PIO UART, `e` toggle PIO UART echo.
#![no_std]
#![no_main]

use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_futures::join::{join, join3};
use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Flex, Level, Output};
use embassy_rp::i2c::{self, Async, I2c};
use embassy_rp::peripherals::{I2C0, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::usb::{self, Driver};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use heapless::String;
use panic_halt as _;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

const PIO_UART_BAUD: u32 = 115_200;
const I2C_HZ: u32 = 100_000;
const ROM_ADDR: u8 = 0x39;
const ROM_ACK: u8 = 0x79;
const NRST_PULSE: Duration = Duration::from_millis(20);
/// Time for the F401 to reach its ROM bootloader after NRST is released.
const ROM_STARTUP: Duration = Duration::from_millis(100);
/// Get returns N, version and N commands; read a bit more than the F401 list.
const GET_FRAME_LEN: usize = 32;
const BLINK: Duration = Duration::from_millis(500);

static PIO_ECHO: AtomicBool = AtomicBool::new(true);
static PIO_SEND_TEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PIO_RX_LOG: Channel<CriticalSectionRawMutex, u8, 64> = Channel::new();

type Line = String<160>;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut backlight = Output::new(p.PIN_25, Level::High);
    let blink = async {
        loop {
            Timer::after(BLINK).await;
            backlight.toggle();
        }
    };

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
    let mut nrst = Flex::new(p.PIN_18);
    nrst.set_low();
    nrst.set_as_input();
    let boot0 = Output::new(p.PIN_28, Level::Low);
    let mut sub = Subordinate { i2c, nrst, boot0 };

    let mut usb_cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
    usb_cfg.manufacturer = Some("DAGON");
    usb_cfg.product = Some("DAGON hub bring-up");
    usb_cfg.serial_number = Some("BRINGUP");
    let mut config_desc = [0; 256];
    let mut bos_desc = [0; 256];
    let mut control_buf = [0; 64];
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        Driver::new(p.USB, Irqs),
        usb_cfg,
        &mut config_desc,
        &mut bos_desc,
        &mut [],
        &mut control_buf,
    );
    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let mut usb = builder.build();

    let console = async {
        loop {
            class.wait_connection().await;
            let _ = session(&mut class, &mut sub).await;
        }
    };

    join(usb.run(), join3(blink, pio_uart(tx, rx), console)).await;
}

async fn pio_uart(mut tx: PioUartTx<'_, PIO0, 0>, mut rx: PioUartRx<'_, PIO0, 1>) {
    loop {
        match select(rx.read_u8(), PIO_SEND_TEST.wait()).await {
            Either::First(b) => {
                if PIO_ECHO.load(Ordering::Relaxed) {
                    tx.write_u8(b).await;
                }
                let _ = PIO_RX_LOG.try_send(b);
            }
            Either::Second(()) => {
                for &b in b"DAGON pio uart test\r\n" {
                    tx.write_u8(b).await;
                }
            }
        }
    }
}

struct Subordinate<'d> {
    i2c: I2c<'d, I2C0, Async>,
    nrst: Flex<'d>,
    boot0: Output<'d>,
}

impl Subordinate<'_> {
    async fn reset(&mut self, rom: bool) {
        self.boot0
            .set_level(if rom { Level::High } else { Level::Low });
        // Open-drain style: drive low, then release to the board's pull-up.
        self.nrst.set_as_output();
        Timer::after(NRST_PULSE).await;
        self.nrst.set_as_input();
    }

    /// ROM `Get` (0x00 0xFF): ACK frame, data frame, ACK frame.
    async fn rom_get(&mut self, out: &mut Line) {
        self.reset(true).await;
        Timer::after(ROM_STARTUP).await;
        if let Err(e) = self.i2c.write_async(ROM_ADDR, [0x00, 0xFF]).await {
            let _ = write!(out, "get: write NACK/err {:?}\r\n", e);
            return;
        }
        let mut ack = [0u8; 1];
        let mut data = [0u8; GET_FRAME_LEN];
        let mut ack2 = [0u8; 1];
        let r = async {
            self.i2c.read_async(ROM_ADDR, &mut ack).await?;
            if ack[0] != ROM_ACK {
                return Ok(());
            }
            self.i2c.read_async(ROM_ADDR, &mut data).await?;
            self.i2c.read_async(ROM_ADDR, &mut ack2).await
        }
        .await;
        let _ = write!(out, "get: ack={:02X}", ack[0]);
        if let Err(e) = r {
            let _ = write!(out, " err {:?}\r\n", e);
            return;
        }
        if ack[0] != ROM_ACK {
            let _ = write!(out, " (expected 79)\r\n");
            return;
        }
        let n = (data[0] as usize + 1).min(GET_FRAME_LEN - 1);
        let _ = write!(out, " ver={:02X} cmds=", data[1]);
        for b in &data[2..=n] {
            let _ = write!(out, "{:02X} ", b);
        }
        let _ = write!(out, "ack2={:02X}\r\n", ack2[0]);
    }
}

async fn session<'d>(
    class: &mut CdcAcmClass<'d, Driver<'d, USB>>,
    sub: &mut Subordinate<'_>,
) -> Result<(), EndpointError> {
    send(class, HELP).await?;
    let mut buf = [0u8; 64];
    loop {
        let mut out = Line::new();
        match select(class.read_packet(&mut buf), PIO_RX_LOG.receive()).await {
            Either::First(n) => {
                for &c in &buf[..n?] {
                    command(c, sub, &mut out).await;
                }
            }
            Either::Second(b) => {
                let _ = write!(out, "pio rx: {:02X}\r\n", b);
            }
        }
        send(class, out.as_bytes()).await?;
    }
}

const HELP: &[u8] = b"DAGON hub bring-up\r\n\
h help | g F401 ROM Get | r F401 reset to flash | t PIO UART test | e toggle echo\r\n";

async fn command(c: u8, sub: &mut Subordinate<'_>, out: &mut Line) {
    match c {
        b'g' => sub.rom_get(out).await,
        b'r' => {
            sub.reset(false).await;
            let _ = write!(out, "f401 reset to flash\r\n");
        }
        b't' => {
            PIO_SEND_TEST.signal(());
            let _ = write!(out, "pio tx: test line sent\r\n");
        }
        b'e' => {
            let on = !PIO_ECHO.load(Ordering::Relaxed);
            PIO_ECHO.store(on, Ordering::Relaxed);
            let _ = write!(out, "pio echo {}\r\n", if on { "on" } else { "off" });
        }
        b'h' | b'?' => {
            let _ = out.push_str(core::str::from_utf8(HELP).unwrap_or(""));
        }
        _ => {}
    }
}

async fn send<'d>(
    class: &mut CdcAcmClass<'d, Driver<'d, USB>>,
    data: &[u8],
) -> Result<(), EndpointError> {
    for chunk in data.chunks(64) {
        class.write_packet(chunk).await?;
    }
    if data.len().is_multiple_of(64) && !data.is_empty() {
        class.write_packet(&[]).await?;
    }
    Ok(())
}
