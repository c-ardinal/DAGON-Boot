//! Simulated relaying node: its own agent on USB, and the links its relay
//! table lists, each a port of its MCU definition: one simulated I2C bus of
//! SimNodes per I2C port (absent addresses NACK), one node behind a SimLink
//! per UART port, the NRST/BOOT0 lines of one I2C node, and the commands
//! RELAY, SUBCTL and I2C_RAW. Every other request, SHOW among them, goes to
//! the node's own agent.
//!
//! Time is simulated: [`SimBus::now_ms`] only moves when something waits
//! (mailbox polls, link delays, ROM work, [`SimRelay::advance_ms`]) and by
//! [`PC_ROUND_TRIP_MS`] per request.

use std::collections::BTreeMap;

use crate::link::SimLink;
use crate::node::SimNode;
use dagon_port::{BusError, LinkKind, Nrst, RelayLinkDef};
use dagon_proto::handler::{self, Handler};
use dagon_proto::mailbox::{State, STATE_LEN};
use dagon_proto::payload::{
    self, boot0, nrst, CapsReq, CapsResp, CmdSet, I2cRaw, PeersResp, Relay, Req, RomTarget, SubCtl,
    UartRaw,
};
use dagon_proto::{find_sof, flags, frame_len, max_payload, Cmd, Frame, FrameError, Status};
use dagon_topology::defs::{McuDef, PortDef};

/// Cost of one PC <-> relaying node request on the simulated clock.
pub const PC_ROUND_TRIP_MS: u64 = 2;
/// Mailbox poll interval on I2C.
pub const MAILBOX_POLL_MS: u64 = 2;
/// Low time of SUBCTL `nrst = PULSE`.
pub const NRST_PULSE_MS: u64 = 10;

/// Cuts complete frames out of a byte stream (SOF search, CRC check).
#[derive(Default)]
pub struct FrameCutter {
    buf: Vec<u8>,
}

impl FrameCutter {
    pub fn push(&mut self, bytes: &[u8], max_payload: usize) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let Some(start) = find_sof(&self.buf, 0) else {
                self.buf.clear();
                break;
            };
            self.buf.drain(..start);
            match Frame::decode(&self.buf, max_payload) {
                Ok((_, n)) => frames.push(self.buf.drain(..n).collect()),
                Err(FrameError::Incomplete) => break,
                // Handed over as its start, for the answer that refuses it.
                Err(FrameError::BadVersion) if dagon_proto::is_refused_start(&self.buf) => {
                    frames.push(self.buf.drain(..dagon_proto::REFUSED_LEN).collect())
                }
                Err(_) => {
                    self.buf.drain(..1);
                }
            }
        }
        frames
    }
}

/// What a relaying node has besides its own agent: what its ports reach
/// and its relay table.
pub struct Relaying {
    pub bus: SimBus,
    /// Its MCU definition's `[[relay_link]]`: the links RELAY reaches
    /// nodes over.
    links: Vec<RelayLinkDef>,
    rom_addr: Option<u8>,
    serves_peers: bool,
}

impl Relaying {
    /// The links RELAY reaches nodes over.
    pub fn links(&self) -> &[RelayLinkDef] {
        &self.links
    }
}

/// The node behind one UART port, and that port's byte stream.
pub struct SimStream {
    pub link: SimLink,
    pub node: SimNode,
    /// `Some`: the node relays too, to what its own ports reach
    /// ([`SimRelay::set_uart_relay`]).
    pub relaying: Option<Box<Relaying>>,
    input: FrameCutter,
    output: Vec<u8>,
}

impl SimStream {
    pub fn new(link: SimLink, node: SimNode) -> Self {
        SimStream {
            link,
            node,
            relaying: None,
            input: FrameCutter::default(),
            output: Vec::new(),
        }
    }

    /// The simulated clock reached `now`: the node, and what it relays to.
    fn tick(&mut self, now: u64) {
        self.node.tick(now);
        if let Some(r) = self.relaying.as_mut() {
            r.bus.advance_ms(now.saturating_sub(r.bus.now_ms));
        }
    }

