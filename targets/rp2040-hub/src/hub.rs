//! The hub's request loop: frames from USB CDC, hub commands (RELAY, SHOW,
//! SUBCTL, I2C_RAW) answered here, everything else by the hub's own agent;
//! display polling while the PC is quiet.

use dagon_agent::{Agent, DefaultHooks, SlotStore};
use dagon_port::{Nrst, SubControl};
use dagon_proto::payload::{boot0, nrst, Relay, Show, SlotState, SubCtl};
use dagon_proto::{flags, max_payload, Cmd, Frame, Status, CRC_LEN, HEADER_LEN, MAX_FRAME};
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::driver::EndpointError;
use rp2040_common::arm::step;
use rp2040_common::cmd;
use rp2040_common::frames::FrameReader;

use crate::lcd::{Home, Node, Progress, Shade, HOME};
use crate::relay::{Links, PioStream, RpI2c};
use crate::store::{HubStore, ACTIVE};
use crate::sub::SubLines;
use crate::{nodes, wdt};

pub const USB_PACKET: u16 = 64;
const POLL_EVERY: Duration = Duration::from_secs(2);
/// No own polling (and the progress screen stays) for this long after a
/// RELAY or SHOW: the PC drives the nodes.
const QUIET_AFTER_PC: Duration = Duration::from_secs(10);
const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// Lets the REBOOT reply leave before the reset drops USB.
const REBOOT_DELAY: Duration = Duration::from_millis(50);
/// I2C_RAW target: the STM32 ROM bootloader.
const ROM_ADDR: u8 = 0x39;

type Cdc<'d> = CdcAcmClass<'d, Driver<'d, USB>>;

pub struct Hub<'d> {
    pub agent: Agent<HubStore, DefaultHooks>,
    pub links: Links<RpI2c<'d>, PioStream<'d>>,
    pub sub: SubLines<'d>,
    pub resp: [u8; MAX_FRAME],
    pub quiet_until: Instant,
    pub poll_seq: u8,
    pub home: Home,
}

impl Hub<'_> {
    pub async fn serve(&mut self, class: &mut Cdc<'_>) -> ! {
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
                self.quiet_until = Instant::now() + QUIET_AFTER_PC;
                match Relay::parse(f.payload) {
                    Ok(r) => self.relay(&r).await,
                    Err(s) => (s, 0),
                }
            }
            Some(Cmd::Show) if is_request => {
                self.quiet_until = Instant::now() + QUIET_AFTER_PC;
                (self.show(f.payload), 0)
            }
            Some(Cmd::SubCtl) if is_request => (self.subctl(f.payload).await, 0),
            Some(Cmd::I2cRaw) if is_request => self.i2c_raw(f.payload).await,
            _ => {
                return match self.agent.handle(request) {
                    Some(reply) => {
                        let n = reply.frame.len();
                        self.resp[..n].copy_from_slice(reply.frame);
                        (n, reply.reboot)
                    }
                    None => (0, false),
                };
            }
        };
        self.resp[HEADER_LEN] = status as u8;
        let n =
            dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, 1 + body).unwrap_or(0);
        (n, false)
    }

    /// RELAY: status, then the node's response frame at `resp[9..]`.
    async fn relay(&mut self, r: &Relay<'_>) -> (Status, usize) {
        if !cmd::relay_fits(r.link, r.addr, r.inner.len()) {
            return (Status::Arg, 0);
        }
        let out = &mut self.resp[HEADER_LEN + 1..];
        match self
            .links
            .exchange(r.link, r.addr, r.inner, r.timeout_ms, out)
            .await
        {
            Ok(n) => (Status::Ok, n),
            Err(s) => (s, 0),
        }
    }

    fn show(&mut self, p: &[u8]) -> Status {
        match Show::parse(p) {
            Ok(s) => {
                self.home.progress = Some(Progress::from(&s));
                HOME.signal(self.home.clone());
                Status::Ok
            }
            Err(e) => e,
        }
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

    /// Raw transactions to the ROM loader, only while BOOT0 is high. The
    /// bytes read go to `resp[9..]`.
    async fn i2c_raw(&mut self, p: &[u8]) -> (Status, usize) {
        let raw = match cmd::i2c_raw(p, self.sub.boot0_high()) {
            Ok(r) => r,
            Err(e) => return (e, 0),
        };
        let n = raw.read_len as usize;
        let out = &mut self.resp[HEADER_LEN + 1..MAX_FRAME - CRC_LEN];
        match self.links.rom(ROM_ADDR, raw.data, &mut out[..n]).await {
            Ok(()) => (Status::Ok, n),
            Err(s) => (s, 0),
        }
    }

    /// Display-only HELLO to every node, then refresh the LCD. Ends the
    /// progress screen once the PC has been quiet.
    async fn poll(&mut self) {
        step(13);
        let st = self.agent.store().status();
        let subs = nodes::load(&mut self.agent.store().flash);
        let me = st.slots[ACTIVE as usize];
        self.home.progress = None;
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
