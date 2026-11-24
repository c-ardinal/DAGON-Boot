//! Serving the PC: DAGON frames from the uplink. RELAY and I2C_RAW go to
//! the worker of the link they name, SUBCTL to the recovery lines (owned
//! here); everything else, SHOW included, is this node's own agent (owned
//! here). A link the board does not wire answers E_UNSUPPORTED; a node
//! without recovery targets does not serve SUBCTL and I2C_RAW at all. SUBCTL
//! and I2C_RAW name their target by link and ROM loader address
//! ([`cmd::recovery_target`]). CAPS
//! about RELAY is answered here too, with the tunnel of the link asked
//! about; CAPS about any other command is the agent's.
//!
//! An answer is at most what the requester takes: the uplink's payload,
//! and the one its HELLO named ([`OwnAgent::ceiling`]). That is the
//! ingress of every tunnel, and a request whose answer would not fit it
//! is refused before a link or a line is used.

use dagon_port::{Link, Nrst, SubControl};
use dagon_proto::handler::{self, Handler};
use dagon_proto::payload::{
    self, boot0, nrst, CapsReq, CapsResp, CmdSet, I2cRaw, Relay as RelayReq, Req as Asked,
    RomTarget, SubCtl,
};
use dagon_proto::{flags, Cmd, Frame, FrameError, Status, HEADER_LEN, MAX_FRAME};

use crate::port::{Op, Port, Req};
use crate::{cmd, task, Config, LinkRoute, Liveness, OwnAgent, RelayClock};

/// A node the relay recovers through its ROM boot loader: its reset and
/// boot-mode lines, and the link and address the loader answers I2C_RAW
/// on. A relay has one per such node, on any of its I2C links.
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
    /// The nodes it recovers, told apart by link and loader address
    /// ([`cmd::recovery_target`]; the first answers SUBCTL and I2C_RAW
    /// that name none). Empty: SUBCTL and I2C_RAW are not served. At most
    /// [`MAX_RECOVERY`] are used.
    pub recovery: &'a mut [Recovery<C>],
    pub watch: W,
    pub clock: T,
    pub config: Config,
}

/// The commands the relay answers itself. Its handlers wait on the
/// node's links, so the rows have no action: [`Pc::handle`] takes the
/// typed request of a row and runs it.
const TABLE: [Handler<(), ()>; 3] = [
    Handler::deferred(Cmd::Relay),
    Handler::deferred(Cmd::SubCtl),
    Handler::deferred(Cmd::I2cRaw),
];

/// CAPS, which the relay answers about RELAY and leaves to the agent
/// about anything else (the agent's HELLO names it).
const CAPS: Handler<(), ()> = Handler::deferred(Cmd::Caps);

/// The most recovery targets a relay tells apart (one bit each of BOOT0).
pub const MAX_RECOVERY: usize = 32;

/// The commands of its own a relay serves: RELAY, and with recovery targets
/// SUBCTL and I2C_RAW. The node's HELLO names them with its agent's.
pub const fn cmds(recovery: bool) -> CmdSet {
    if recovery {
        handler::cmds(&TABLE)
    } else {
        CmdSet::EMPTY.with(Cmd::Relay as u8)
    }
}