    /// Request out, response back; returns it with the round-trip delay,
    /// the time the node took relaying included.
    fn carry(&mut self, frame: &[u8], now: u64) -> (Option<Vec<u8>>, u64) {
        let d = self.link.delay_ms;
        let Some(f) = self.link.carry(frame).filter(|_| !self.link.silent) else {
            return (None, 2 * d);
        };
        let arrived = now + d;
        let (resp, took) = match self.relaying.as_mut() {
            None => (self.node.handle_frame(&f, arrived), 0),
            // A node that is held, off or silent relays nothing either.
            Some(_) if !self.node.answers_at(arrived) => (None, 0),
            Some(r) => {
                r.bus.advance_ms(arrived.saturating_sub(r.bus.now_ms));
                let resp = Serving {
                    node: &mut self.node,
                    bus: &mut r.bus,
                    links: &r.links,
                    rom_addr: r.rom_addr,
                    serves_peers: r.serves_peers,
                    ingress: max_payload::UART,
                }
                .request(&f);
                (resp, r.bus.now_ms - arrived)
            }
        };
        (resp.and_then(|r| self.link.carry(&r)), 2 * d + took)
    }
}

/// The node RELAY over link `link` of `links` to `addr` reaches on `bus`,
/// as the stream it is behind when the link is a UART.
fn behind<'a>(
    bus: &'a mut SimBus,
    links: &[RelayLinkDef],
    link: u8,
    addr: u8,
) -> Option<(&'a mut SimNode, Option<&'a mut Relaying>)> {
    let l = links.iter().find(|l| l.id == link)?;
    match l.kind {
        LinkKind::I2c => bus
            .i2c
            .get_mut(&l.port)?
            .iter_mut()
            .find(|n| n.i2c_addr == Some(addr))
            .map(|n| (n, None)),
        LinkKind::Uart => bus
            .uart
            .get_mut(&l.port)
            .map(|s| (&mut s.node, s.relaying.as_deref_mut())),
        _ => None,
    }
}

/// What the relaying node's ports reach, on one simulated clock. Ports are
/// keyed by their `[[link_port]]` index in its MCU definition.
pub struct SimBus {
    pub now_ms: u64,
    /// The nodes on each I2C port.
    pub i2c: BTreeMap<u8, Vec<SimNode>>,
    /// The node behind each UART port.
    pub uart: BTreeMap<u8, SimStream>,
    /// The node wired to NRST and BOOT0: its I2C port and its place there.
    pub sub: Option<(u8, usize)>,
    /// Times a port or the recovery lines were used: every write, read
    /// and exchange, and every setting of the lines.
    pub ops: u64,
}

impl SimBus {
    pub fn new() -> Self {
        SimBus {
            now_ms: 0,
            i2c: BTreeMap::new(),
            uart: BTreeMap::new(),
            sub: None,
            ops: 0,
        }
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.now_ms += ms;
        let now = self.now_ms;
        self.i2c.values_mut().flatten().for_each(|n| n.tick(now));
        self.uart.values_mut().for_each(|s| s.tick(now));
    }

