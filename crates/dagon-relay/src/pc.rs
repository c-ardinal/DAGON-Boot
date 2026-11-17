//! Serving the PC: DAGON frames from the uplink. RELAY and I2C_RAW go to
//! the worker of the link they name, SUBCTL to the recovery lines (owned
//! here); everything else, SHOW included, is this node's own agent (owned
//! here). A link or recovery lines the node does not have answer
//! E_UNSUPPORTED.

use dagon_port::{Link, Nrst, SubControl};
use dagon_proto::payload::{self, boot0, nrst, Relay as RelayReq, SubCtl};
use dagon_proto::{flags, Cmd, Frame, FrameError, Status, HEADER_LEN, MAX_FRAME};

use crate::port::{Op, Port, Req};
use crate::{cmd, task, Config, LinkRoute, Liveness, OwnAgent, RelayClock};

/// The reset and boot-mode lines of a node, and the link and address its
/// ROM boot loader answers I2C_RAW on.
pub struct Recovery<C> {
    pub lines: C,
    /// Id of that link (one of the node's [`RelayLinkDef`](dagon_port::RelayLinkDef)s).
    pub link: u8,
    /// The loader's 7-bit I2C address: the `[rom_i2c] addr` of that node's
    /// MCU definition, which the MCU package's build generates.
    pub rom_addr: u8,
}

/// What [`serve`] runs on: this node's agent, its links and the parts it
/// shares with the link workers.
pub struct Relay<'a, A, C, W, T> {
    pub agent: A,
    /// Every link the node relays over, one entry each (the MCU package's
    /// list), with the port of the link's worker where the board wires it.
    pub links: &'a [LinkRoute<'a>],
    pub recovery: Option<Recovery<C>>,
    pub watch: W,
    pub clock: T,
    pub config: Config,
}

struct Pc<'a, A, C, W, T> {
    r: Relay<'a, A, C, W, T>,
    resp: [u8; MAX_FRAME],
    next_id: u32,
    /// BOOT0 as last set successfully (I2C_RAW only while it is high).
    boot0_high: bool,
}

