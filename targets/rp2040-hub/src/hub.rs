//! The PC task (high-priority interrupt executor): DAGON frames from USB
//! CDC. RELAY and I2C_RAW go to the link task that owns the bus, SHOW to
//! the shared display state, SUBCTL to the F401 control lines (owned
//! here); everything else is the hub's own agent (owned here, with the
//! flash).

use portable_atomic::{AtomicU32, Ordering};

use dagon_agent::{Agent, DefaultHooks, SlotStore};
use dagon_port::{Nrst, SubControl};
use dagon_proto::payload::{boot0, link, nrst, Relay, Show, SlotState, SubCtl};
use dagon_proto::{flags, max_payload, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};
use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::UsbDevice;
use heapless::Vec;
use rp2040_common::arm::step;
use rp2040_common::cmd;
use rp2040_common::frames::FrameReader;
use static_cell::StaticCell;

use crate::lcd::{Progress, Shade};
use crate::links::{Op, Port, Req, I2C_PORT, UART_PORT};
use crate::state::{self, Seen};
use crate::store::{HubStore, ACTIVE};
use crate::sub::SubLines;
use crate::wdt::{self, Task};
use crate::{nodes, Irqs};

pub const USB_PACKET: u16 = 64;
const USB_VID: u16 = 0xc0de;
const USB_PID: u16 = 0xcafe;

static USB_BUFS: StaticCell<([u8; 256], [u8; 256], [u8; 64])> = StaticCell::new();
static USB_STATE: StaticCell<State> = StaticCell::new();
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// Lets the REBOOT reply leave before the reset drops USB.
const REBOOT_DELAY: Duration = Duration::from_millis(50);
/// I2C_RAW target: the STM32 ROM bootloader.
const ROM_ADDR: u8 = 0x39;
/// Bound on one I2C_RAW request at the link task.
const RAW_TIMEOUT_MS: u16 = 1_000;

pub type Cdc = CdcAcmClass<'static, Driver<'static, USB>>;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub struct Pc {
    pub agent: Agent<HubStore, DefaultHooks>,
    pub sub: SubLines<'static>,
    pub resp: [u8; MAX_FRAME],
}

/// Runs inside the high-priority executor: builds the USB device there
/// (its types stay in that executor), starts it, then serves the PC.
#[embassy_executor::task]
pub async fn task(usb: Peri<'static, USB>, serial: &'static str, pc: Pc) -> ! {
    let mut cfg = embassy_usb::Config::new(USB_VID, USB_PID);
    cfg.manufacturer = Some("DAGON");
    cfg.product = Some("DAGON hub");
    cfg.serial_number = Some(serial);
    let (config_desc, bos_desc, control_buf) = USB_BUFS.init(([0; 256], [0; 256], [0; 64]));
    let mut builder = embassy_usb::Builder::new(
        Driver::new(usb, Irqs),
        cfg,
        config_desc,
        bos_desc,
        &mut [],
        control_buf,
    );
    let class = CdcAcmClass::new(&mut builder, USB_STATE.init(State::new()), USB_PACKET);
    let device = builder.build();
    // SAFETY: awaited directly inside this embassy task, whose context
    // comes from the interrupt executor running it.
    let spawner = unsafe { Spawner::for_current_executor() }.await;
    spawner.spawn(usb_task(device).expect("task pool"));
    serve(pc, class).await
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}

async fn serve(mut pc: Pc, mut class: Cdc) -> ! {
    pc.publish(true);
    let mut rx = FrameReader::<{ MAX_FRAME + USB_PACKET as usize }>::new(max_payload::USB);
    let mut pkt = [0u8; USB_PACKET as usize];
    loop {
        wdt::idle(Task::Pc);
        let got = recv(&mut class, &mut pkt).await;
        wdt::busy(Task::Pc);
        let Ok(n) = got else {
            rx.clear();
            continue;
        };
        let mut rest = &pkt[..n];
        while !rest.is_empty() {
            let k = rx.push(rest);
            rest = &rest[k..];
            while let Some(len) = rx.poll() {
                let (out, reboot) = pc.handle(rx.frame(len)).await;
                rx.consume(len);
                let _ = send(&mut class, &pc.resp[..out]).await;
                if reboot {
                    Timer::after(REBOOT_DELAY).await;
                    wdt::reboot();
                }
            }
        }
    }
}

impl Pc {
    /// Shares the hub's own state and, if `subs`, the node list (topology
    /// display copy) with the other tasks.
    fn publish(&mut self, subs: bool) {
        let st = self.agent.store().status();
        let me = st.slots[ACTIVE as usize];
        state::set_hub(Seen {
            version: Some(me.version),
            shade: if me.state == SlotState::Trying {
                Shade::Trial
            } else {
                Shade::Confirmed
            },
        });
        if subs {
            state::set_subs(nodes::load(&mut self.agent.store().flash));
        }
    }