struct Pc<'a, A, C, W, T> {
    r: Relay<'a, A, C, W, T>,
    resp: [u8; MAX_FRAME],
    next_id: u32,
    /// BOOT0 of each recovery target (bit i: `recovery[i]`) as last set
    /// successfully (I2C_RAW only while it is high).
    boot0_high: u32,
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
        boot0_high: 0,
    };
    pc.r.agent.also_serves(cmds(!pc.r.recovery.is_empty()));
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
        // has a full watchdog period (execution model, section 11, rule 4).
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
        let served = cmds(!self.r.recovery.is_empty());
        let row = handler::find(&TABLE, f.cmd)
            .filter(|h| served.has(h.cmd as u8))
            .or((f.cmd == Cmd::Caps as u8).then_some(&CAPS));
        let Some(row) = row.filter(|_| is_request) else {
            return self.own(request);
        };
        // What the requester takes: the ingress of a tunnel, and the most
        // an answer may be.
        let ceiling = self.r.agent.ceiling().over(max);
        let room = ceiling.bytes();
        // The whole request is read, and its answer held against the
        // ceiling, before any of it runs.
        let (status, len) = match row.request(f.payload, ceiling) {
            Err(refused) => (refused, 0),
            Ok(Asked::Caps(c)) if c.cmd != Cmd::Relay as u8 => return self.own(request),
            Ok(Asked::Caps(c)) => self.caps(c, room),
            Ok(Asked::SubCtl(s)) => (self.lines(s).await, 0),
            Ok(Asked::I2cRaw(raw)) => self.i2c_raw(raw, room).await,
            Ok(Asked::Relay(r)) => self.relay(&r, room).await,
            Ok(_) => (Status::Unsupported, 0),
        };
        let out = &mut self.resp[HEADER_LEN..];
        let len = if status != Status::Ok {
            payload::put_refusal(out, status, f.cmd, f.payload)
        } else {
            out[0] = Status::Ok as u8;
            if row.spec.resp.bulk {
                // The worker's answer is the bulk, already in place.
                1 + payload::finish_bulk_resp(&mut out[1..], len).unwrap_or(0)
            } else {
                // The body, if any, is already in place.
                1 + len
            }
        };
        let n = dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, len).unwrap_or(0);
        (n, None)
    }

    /// CAPS about RELAY: its fields, and for the link asked about the
    /// longest inner frame forwarded over it for a requester that takes
    /// `room` payload bytes ([`RelayReq::tunnel_mtu`]). E_ARG for a link
    /// the node does not have. The body goes after the status byte;
    /// returns its length.
    fn caps(&mut self, asked: CapsReq, room: usize) -> (Status, usize) {
        let tunnel = match asked.link.map(|id| self.route(id)) {
            None => None,
            Some(None) => return (Status::Arg, 0),
            Some(Some(route)) => {
                let mtu = RelayReq::tunnel_mtu(room, usize::from(route.link.max_payload));
                Some(u16::try_from(mtu).unwrap_or(u16::MAX))
            }
        };
        let answer = CapsResp {
            tunnel_mtu: tunnel,
            ..CapsResp::of(Cmd::Relay)
        };
        match answer.encode(&mut self.resp[HEADER_LEN + 1..]) {
            Ok(n) => (Status::Ok, n),
            Err(_) => (Status::Arg, 0),
        }
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
    /// goes where the response's bulk starts, within the `max` payload
    /// bytes the requester takes (after the status byte and the
    /// delimiter): E_ARG for one longer than that.
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

    /// RELAY: E_ARG, before anything is forwarded, for a link the node does
    /// not have, an address that link does not take, or an inner frame
    /// that is not one whole frame within the tunnel from the `max` payload
    /// bytes the requester takes over that link ([`cmd::relay_admits`]);
    /// E_UNSUPPORTED for a link the board does not wire.
    async fn relay(&mut self, r: &RelayReq<'_>, max: usize) -> (Status, usize) {
        let Some(route) = self.route(r.link) else {
            return (Status::Arg, 0);
        };
        if !cmd::relay_admits(&route.link, r, max) {
            return (Status::Arg, 0);
        }
        let Some(port) = route.port else {
            return (Status::Unsupported, 0);
        };
        let wait = r.timeout_ms.unwrap_or(dagon_port::timeout::RELAY_WAIT_MS);
        let req = self.req(Op::Relay, r.addr, wait, 0);
        self.via(port, req, r.inner, max).await
    }

    /// The recovery target a SUBCTL or I2C_RAW names, by index.
    fn target(&self, asked: Option<RomTarget>) -> Result<usize, Status> {
        let known = self.r.recovery.iter().take(MAX_RECOVERY);
        cmd::recovery_target(known.map(|r| (r.link, r.rom_addr)), asked)
    }

    /// SUBCTL: BOOT0 first, then NRST (a pulse samples the new BOOT0), of
    /// the target it names.
    async fn lines(&mut self, s: SubCtl) -> Status {
        let line = match s.nrst {
            nrst::RELEASE => Nrst::Release,
            nrst::LOW => Nrst::Low,
            _ => Nrst::Pulse,
        };
        let i = match self.target(s.target) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let high = s.boot0 == boot0::HIGH;
        match self.r.recovery[i].lines.set_lines(line, high).await {
            Ok(()) => {
                let bit = 1 << i;
                self.boot0_high = if high {
                    self.boot0_high | bit
                } else {
                    self.boot0_high & !bit
                };
                Status::Ok
            }
            Err(_) => Status::Timeout,
        }
    }

    /// Raw transactions to the ROM loader of the target it names, only
    /// while that target's BOOT0 is high, each within the buffer of the
    /// target's link.
    async fn i2c_raw(&mut self, raw: I2cRaw<'_>, max: usize) -> (Status, usize) {
        let i = match self.target(raw.target) {
            Ok(i) => i,
            Err(e) => return (e, 0),
        };
        let rec = &self.r.recovery[i];
        let addr = rec.rom_addr;
        let route = self.route(rec.link);
        let Some((link, port)) = route.and_then(|k| Some((k.link, k.port?))) else {
            return (Status::Unsupported, 0);
        };
        let high = self.boot0_high & (1 << i) != 0;
        if let Err(e) = cmd::i2c_raw(&raw, high, &link) {
            return (e, 0);
        }
        let timeout = self.r.config.raw_timeout_ms;
        let req = self.req(Op::Raw, addr, timeout, raw.read_len);
        self.via(port, req, raw.data, max).await
    }
}
