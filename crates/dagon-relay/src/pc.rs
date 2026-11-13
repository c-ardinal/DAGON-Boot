//! Serving the PC: DAGON frames from the uplink. RELAY and I2C_RAW go to
//! the worker of the link they name, SHOW to the [`State`], SUBCTL to the
//! recovery lines (owned here); everything else is this node's own agent
//! (owned here, with the flash). A link or recovery lines the node does
//! not have answer E_UNSUPPORTED.

use dagon_port::{Flash, Link, Nrst, SubControl};
use dagon_proto::payload::{boot0, nrst, HelloResp, Relay as RelayReq, Show, StatusResp, SubCtl};
use dagon_proto::{flags, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};
use heapless::Vec;

use crate::nodes::Identity;
use crate::port::{Op, Port, Req};
use crate::view::{Progress, Seen};
use crate::{cmd, nodes, task, Config, LinkRoute, Liveness, OwnAgent, RelayClock, State};

/// The reset and boot-mode lines of a node, and the link its ROM boot
/// loader answers I2C_RAW on.
pub struct Recovery<C> {
    pub lines: C,
    /// Id of that link (one of the node's [`RelayLinkDef`](dagon_port::RelayLinkDef)s).
    pub link: u8,
}

/// What [`serve`] runs on: this node's agent and flash, its links, and the
/// parts it shares with the link workers and a display.
pub struct Relay<'a, A, F, C, W, T> {
    pub agent: A,
    /// A handle on the flash the agent's store writes: the topology
    /// display copy is read through it, after its write.
    pub flash: F,
    /// Flash address of the topology display copy.
    pub topology: u32,
    /// Who this node is in the topology.
    pub identity: Identity<'a>,
    /// Every link the node relays over, one entry each (the MCU package's
    /// list), with the port of the link's worker where the board wires it.
    pub links: &'a [LinkRoute<'a>],
    pub recovery: Option<Recovery<C>>,
    pub state: &'a State,
    pub watch: W,
    pub clock: T,
    pub config: Config,
}

struct Pc<'a, A, F, C, W, T> {
    r: Relay<'a, A, F, C, W, T>,
    resp: [u8; MAX_FRAME],
    next_id: u32,
    /// BOOT0 as last set successfully (I2C_RAW only while it is high).
    boot0_high: bool,
}