    /// Port `port` as a `dagon-port` I2C bus and byte stream.
    pub fn port(&mut self, port: u8) -> SimPort<'_> {
        SimPort { bus: self, port }
    }

    /// The node configured at `addr` on I2C port `port` (present or not).
    pub fn i2c_node(&mut self, port: u8, addr: u8) -> Option<&mut SimNode> {
        self.i2c
            .get_mut(&port)?
            .iter_mut()
            .find(|n| n.i2c_addr == Some(addr))
    }

    fn target(&mut self, port: u8, addr: u8) -> Option<&mut SimNode> {
        self.i2c
            .get_mut(&port)?
            .iter_mut()
            .find(|n| n.i2c_present(addr))
    }

    pub fn i2c_write(&mut self, port: u8, addr: u8, data: &[u8]) -> Result<(), BusError> {
        self.ops += 1;
        let mut now = self.now_ms;
        let r = match self.target(port, addr) {
            Some(n) => n.i2c_write(addr, data, &mut now),
            None => Err(BusError::Nack),
        };
        if now > self.now_ms {
            self.advance_ms(now - self.now_ms);
        }
        r
    }

    pub fn i2c_read(&mut self, port: u8, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        self.ops += 1;
        let now = self.now_ms;
        match self.target(port, addr) {
            Some(n) => n.i2c_read(addr, buf, now),
            None => Err(BusError::Nack),
        }
    }

    /// 7-bit addresses 0x08..=0x77 on `port` that ACK an address-only write.
    pub fn scan(&mut self, port: u8) -> Vec<u8> {
        (0x08..=0x77u8)
            .filter(|&a| self.i2c_write(port, a, &[]).is_ok())
            .collect()
    }

    /// RELAY over I2C: write the request (retrying NACK), poll the 3-byte
    /// state every [`MAILBOX_POLL_MS`], read the response.
    pub fn i2c_exchange(
        &mut self,
        port: u8,
        addr: u8,
        frame: &[u8],
        timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        self.ops += 1;
        let deadline = self.now_ms + timeout_ms;
        while self.i2c_write(port, addr, frame).is_err() {
            if self.now_ms >= deadline {
                return None;
            }
            self.advance_ms(MAILBOX_POLL_MS);
        }
        loop {
            self.advance_ms(MAILBOX_POLL_MS);
            let mut st = [0u8; STATE_LEN];
            let state = self
                .i2c_read(port, addr, &mut st)
                .ok()
                .and_then(|_| State::parse(st));
            if let Some(State::Ready(n)) = state {
                let mut resp = vec![0u8; n as usize];
                return self.i2c_read(port, addr, &mut resp).ok().map(|_| resp);
            }
            if self.now_ms >= deadline {
                return None;
            }
        }
    }

    /// RELAY over UART: through the port's link both ways; delays count
    /// against `timeout_ms`.
    pub fn uart_exchange(&mut self, port: u8, frame: &[u8], timeout_ms: u64) -> Option<Vec<u8>> {
        self.ops += 1;
        match self.uart_carry(port, frame) {
            (Some(r), delay) if delay < timeout_ms => {
                self.advance_ms(delay);
                Some(r)
            }
            _ => {
                self.advance_ms(timeout_ms);
                None
            }
        }
    }

    fn uart_carry(&mut self, port: u8, frame: &[u8]) -> (Option<Vec<u8>>, u64) {
        let now = self.now_ms;
        match self.uart.get_mut(&port) {
            Some(s) => s.carry(frame, now),
            None => (None, 0),
        }
    }

    /// Byte-stream side of UART port `port`.
    pub fn uart_write(&mut self, port: u8, bytes: &[u8]) {
        let Some(s) = self.uart.get_mut(&port) else {
            return;
        };
        for f in s.input.push(bytes, max_payload::UART) {
            let (resp, delay) = self.uart_carry(port, &f);
            self.advance_ms(delay);
            if let Some(s) = self.uart.get_mut(&port) {
                s.output.extend(resp.unwrap_or_default());
            }
        }
    }

    /// UART_RAW on UART port `port`: `write` goes to the node as it is,
    /// then up to `read_max` bytes come back, the first within `wait_ms`
    /// (nothing arrived: empty, after the whole wait). `None`: no such
    /// port.
    pub fn uart_raw(
        &mut self,
        port: u8,
        write: &[u8],
        read_max: usize,
        wait_ms: u64,
    ) -> Option<Vec<u8>> {
        // 115200 bit/s, 10 bits a byte.
        let byte_ms = |n: usize| (n as u64 * 10).div_ceil(115);
        self.ops += 1;
        let mut now = self.now_ms + byte_ms(write.len());
        let s = self.uart.get_mut(&port)?;
        let mut read = vec![0u8; read_max];
        let n = if s.link.silent {
            0
        } else {
            s.node.uart_raw_write(write, &mut now);
            s.node.uart_raw_read(&mut read)
        };
        read.truncate(n);
        now += match n {
            0 => wait_ms,
            n => byte_ms(n) + u64::from(dagon_proto::table::uart_raw::GAP_MS),
        };
        self.advance_ms(now - self.now_ms);
        Some(read)
    }

    /// Bytes received on UART port `port` and not yet taken.
    pub fn uart_pending(&self, port: u8) -> usize {
        self.uart.get(&port).map_or(0, |s| s.output.len())
    }

    /// Takes up to `buf.len()` received bytes of `port`; returns the count.
    pub fn uart_take(&mut self, port: u8, buf: &mut [u8]) -> usize {
        let Some(s) = self.uart.get_mut(&port) else {
            return 0;
        };
        let n = buf.len().min(s.output.len());
        buf[..n].copy_from_slice(&s.output[..n]);
        s.output.drain(..n);
        n
    }

    pub fn sub_mut(&mut self) -> Option<&mut SimNode> {
        let (port, i) = self.sub?;
        self.i2c.get_mut(&port)?.get_mut(i)
    }

    /// NRST of the sub node: `true` holds it low, `false` releases.
    pub fn set_reset(&mut self, asserted: bool) {
        let now = self.now_ms;
        if let Some(n) = self.sub_mut() {
            n.set_nrst_low(asserted, now);
        }
    }

    pub fn set_boot(&mut self, high: bool) {
        if let Some(n) = self.sub_mut() {
            n.set_boot0(high);
        }
    }

    /// SUBCTL semantics: BOOT0 first, then the NRST action (a pulse holds
    /// NRST low for [`NRST_PULSE_MS`]). `Io` if no node is wired.
    pub fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        self.ops += 1;
        if self.sub.is_none() {
            return Err(BusError::Io);
        }
        self.set_boot(boot0_high);
        match nrst {
            Nrst::Low => self.set_reset(true),
            Nrst::Release => self.set_reset(false),
            Nrst::Pulse => {
                self.set_reset(true);
                self.advance_ms(NRST_PULSE_MS);
                self.set_reset(false);
            }
        }
        Ok(())
    }
}

