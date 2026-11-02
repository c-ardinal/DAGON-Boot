//! DAGON hub app (RP2040-Touch-LCD-1.28).
//!
//! USB CDC carries DAGON frames from the PC. RELAY goes to the F401 on I2C0
//! or the watch on the PIO UART; everything else (HELLO, STATUS, XFER_*,
//! SET_PENDING, REBOOT, CONFIRM) is the hub's own agent, which updates the
//! inactive app slot. While nobody relays, the hub polls each node with a
//! HELLO every 2 s and shows the result on the LCD home screen.
#![no_std]
#![no_main]

mod lcd;
mod nodes;
mod relay;
mod store;
mod sub;
mod wdt;

use dagon_agent::{Agent, Config, DefaultHooks, SlotStore};
use dagon_format::header::{mcu_id, port_kind, target_tag};
use dagon_proto::payload::{mode, role, Relay, SlotState, StatusResp};
use dagon_proto::{flags, max_payload, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C0, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram};
use embassy_rp::spi::{self, Spi};
use embassy_rp::usb::{self, Driver};
use embassy_rp::watchdog::Watchdog;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use panic_halt as _;
use rp2040_common::frames::FrameReader;
use rp2040_common::layout::HUB_TAG;
use rp2040_common::rp::RpFlash;

use lcd::{Home, Node, Shade, HOME};
use relay::{Links, PioStream, RpI2c};
use store::{HubStore, ACTIVE};

include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

const USB_VID: u16 = 0xc0de;
const USB_PID: u16 = 0xcafe;
const USB_PACKET: u16 = 64;
const PIO_UART_BAUD: u32 = 115_200;
const I2C_HZ: u32 = 400_000;
const LCD_SPI_HZ: u32 = 40_000_000;
const POLL_EVERY: Duration = Duration::from_secs(2);
/// No own polling for this long after a RELAY (the PC drives the nodes).
const QUIET_AFTER_RELAY: Duration = Duration::from_secs(10);
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// Lets the REBOOT reply leave before the reset drops USB.
const REBOOT_DELAY: Duration = Duration::from_millis(50);

type Cdc<'d> = CdcAcmClass<'d, Driver<'d, USB>>;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut store = HubStore {
        flash: RpFlash::new(p.FLASH),
    };
    let st = store.status();
    wdt::install(Watchdog::new(p.WATCHDOG), !on_trial(&st));
    let serial_hex = hex(&store.flash.unique_id());
    let serial = core::str::from_utf8(&serial_hex).unwrap_or("0");

    let _sub = sub::SubLines::new(p.PIN_18, p.PIN_28);
    let _backlight = Output::new(p.PIN_25, Level::High);

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
        spi: Spi::new_blocking_txonly(p.SPI1, p.PIN_10, p.PIN_11, spi_cfg),
        cs: Output::new(p.PIN_9, Level::High),
        dc: Output::new(p.PIN_8, Level::Low),
        rst: Output::new(p.PIN_13, Level::High),
    };

    let mut usb_cfg = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_cfg.manufacturer = Some("DAGON");
    usb_cfg.product = Some("DAGON hub");
    usb_cfg.serial_number = Some(serial);
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
    let mut class = CdcAcmClass::new(&mut builder, &mut state, USB_PACKET);
    let mut usb = builder.build();

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
    let mut hub = Hub {
        agent,
        links: Links::new(RpI2c(i2c), PioStream { tx, rx }),
        resp: [0; MAX_FRAME],
        quiet_until: Instant::now(),
        poll_seq: 0x80,
        home: Home::default(),
    };

    join4(
        wdt::run(),
        usb.run(),
        hub.serve(&mut class),
        lcd::run(lcd_pins),
    )
    .await;
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

struct Hub<'d> {
    agent: Agent<HubStore, DefaultHooks>,
    links: Links<RpI2c<'d>, PioStream<'d>>,
    resp: [u8; MAX_FRAME],
    quiet_until: Instant,
    poll_seq: u8,
    home: Home,
}