    /// Answers one request into `self.resp`; returns its length (0: no
    /// answer) and whether to reboot after sending it.
    async fn handle(&mut self, request: &[u8]) -> (usize, bool) {
        step(14);
        let Ok((f, _)) = Frame::decode(request, max_payload::USB) else {
            return (0, false);
        };
        let is_request = f.flags & (flags::RESP | flags::EVENT) == 0;
        let (status, body) = match Cmd::from_u8(f.cmd) {
            Some(Cmd::Relay) if is_request => {
                state::pc_active();
                match Relay::parse(f.payload) {
                    Ok(r) => self.relay(&r).await,
                    Err(s) => (s, 0),
                }
            }
            Some(Cmd::Show) if is_request => {
                state::pc_active();
                (show(f.payload), 0)
            }
            Some(Cmd::SubCtl) if is_request => (self.subctl(f.payload).await, 0),
            Some(Cmd::I2cRaw) if is_request => self.i2c_raw(f.payload).await,
            _ => {
                let cmd = f.cmd;
                let answer = match self.agent.handle(request) {
                    Some(reply) => {
                        let n = reply.frame.len();
                        self.resp[..n].copy_from_slice(reply.frame);
                        (n, reply.reboot)
                    }
                    None => (0, false),
                };
                let changes_self = [Cmd::SetPending, Cmd::Confirm, Cmd::XferEnd]
                    .iter()
                    .any(|c| *c as u8 == cmd);
                if changes_self {
                    self.publish(cmd == Cmd::XferEnd as u8);
                }
                return answer;
            }
        };
        self.resp[HEADER_LEN] = status as u8;
        let n =
            dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, 1 + body).unwrap_or(0);
        (n, false)
    }

    /// Passes a request to the link task that owns the bus; its answer
    /// body goes to `resp[9..]`.
    async fn via(
        &mut self,
        port: &Port,
        op: Op,
        addr: u8,
        timeout_ms: u16,
        read_len: u16,
        data: &[u8],
    ) -> (Status, usize) {
        let req = Req {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            op,
            addr,
            timeout_ms,
            read_len,
            data: Vec::from_slice(data).unwrap_or_default(),
        };
        wdt::busy_for(
            Task::Pc,
            Duration::from_millis(timeout_ms as u64) + Duration::from_secs(1),
        );
        let resp = port.call(req).await;
        let n = resp.data.len();
        self.resp[HEADER_LEN + 1..HEADER_LEN + 1 + n].copy_from_slice(&resp.data);
        (resp.status, if resp.status == Status::Ok { n } else { 0 })
    }

    async fn relay(&mut self, r: &Relay<'_>) -> (Status, usize) {
        if !cmd::relay_fits(r.link, r.addr, r.inner.len()) {
            return (Status::Arg, 0);
        }
        let port = if r.link == link::I2C0 {
            &I2C_PORT
        } else {
            &UART_PORT
        };
        self.via(port, Op::Relay, r.addr, r.timeout_ms, 0, r.inner)
            .await
    }

    /// BOOT0 first, then NRST (a pulse samples the new BOOT0).
    async fn subctl(&mut self, p: &[u8]) -> Status {
        let s = match SubCtl::parse(p) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let line = match s.nrst {
            nrst::RELEASE => Nrst::Release,
            nrst::LOW => Nrst::Low,
            _ => Nrst::Pulse,
        };
        match self.sub.set_lines(line, s.boot0 == boot0::HIGH).await {
            Ok(()) => Status::Ok,
            Err(_) => Status::Timeout,
        }
    }

    /// Raw transactions to the ROM loader, only while BOOT0 is high.
    async fn i2c_raw(&mut self, p: &[u8]) -> (Status, usize) {
        let raw = match cmd::i2c_raw(p, self.sub.boot0_high()) {
            Ok(r) => r,
            Err(e) => return (e, 0),
        };
        let (len, data) = (raw.read_len, raw.data);
        self.via(&I2C_PORT, Op::Raw, ROM_ADDR, RAW_TIMEOUT_MS, len, data)
            .await
    }
}

fn show(p: &[u8]) -> Status {
    match Show::parse(p) {
        Ok(s) => {
            state::set_progress(Progress::from(&s));
            Status::Ok
        }
        Err(e) => e,
    }
}

async fn recv(class: &mut Cdc, pkt: &mut [u8]) -> Result<usize, EndpointError> {
    class.wait_connection().await;
    class.read_packet(pkt).await
}

/// Writes one frame; gives up if the host stops reading.
async fn send(class: &mut Cdc, data: &[u8]) -> Result<(), EndpointError> {
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
