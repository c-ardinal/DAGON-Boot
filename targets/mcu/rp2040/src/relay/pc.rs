//! The PC task (high-priority interrupt executor): DAGON frames from USB
//! CDC. RELAY and I2C_RAW go to the link task that owns the bus, SHOW to
//! the shared state (a display shows it), SUBCTL to the sub node's control
//! lines (owned here); everything else is the 親機's own agent (owned
//! here, with the flash). A link or the control lines the board does not
//! have answer E_UNSUPPORTED.

use portable_atomic::{AtomicU32, Ordering};

use dagon_agent::SlotStore;
use dagon_port::{Nrst, SubControl};
use dagon_proto::payload::{boot0, link, nrst, Relay, Show, SlotState, SubCtl};
use dagon_proto::{flags, max_payload, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};
use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::Peri;
use embassy_time::Timer;
use heapless::Vec;

use super::nodes::{self, SubNode};
use super::ports::{Op, Port, Req, I2C_PORT, UART_PORT};
use super::state::{self, Progress, Seen, Shade};
use super::task;
use crate::arm::step;
use crate::cmd;
use crate::node::AppAgent;
use crate::store::ACTIVE;
use crate::sub::SubLines;
use crate::usb::{self, CdcLink, REBOOT_DELAY};
use crate::watchdog::Watch;

/// I2C_RAW target: the STM32 ROM bootloader.
const ROM_ADDR: u8 = 0x39;
/// Bound on one I2C_RAW request at the link task.
const RAW_TIMEOUT_MS: u16 = 1_000;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub struct Pc {
    pub agent: AppAgent,
    pub sub: Option<SubLines<'static>>,
    pub has_i2c: bool,
    pub has_uart: bool,
    pub fallback: &'static [SubNode],
    pub watch: &'static dyn Watch,
    pub resp: [u8; MAX_FRAME],
}

/// Runs inside the high-priority executor: builds the USB device there
/// (its types stay in that executor), starts it, then serves the PC.
#[embassy_executor::task]
pub async fn task(
    usb: Peri<'static, USB>,
    product: &'static str,
    serial: &'static str,
    pc: Pc,
) -> ! {
    let (device, link) = usb::new(usb, product, serial);
    // SAFETY: awaited directly inside this embassy task, whose context
    // comes from the interrupt executor running it.
    let spawner = unsafe { Spawner::for_current_executor() }.await;
    spawner.spawn(usb::run(device).expect("task pool"));
    serve(pc, link).await
}

async fn serve(mut pc: Pc, mut link: CdcLink) -> ! {
    pc.publish(true);
    loop {
        pc.watch.idle(task::PC);
        let request = link.read_frame().await;
        pc.watch.busy(task::PC);
        let (out, reboot) = pc.handle(request).await;
        let _ = link.write_frame(&pc.resp[..out]).await;
        if reboot {
            Timer::after(REBOOT_DELAY).await;
            pc.watch.reboot();
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
            state::set_subs(nodes::load(&mut self.agent.store().flash, self.fallback));
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
        // The link task holds the deadline while it works on the request.
        self.watch.idle(task::PC);
        let resp = port.call(req).await;
        self.watch.busy(task::PC);
        let n = resp.data.len();
        self.resp[HEADER_LEN + 1..HEADER_LEN + 1 + n].copy_from_slice(&resp.data);
        (resp.status, if resp.status == Status::Ok { n } else { 0 })
    }

    async fn relay(&mut self, r: &Relay<'_>) -> (Status, usize) {
        if !cmd::relay_fits(r.link, r.addr, r.inner.len()) {
            return (Status::Arg, 0);
        }
        let (port, present) = if r.link == link::I2C0 {
            (&I2C_PORT, self.has_i2c)
        } else {
            (&UART_PORT, self.has_uart)
        };
        if !present {
            return (Status::Unsupported, 0);
        }
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
        let Some(sub) = self.sub.as_mut() else {
            return Status::Unsupported;
        };
        match sub.set_lines(line, s.boot0 == boot0::HIGH).await {
            Ok(()) => Status::Ok,
            Err(_) => Status::Timeout,
        }
    }

    /// Raw transactions to the ROM loader, only while BOOT0 is high.
    async fn i2c_raw(&mut self, p: &[u8]) -> (Status, usize) {
        let Some(sub) = self.sub.as_ref().filter(|_| self.has_i2c) else {
            return (Status::Unsupported, 0);
        };
        let raw = match cmd::i2c_raw(p, sub.boot0_high()) {
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