/// Serves the PC over the uplink `up`, whatever its kind, for good. A
/// reboot waits the delay `allow_reboot_now` asked for, idle under the
/// guard, then resets.
pub async fn serve<L, A, C, W, T>(relay: Relay<'_, A, C, W, T>, mut up: L) -> !
where
    L: Link,
    A: OwnAgent,
    C: SubControl,
    W: Liveness,
    T: RelayClock,
{
    let mut pc = Pc {
        r: relay,
        resp: [0; MAX_FRAME],
        next_id: 1,
        boot0_high: false,
    };
    let max = up.max_payload();
    let (via, addr) = (up.kind().code(), up.addr());
    let mut link_up = false;
    loop {
        pc.r.watch.idle(task::PC);
        let read = up.read_frame().await;
        pc.r.watch.busy(task::PC);
        let Ok(request) = read else {
            continue;
        };
        // Every request starts with a gated feed, so a flash erase it runs
        // has a full watchdog period (execution model, section 12, rule 4).
        pc.r.watch.progress(task::PC);
        if !link_up {
            link_up = true;
            pc.r.agent.link_up(via, addr);
        }
        let arrived = pc.r.clock.now_ms();
        pc.r.agent.request(request, via);
        let (out, reboot) = pc.handle(request, max).await;
        if up.write_frame(&pc.resp[..out]).await.is_err() {
            // The PC is gone: no reply, no reboot; its retry gets the
            // same reply (same seq).
            link_up = false;
            pc.r.agent.link_down(via, addr);
            continue;
        }
        let ms = pc.r.clock.now_ms().saturating_sub(arrived);
        pc.r.agent
            .replied(&pc.resp[..out], u32::try_from(ms).unwrap_or(u32::MAX));
        if let Some(delay_ms) = reboot {
            // Only once the reply has left: the reset drops the link.
            if up.flush().await.is_err() {
                continue;
            }
            pc.r.watch.idle(task::PC);
            let at = pc.r.clock.now_ms() + u64::from(delay_ms);
            pc.r.clock.wait_until(at).await;
            pc.r.agent.before_reboot();
            pc.r.watch.reboot();
        }
    }
}

impl<'a, A, C, W, T> Pc<'a, A, C, W, T>
where
    A: OwnAgent,
    C: SubControl,
    W: Liveness,
    T: RelayClock,
{
    /// Answers one request into `self.resp`; returns its length (0: no
    /// answer) and, when a reboot follows the reply, its delay in ms.
    async fn handle(&mut self, request: &[u8], max: usize) -> (usize, Option<u32>) {
        let f = match Frame::decode(request, max) {
            Ok((f, _)) => f,
            // A request of another wire version is told so.
            Err(FrameError::BadVersion) => {
                let n = dagon_proto::version_refusal(request, &mut self.resp);
                return (n.unwrap_or(0), None);
            }
            Err(_) => return (0, None),
        };
        let is_request = f.flags & (flags::RESP | flags::EVENT) == 0;
        let (status, bulk) = match Cmd::from_u8(f.cmd) {
            Some(Cmd::Relay) if is_request => match RelayReq::parse(f.payload) {
                Ok(r) => self.relay(&r, max).await,
                Err(s) => (s, 0),
            },
            Some(Cmd::SubCtl) if is_request => (self.lines(f.payload).await, 0),
            Some(Cmd::I2cRaw) if is_request => self.i2c_raw(f.payload, max).await,
            _ => return self.own(request),
        };
        let out = &mut self.resp[HEADER_LEN..];
        let len = match (status, Cmd::from_u8(f.cmd)) {
            // The worker's answer is the bulk, already in place.
            (Status::Ok, Some(Cmd::Relay | Cmd::I2cRaw)) => {
                out[0] = Status::Ok as u8;
                1 + payload::finish_bulk_resp(&mut out[1..], bulk).unwrap_or(0)
            }
            _ => payload::put_refusal(out, status, f.cmd, f.payload),
        };
        let n = dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, len).unwrap_or(0);
        (n, None)
    }

    /// A request for this node's own agent.
    fn own(&mut self, request: &[u8]) -> (usize, Option<u32>) {
        match self.r.agent.handle(request) {
            Some((frame, reboot_ms)) => {
                let n = frame.len();
                self.resp[..n].copy_from_slice(frame);
                (n, reboot_ms)
            }
            None => (0, None),
        }
    }

    /// Passes `req` with `data` to the worker behind `port`; its answer
    /// goes where the response's bulk starts, within the uplink's `max`
    /// payload (after the status byte and the delimiter): E_ARG for one
    /// longer than that.
    async fn via(&mut self, port: &Port, req: Req, data: &[u8], max: usize) -> (Status, usize) {
        // The link worker holds the deadline while it works on the request.
        self.r.watch.idle(task::PC);
        let margin = self.r.config.port_margin_ms;
        // An uplink carries at most MAX_FRAME (the largest frame of any link).
        let body = &mut self.resp[HEADER_LEN + 1 + payload::BULK_RESP_AT..HEADER_LEN + max];
        let answer = port.call(req, data, body, &self.r.clock, margin).await;
        self.r.watch.busy(task::PC);
        answer
    }

    /// A request to a link worker, with the next id.
    fn req(&mut self, op: Op, addr: u8, timeout_ms: u16, read_len: u16) -> Req {
        let id = self.next_id;
        self.next_id = id.wrapping_add(1);
        Req {
            id,
            op,
            addr,
            timeout_ms,
            read_len,
        }
    }

    fn route(&self, link: u8) -> Option<LinkRoute<'a>> {
        self.r.links.iter().find(|k| k.link.id == link).copied()
    }

    /// RELAY: E_ARG for a link the node does not have, or an address or
    /// frame that link does not take; E_UNSUPPORTED for a link the board
    /// does not wire.
    async fn relay(&mut self, r: &RelayReq<'_>, max: usize) -> (Status, usize) {
        let Some(route) = self.route(r.link) else {
            return (Status::Arg, 0);
        };
        if !route.link.fits(r.addr, r.inner.len()) {
            return (Status::Arg, 0);
        }
        let Some(port) = route.port else {
            return (Status::Unsupported, 0);
        };
        let wait = r.timeout_ms.unwrap_or(dagon_port::timeout::RELAY_WAIT_MS);
        let req = self.req(Op::Relay, r.addr, wait, 0);
        self.via(port, req, r.inner, max).await
    }

    /// SUBCTL: BOOT0 first, then NRST (a pulse samples the new BOOT0).
    async fn lines(&mut self, p: &[u8]) -> Status {
        let s = match SubCtl::parse(p) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let line = match s.nrst {
            nrst::RELEASE => Nrst::Release,
            nrst::LOW => Nrst::Low,
            _ => Nrst::Pulse,
        };
        let Some(rec) = self.r.recovery.as_mut() else {
            return Status::Unsupported;
        };
        let high = s.boot0 == boot0::HIGH;
        match rec.lines.set_lines(line, high).await {
            Ok(()) => {
                self.boot0_high = high;
                Status::Ok
            }
            Err(_) => Status::Timeout,
        }
    }

    /// Raw transactions to the ROM loader, only while BOOT0 is high, each
    /// within the recovery link's buffer.
    async fn i2c_raw(&mut self, p: &[u8], max: usize) -> (Status, usize) {
        let Some(rec) = self.r.recovery.as_ref() else {
            return (Status::Unsupported, 0);
        };
        let addr = rec.rom_addr;
        let route = self.route(rec.link);
        let Some((link, port)) = route.and_then(|k| Some((k.link, k.port?))) else {
            return (Status::Unsupported, 0);
        };
        let raw = match cmd::i2c_raw(p, self.boot0_high, &link) {
            Ok(r) => r,
            Err(e) => return (e, 0),
        };
        let timeout = self.r.config.raw_timeout_ms;
        let req = self.req(Op::Raw, addr, timeout, raw.read_len);
        self.via(port, req, raw.data, max).await
    }
}