impl Default for SimBus {
    fn default() -> Self {
        SimBus::new()
    }
}

/// One port of a [`SimBus`] (see [`SimBus::port`]).
pub struct SimPort<'a> {
    pub(crate) bus: &'a mut SimBus,
    pub(crate) port: u8,
}

pub struct SimRelay {
    /// The relaying node's own agent (HELLO, STATUS, XFER_* to itself).
    pub node: SimNode,
    pub bus: SimBus,
    usb_in: FrameCutter,
    usb_out: Vec<u8>,
    /// The id of the MCU definition it was given.
    mcu: String,
    /// That definition's `[[link_port]]`: where media attach.
    ports: Vec<PortDef>,
    /// That definition's `[[relay_link]]`: the links RELAY reaches nodes over.
    links: Vec<RelayLinkDef>,
    /// The address I2C_RAW goes to: the ROM loader of the node wired to
    /// NRST/BOOT0, from its MCU definition (set by [`SimRelay::add_i2c`],
    /// as the MCU package's build generates it for the relay).
    pub rom_addr: Option<u8>,
    /// False: a relay built before PEERS (it answers `Unsupported`, as its
    /// agent does for any command it does not know).
    pub serves_peers: bool,
}

type Answer = (Status, Vec<u8>);

impl SimRelay {
    /// The relaying node with the built-in definition of its MCU (for
    /// when no definitions are given).
    pub fn new(mut node: SimNode) -> Self {
        let def = node.store().layout.definition();
        SimRelay::with_definition(node, def)
    }

    /// The relaying node with `def`, the effective definition of its MCU (a
    /// project's override included): its `[[link_port]]` places the media
    /// and its `[[relay_link]]` routes RELAY. Nothing else is read.
    pub fn with_definition(mut node: SimNode, def: &McuDef) -> Self {
        let mcu = node.store().layout.mcu;
        assert!(
            def.id == mcu,
            "the definition of {} given to a node of {mcu}",
            def.id
        );
        let always = [Cmd::Relay, Cmd::Peers, Cmd::UartRaw];
        node.also_serves(always.iter().fold(CmdSet::EMPTY, |s, c| s.with(*c as u8)));
        SimRelay {
            node,
            bus: SimBus::new(),
            usb_in: FrameCutter::default(),
            usb_out: Vec::new(),
            mcu: def.id.clone(),
            ports: def.ports.clone(),
            links: def.relay_link_defs(),
            rom_addr: None,
            serves_peers: true,
        }
    }

    /// The links RELAY reaches nodes over.
    pub fn links(&self) -> &[RelayLinkDef] {
        &self.links
    }