/// Serves the PC over the uplink `up`, whatever its kind, for good. A
/// reboot waits the delay `allow_reboot_now` asked for, idle under the
/// guard, then resets.
pub async fn serve<L, A, F, C, W, T>(relay: Relay<'_, A, F, C, W, T>, mut up: L) -> !
where
    L: Link,
    A: OwnAgent,
    F: Flash,
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
    pc.publish(true);
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

impl<'a, A, F, C, W, T> Pc<'a, A, F, C, W, T>
where
    A: OwnAgent,
    F: Flash,
    C: SubControl,
    W: Liveness,
    T: RelayClock,
{
    /// Shares this node's own state and, if `linked`, the node list
    /// (topology display copy) with the other tasks.
    fn publish(&mut self, linked: bool) {
        let r = &mut self.r;
        r.state.set_own(Seen::from_status(r.agent.status()));
        if linked {
            let links = r.links.iter().map(|k| &k.link);
            let nodes = nodes::load(
                &mut r.flash,
                r.topology,
                r.identity,
                links,
                r.config.fallback,
            );
            r.state.set_nodes(nodes);
        }
    }

    /// Answers one request into `self.resp`; returns its length (0: no
    /// answer) and, when a reboot follows the reply, its delay in ms.
    async fn handle(&mut self, request: &[u8], max: usize) -> (usize, Option<u32>) {
        let Ok((f, _)) = Frame::decode(request, max) else {
            return (0, None);
        };
        let is_request = f.flags & (flags::RESP | flags::EVENT) == 0;
        let (status, body) = match Cmd::from_u8(f.cmd) {
            Some(Cmd::Relay) if is_request => {
                self.pc_active();
                match RelayReq::parse(f.payload) {
                    Ok(r) => self.relay(&r).await,
                    Err(s) => (s, 0),
                }
            }
            Some(Cmd::Show) if is_request => {
                self.pc_active();
                (self.show(f.payload), 0)
            }
            Some(Cmd::SubCtl) if is_request => (self.lines(f.payload).await, 0),
            Some(Cmd::I2cRaw) if is_request => self.i2c_raw(f.payload).await,
            _ => return self.own(request, f.cmd),
        };
        self.resp[HEADER_LEN] = status as u8;
        let n =
            dagon_proto::finish(&mut self.resp, flags::RESP, f.seq, f.cmd, 1 + body).unwrap_or(0);
        (n, None)
    }

    /// A request for this node's own agent.
    fn own(&mut self, request: &[u8], cmd: u8) -> (usize, Option<u32>) {
        let answer = match self.r.agent.handle(request) {
            Some((frame, reboot_ms)) => {
                let n = frame.len();
                self.resp[..n].copy_from_slice(frame);
                (n, reboot_ms)
            }
            None => (0, None),
        };
        let changes_self = [Cmd::SetPending, Cmd::Confirm, Cmd::XferEnd]
            .iter()
            .any(|c| *c as u8 == cmd);
        if changes_self {
            self.publish(cmd == Cmd::XferEnd as u8);
        }
        answer
    }

    fn pc_active(&self) {
        let now = self.r.clock.now_ms();
        self.r.state.pc_active(now, self.r.config.quiet_ms);
    }

    /// Passes a request to the worker behind `port`; its answer body goes
    /// to `resp[9..]`.
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
            id: self.next_id,
            op,
            addr,
            timeout_ms,
            read_len,
            data: Vec::from_slice(data).unwrap_or_default(),
        };
        self.next_id = self.next_id.wrapping_add(1);
        // The link worker holds the deadline while it works on the request.
        self.r.watch.idle(task::PC);
        let margin = self.r.config.port_margin_ms;
        let resp = port.call(req, &self.r.clock, margin).await;
        self.r.watch.busy(task::PC);
        let n = resp.data.len();
        self.resp[HEADER_LEN + 1..HEADER_LEN + 1 + n].copy_from_slice(&resp.data);
        (resp.status, if resp.status == Status::Ok { n } else { 0 })
    }

    fn route(&self, link: u8) -> Option<LinkRoute<'a>> {
        self.r.links.iter().find(|k| k.link.id == link).copied()
    }

    /// RELAY: E_ARG for a link the node does not have, or an address or
    /// frame that link does not take; E_UNSUPPORTED for a link the board
    /// does not wire.
    async fn relay(&mut self, r: &RelayReq<'_>) -> (Status, usize) {
        let Some(route) = self.route(r.link) else {
            return (Status::Arg, 0);
        };
        if !route.link.fits(r.addr, r.inner.len()) {
            return (Status::Arg, 0);
        }
        let Some(port) = route.port else {
            return (Status::Unsupported, 0);
        };
        let (status, n) = self
            .via(port, Op::Relay, r.addr, r.timeout_ms, 0, r.inner)
            .await;
        if status == Status::Ok {
            let answer = &self.resp[HEADER_LEN + 1..HEADER_LEN + 1 + n];
            if let Some(seen) = relayed_seen(answer, usize::from(route.link.max_payload)) {
                self.r.state.set_seen_at(r.link, r.addr, seen);
            }
        }
        (status, n)
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

    /// Raw transactions to the ROM loader, only while BOOT0 is high.
    async fn i2c_raw(&mut self, p: &[u8]) -> (Status, usize) {
        let route = self
            .r
            .recovery
            .as_ref()
            .and_then(|rec| self.route(rec.link));
        let Some(port) = route.and_then(|k| k.port) else {
            return (Status::Unsupported, 0);
        };
        let raw = match cmd::i2c_raw(p, self.boot0_high) {
            Ok(r) => r,
            Err(e) => return (e, 0),
        };
        let (len, data) = (raw.read_len, raw.data);
        let (addr, timeout) = (self.r.config.rom_addr, self.r.config.raw_timeout_ms);
        self.via(port, Op::Raw, addr, timeout, len, data).await
    }

    fn show(&self, p: &[u8]) -> Status {
        match Show::parse(p) {
            Ok(s) => {
                self.r.state.set_progress(Progress::from(&s));
                Status::Ok
            }
            Err(e) => e,
        }
    }
}

/// What a relayed HELLO or STATUS answer tells about its node.
fn relayed_seen(frame: &[u8], max: usize) -> Option<Seen> {
    let (f, _) = Frame::decode(frame, max).ok()?;
    let (Status::Ok, body) = dagon_proto::split_status(f.payload)? else {
        return None;
    };
    match Cmd::from_u8(f.cmd)? {
        Cmd::Hello => Some(Seen::from_hello(HelloResp::parse(body).ok().as_ref())),
        Cmd::Status => StatusResp::parse(body)
            .ok()
            .map(|st| Seen::from_status(&st)),
        _ => None,
    }
}