impl Hub<'_> {
    async fn serve(&mut self, class: &mut Cdc<'_>) -> ! {
        let mut rx = FrameReader::<{ MAX_FRAME + USB_PACKET as usize }>::new(max_payload::USB);
        let mut pkt = [0u8; USB_PACKET as usize];
        let mut next_poll = Instant::now();
        loop {
            match select(recv(class, &mut pkt), Timer::at(next_poll)).await {
                Either::First(Ok(n)) => {
                    let mut rest = &pkt[..n];
                    while !rest.is_empty() {
                        let k = rx.push(rest);
                        rest = &rest[k..];
                        while let Some(len) = rx.poll() {
                            let (out, reboot) = self.handle(rx.frame(len)).await;
                            rx.consume(len);
                            let _ = send(class, &self.resp[..out]).await;
                            if reboot {
                                Timer::after(REBOOT_DELAY).await;
                                wdt::reboot();
                            }
                        }
                    }
                }
                Either::First(Err(_)) => rx.clear(),
                Either::Second(()) => {
                    if Instant::now() >= self.quiet_until {
                        self.poll().await;
                    }
                    next_poll = Instant::now() + POLL_EVERY;
                }
            }
        }
    }

    /// Answers one request into `self.resp`; returns its length and whether
    /// to reboot after sending it.
    async fn handle(&mut self, request: &[u8]) -> (usize, bool) {
        let Ok((f, _)) = Frame::decode(request, max_payload::USB) else {
            return (0, false);
        };
        if f.cmd != Cmd::Relay as u8 || f.flags & (flags::RESP | flags::EVENT) != 0 {
            return match self.agent.handle(request) {
                Some(reply) => {
                    let n = reply.frame.len();
                    self.resp[..n].copy_from_slice(reply.frame);
                    (n, reply.reboot)
                }
                None => (0, false),
            };
        }
        self.quiet_until = Instant::now() + QUIET_AFTER_RELAY;
        let body = match Relay::parse(f.payload) {
            Ok(r) => self.relay(&r).await,
            Err(s) => self.status_only(s),
        };
        let n = dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, body).unwrap_or(0);
        (n, false)
    }

    /// RELAY response payload in `self.resp[HEADER_LEN..]`: status, then the
    /// node's response frame.
    async fn relay(&mut self, r: &Relay<'_>) -> usize {
        let Some(max) = relay::link_max(r.link) else {
            return self.status_only(Status::Arg);
        };
        match Frame::decode(r.inner, max) {
            Ok((_, n)) if n == r.inner.len() => {}
            _ => return self.status_only(Status::Arg),
        }
        let out = &mut self.resp[HEADER_LEN + 1..];
        match self
            .links
            .exchange(r.link, r.addr, r.inner, r.timeout_ms, out)
            .await
        {
            Ok(n) => {
                self.resp[HEADER_LEN] = Status::Ok as u8;
                1 + n
            }
            Err(s) => self.status_only(s),
        }
    }

    fn status_only(&mut self, s: Status) -> usize {
        self.resp[HEADER_LEN] = s as u8;
        1
    }

    /// Display-only HELLO to every node, then refresh the LCD.
    async fn poll(&mut self) {
        let st = self.agent.store().status();
        let subs = nodes::load(&mut self.agent.store().flash);
        let me = st.slots[ACTIVE as usize];
        self.home.hub = Node {
            node: nodes::HUB,
            version: Some(me.version),
            shade: if me.state == SlotState::Trying {
                Shade::Trial
            } else {
                Shade::Confirmed
            },
        };
        self.home.subs.clear();
        for sub in subs {
            self.poll_seq = self.poll_seq.wrapping_add(1) | 0x80;
            let hello = self.links.hello(sub.link, sub.addr, self.poll_seq).await;
            let (version, shade) = match hello {
                Some(h) if h.trial => (Some(h.active_version), Shade::Trial),
                Some(h) => (Some(h.active_version), Shade::Confirmed),
                None => (None, Shade::NoAnswer),
            };
            let _ = self.home.subs.push(Node {
                node: sub,
                version,
                shade,
            });
        }
        HOME.signal(self.home.clone());
    }
}

async fn recv(class: &mut Cdc<'_>, pkt: &mut [u8]) -> Result<usize, EndpointError> {
    class.wait_connection().await;
    class.read_packet(pkt).await
}

/// Writes one frame; gives up if the host stops reading.
async fn send(class: &mut Cdc<'_>, data: &[u8]) -> Result<(), EndpointError> {
    if data.is_empty() {
        return Ok(());
    }
    let write = async {
        for chunk in data.chunks(USB_PACKET as usize) {
            class.write_packet(chunk).await?;
        }
        if data.len().is_multiple_of(USB_PACKET as usize) {
            class.write_packet(&[]).await?;
        }
        Ok(())
    };
    with_timeout(USB_WRITE_TIMEOUT, write)
        .await
        .unwrap_or(Err(EndpointError::Disabled))
}