    /// The index of port `name` in its MCU definition (panics if it has
    /// none, or one of a different kind).
    pub fn port(&self, name: &str, kind: LinkKind) -> u8 {
        let p = self
            .ports
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("MCU definition {} has no port {name}", self.mcu));
        assert!(
            p.kind.name() == kind.name(),
            "port {name} of MCU definition {} is {}, not {}",
            self.mcu,
            p.kind.name(),
            kind.name()
        );
        p.index
    }

    /// The node a RELAY over link `link` to `addr` reaches: on an I2C link
    /// the one at `addr`, on a UART link the one behind the port.
    pub fn node_behind(&self, link: u8, addr: u8) -> Option<&SimNode> {
        let l = self.links.iter().find(|l| l.id == link)?;
        match l.kind {
            dagon_port::LinkKind::I2c => self
                .bus
                .i2c
                .get(&l.port)?
                .iter()
                .find(|n| n.i2c_addr == Some(addr)),
            dagon_port::LinkKind::Uart => self.bus.uart.get(&l.port).map(|s| &s.node),
            _ => None,
        }
    }

    /// [`SimRelay::node_behind`], to change.
    pub fn node_behind_mut(&mut self, link: u8, addr: u8) -> Option<&mut SimNode> {
        self.node_through(&[(link, addr)])
    }

    /// The node the RELAYs `hops` reach, each a link and an address at the
    /// relaying node before it, nearest first (none: this node itself).
    pub fn node_through(&mut self, hops: &[(u8, u8)]) -> Option<&mut SimNode> {
        let Some((&(link, addr), mut rest)) = hops.split_first() else {
            return Some(&mut self.node);
        };
        let (mut node, mut relaying) = behind(&mut self.bus, &self.links, link, addr)?;
        while let Some((&(link, addr), after)) = rest.split_first() {
            let r = relaying?;
            (node, relaying) = behind(&mut r.bus, &r.links, link, addr)?;
            rest = after;
        }
        Some(node)
    }

    pub fn now_ms(&self) -> u64 {
        self.bus.now_ms
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.bus.advance_ms(ms);
        self.node.tick(self.bus.now_ms);
    }

    /// Adds a node on I2C port `port` at its `i2c_addr`; `wired` connects
    /// NRST/BOOT0 to it and points I2C_RAW at its ROM loader.
    pub fn add_i2c(&mut self, port: &str, node: SimNode, wired: bool) -> usize {
        let p = self.port(port, LinkKind::I2c);
        if wired {
            self.rom_addr = node.rom_addr;
            self.node.also_serves(handler::cmds(&Self::TABLE));
        }
        let nodes = self.bus.i2c.entry(p).or_default();
        nodes.push(node);
        let i = nodes.len() - 1;
        if wired {
            self.bus.sub = Some((p, i));
        }
        i
    }

    /// The node configured at `addr` on I2C port `port` (present or not).
    pub fn i2c_node(&mut self, port: &str, addr: u8) -> Option<&mut SimNode> {
        let p = self.port(port, LinkKind::I2c);
        self.bus.i2c_node(p, addr)
    }

    /// Puts `node` behind `link` on UART port `port`.
    pub fn set_uart(&mut self, port: &str, link: SimLink, node: SimNode) {
        let p = self.port(port, LinkKind::Uart);
        self.bus.uart.insert(p, SimStream::new(link, node));
    }

    /// Puts the relaying node `relay`, with what its own ports reach,
    /// behind `link` on UART port `port`: it answers there what it answers
    /// the PC, for a requester whose frames are that link's.
    pub fn set_uart_relay(&mut self, port: &str, link: SimLink, relay: SimRelay) {
        let p = self.port(port, LinkKind::Uart);
        let mut stream = SimStream::new(link, relay.node);
        stream.relaying = Some(Box::new(Relaying {
            bus: relay.bus,
            links: relay.links,
            rom_addr: relay.rom_addr,
            serves_peers: relay.serves_peers,
        }));
        self.bus.uart.insert(p, stream);
    }

    /// The node and link on UART port `port`.
    pub fn uart(&mut self, port: &str) -> Option<&mut SimStream> {
        let p = self.port(port, LinkKind::Uart);
        self.bus.uart.get_mut(&p)
    }

    /// Byte-stream USB CDC: feed bytes from the PC.
    pub fn usb_write(&mut self, bytes: &[u8]) {
        for f in self.usb_in.push(bytes, max_payload::USB) {
            if let Some(r) = self.request(&f) {
                self.usb_out.extend_from_slice(&r);
            }
        }
    }

    /// Byte-stream USB CDC: bytes for the PC.
    pub fn usb_read(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.usb_out)
    }

    /// One request frame from the PC; returns the response frame (`None`:
    /// not a request, or the node's own agent is not answering).
    pub fn request(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        self.advance_ms(PC_ROUND_TRIP_MS);
        self.serving().request(frame)
    }

    /// This node answering a request of the PC's.
    fn serving(&mut self) -> Serving<'_> {
        Serving {
            node: &mut self.node,
            bus: &mut self.bus,
            links: &self.links,
            rom_addr: self.rom_addr,
            serves_peers: self.serves_peers,
            ingress: max_payload::USB,
        }
    }

    /// The commands the relay answers itself; [`SimRelay::request`] takes
    /// the typed request of a row and runs it, as the relay on a node.
    const TABLE: [Handler<(), ()>; 5] = [
        Handler::deferred(Cmd::Relay),
        Handler::deferred(Cmd::Peers),
        Handler::deferred(Cmd::UartRaw),
        Handler::deferred(Cmd::SubCtl),
        Handler::deferred(Cmd::I2cRaw),
    ];

    /// CAPS, answered here about RELAY and by the agent about the rest.
    const CAPS: Handler<(), ()> = Handler::deferred(Cmd::Caps);
}

/// A relaying node answering one request: its own agent, what its ports
/// reach, its relay table, and the largest payload of the link the request
/// came in on.
struct Serving<'a> {
    node: &'a mut SimNode,
    bus: &'a mut SimBus,
    links: &'a [RelayLinkDef],
    rom_addr: Option<u8>,
    serves_peers: bool,
    ingress: usize,
}

impl Serving<'_> {
    /// One request frame from whoever drives this node; returns the
    /// response frame (`None`: not a request, or the node's own agent is
    /// not answering).
    fn request(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        let f = match Frame::decode(frame, self.ingress) {
            Ok((f, _)) => f,
            Err(FrameError::BadVersion) => {
                let mut out = vec![0u8; frame_len(4)];
                let n = dagon_proto::version_refusal(frame, &mut out)?;
                out.truncate(n);
                return Some(out);
            }
            Err(_) => return None,
        };
        if f.flags & (flags::RESP | flags::EVENT) != 0 {
            return None;
        }
        // Its boot core relays nothing: it answers every request itself.
        if self.node.in_boot_core() {
            return self.node.handle_frame(frame, self.bus.now_ms);
        }
        // As the relay on a node: RELAY, PEERS and UART_RAW always, SUBCTL and
        // I2C_RAW with a node on the recovery lines, CAPS about RELAY; the
        // rest is the node's own agent.
        let wired = self.bus.sub.is_some();
        let row = handler::find(&SimRelay::TABLE, f.cmd)
            .filter(|h| wired || matches!(h.cmd, Cmd::Relay | Cmd::Peers | Cmd::UartRaw))
            .filter(|h| self.serves_peers || h.cmd != Cmd::Peers)
            .or((f.cmd == Cmd::Caps as u8).then_some(&SimRelay::CAPS));
        let Some(row) = row else {
            return self.node.handle_frame(frame, self.bus.now_ms);
        };
        // What the requester takes: the ingress of a tunnel, and the most
        // an answer may be.
        let ceiling = self.node.ceiling().over(self.ingress);
        let room = ceiling.bytes();
        // The whole request is read, and its answer held against the
        // ceiling, before any of it runs.
        let (status, bytes) = match row.request(f.payload, ceiling) {
            Err(refused) => (refused, Vec::new()),
            Ok(Req::Caps(c)) if c.cmd != Cmd::Relay as u8 => {
                return self.node.handle_frame(frame, self.bus.now_ms)
            }
            Ok(Req::Caps(c)) => self.caps(c, room),
            Ok(Req::Relay(r)) => self.relay(&r, room),
            Ok(Req::Peers(page)) => self.peers(page),
            Ok(Req::SubCtl(s)) => self.subctl(s),
            Ok(Req::I2cRaw(raw)) => self.i2c_raw(&raw),
            Ok(Req::UartRaw(raw)) => self.uart_raw(&raw),
            Ok(_) => (Status::Unsupported, Vec::new()),
        };
        let mut out = vec![0u8; frame_len(2 + bytes.len()).max(frame_len(8))];
        let body = &mut out[dagon_proto::HEADER_LEN..];
        let len = if status != Status::Ok {
            payload::put_refusal(body, status, f.cmd, f.payload)
        } else {
            body[0] = Status::Ok as u8;
            if row.spec.resp.bulk {
                1 + payload::encode_bulk_resp(&bytes, &mut body[1..]).ok()?
            } else {
                body.get_mut(1..1 + bytes.len())?.copy_from_slice(&bytes);
                1 + bytes.len()
            }
        };
        let n = dagon_proto::finish(&mut out, flags::RESP, f.seq, f.cmd, len).ok()?;
        out.truncate(n);
        Some(out)
    }

    /// CAPS about RELAY: its fields, and for the link asked about the
    /// tunnel over it for a requester that takes `room` payload bytes;
    /// `Arg` for a link the definition does not list. The answer's body.
    fn caps(&mut self, asked: CapsReq, room: usize) -> Answer {
        let tunnel = match asked.link.map(|id| self.links.iter().find(|l| l.id == id)) {
            None => None,
            Some(None) => return (Status::Arg, Vec::new()),
            Some(Some(l)) => {
                let mtu = Relay::tunnel_mtu(room, usize::from(l.max_payload));
                Some(u16::try_from(mtu).unwrap_or(u16::MAX))
            }
        };
        let answer = CapsResp {
            tunnel_mtu: tunnel,
            ..CapsResp::of(Cmd::Relay)
        };
        let mut body = vec![0u8; CapsResp::MAX_LEN];
        match answer.encode(&mut body) {
            Ok(n) => {
                body.truncate(n);
                (Status::Ok, body)
            }
            Err(_) => (Status::Arg, Vec::new()),
        }
    }

    /// RELAY over the link its id names: `Arg`, before anything is
    /// forwarded, for an id the table does not list, an address the link
    /// does not take or an inner frame the tunnel over that link does not
    /// admit for a requester that takes `room` payload bytes
    /// (`Relay::admits`); `Unsupported` for a link kind the simulator has
    /// no medium for.
    fn relay(&mut self, r: &Relay<'_>, room: usize) -> Answer {
        let t = u64::from(r.timeout_ms.unwrap_or(dagon_port::timeout::RELAY_WAIT_MS));
        let Some(link) = self
            .links
            .iter()
            .find(|l| l.id == r.link)
            .filter(|l| l.addressing.accepts(r.addr) && r.admits(room, usize::from(l.max_payload)))
            .copied()
        else {
            return (Status::Arg, Vec::new());
        };
        let resp = match link.kind {
            LinkKind::I2c => self.bus.i2c_exchange(link.port, r.addr, r.inner, t),
            LinkKind::Uart => self.bus.uart_exchange(link.port, r.inner, t),
            _ => return (Status::Unsupported, Vec::new()),
        };
        match resp {
            Some(f) => {
                // As the relay on a node: a peer that answers with a valid
                // response frame is recorded.
                let valid = Frame::decode(&f, usize::MAX)
                    .is_ok_and(|(inner, _)| inner.flags & flags::RESP != 0);
                if valid {
                    self.node.peer_answered(r.link, r.addr);
                }
                (Status::Ok, f)
            }
            None => (Status::Timeout, Vec::new()),
        }
    }

    /// PEERS: page `page` of the peers the node's state log records;
    /// `Unsupported` when they cannot be read.
    fn peers(&mut self, page: u8) -> Answer {
        let Some(log) = self.node.peers() else {
            return (Status::Unsupported, Vec::new());
        };
        let mut body = vec![0u8; PeersResp::MAX_LEN];
        match PeersResp::page(&log, page).encode(&mut body) {
            Ok(n) => {
                body.truncate(n);
                (Status::Ok, body)
            }
            Err(_) => (Status::Arg, Vec::new()),
        }
    }

    /// Raw bytes to and from the node on the UART link it names, as the
    /// relay on a node (`dagon_relay::cmd::uart_raw`): E_ARG for a link
    /// the definition does not list or that is no UART, and for data or a
    /// read longer than the link's buffer; E_UNSUPPORTED for a link
    /// nothing is wired to.
    fn uart_raw(&mut self, raw: &UartRaw<'_>) -> Answer {
        let Some(link) = self.links.iter().find(|l| l.id == raw.link) else {
            return (Status::Arg, Vec::new());
        };
        let limit = link.buffer_len();
        if link.kind != LinkKind::Uart
            || usize::from(raw.read_max) > limit
            || raw.data.len() > limit
        {
            return (Status::Arg, Vec::new());
        }
        let wait = u64::from(raw.wait_ms);
        match self
            .bus
            .uart_raw(link.port, raw.data, raw.read_max.into(), wait)
        {
            Some(read) => (Status::Ok, read),
            None => (Status::Unsupported, Vec::new()),
        }
    }

    /// Whether a SUBCTL or I2C_RAW names the one recovery target here, the
    /// wired node (`dagon_relay::cmd::recovery_target` over one target):
    /// naming none is naming it; another link or loader address is E_ARG.
    fn names_the_wired(&self, asked: Option<RomTarget>) -> Result<(), Status> {
        let Some(t) = asked else {
            return Ok(());
        };
        let link = self.bus.sub.and_then(|(port, _)| {
            self.links
                .iter()
                .find(|l| l.port == port && l.kind == LinkKind::I2c)
        });
        match (link, self.rom_addr) {
            (Some(l), Some(rom)) if (l.id, rom) == (t.link, t.rom_addr) => Ok(()),
            _ => Err(Status::Arg),
        }
    }

    fn subctl(&mut self, s: SubCtl) -> Answer {
        if let Err(e) = self.names_the_wired(s.target) {
            return (e, Vec::new());
        }
        let line = match s.nrst {
            nrst::LOW => Nrst::Low,
            nrst::RELEASE => Nrst::Release,
            _ => Nrst::Pulse,
        };
        match self.bus.set_lines(line, s.boot0 == boot0::HIGH) {
            Ok(()) => {
                // The wired node's link, as the firmware relay tells its
                // agent (0 for a table without that port).
                let link = self.bus.sub.and_then(|(port, _)| {
                    self.links
                        .iter()
                        .find(|l| l.port == port && l.kind == LinkKind::I2c)
                });
                self.node
                    .child_control(link.map_or(0, |l| l.id), s.nrst, s.boot0);
                (Status::Ok, Vec::new())
            }
            Err(_) => (dagon_proto::table::subctl::LINES_FAILED, Vec::new()),
        }
    }

    /// Raw transactions to the ROM loader on the wired node's port,
    /// accepted only while BOOT0 is high, each within the buffer of that
    /// port's relay link (`RelayLinkDef::buffer_len`, the relay's limit
    /// too): E_ARG for longer data or reads.
    fn i2c_raw(&mut self, raw: &I2cRaw<'_>) -> Answer {
        if let Err(e) = self.names_the_wired(raw.target) {
            return (e, Vec::new());
        }
        if !self.bus.sub_mut().is_some_and(|n| n.boot0()) {
            return (Status::State, Vec::new());
        }
        let Some((port, _)) = self.bus.sub else {
            return (Status::State, Vec::new());
        };
        let Some(link) = self
            .links
            .iter()
            .find(|l| l.port == port && l.kind == LinkKind::I2c)
        else {
            return (Status::Unsupported, Vec::new());
        };
        let Some(rom) = self.rom_addr else {
            return (Status::Unsupported, Vec::new());
        };
        let limit = link.buffer_len();
        if usize::from(raw.read_len) > limit || raw.data.len() > limit {
            return (Status::Arg, Vec::new());
        }
        if !raw.data.is_empty() && self.bus.i2c_write(port, rom, raw.data).is_err() {
            return (Status::Timeout, Vec::new());
        }
        let mut buf = vec![0u8; raw.read_len as usize];
        if !buf.is_empty() && self.bus.i2c_read(port, rom, &mut buf).is_err() {
            return (Status::Timeout, Vec::new());
        }
        (Status::Ok, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines that cannot be set get the status the protocol gives that
    /// failure, as the relay on a node answers it.
    #[test]
    fn lines_that_cannot_be_set_answer_the_protocols_status() {
        let mut relay = crate::sample_hub(7);
        relay.bus.sub = None;
        let asked = SubCtl {
            nrst: nrst::PULSE,
            boot0: boot0::HIGH,
            target: None,
        };
        assert_eq!(relay.serving().subctl(asked).0, Status::Timeout);
        assert_eq!(
            dagon_proto::table::subctl::LINES_FAILED,
            Status::Timeout,
            "the hub's SUBCTL port reads this status as a timeout"
        );
    }
}
