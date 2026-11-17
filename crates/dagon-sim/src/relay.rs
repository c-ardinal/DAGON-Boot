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
use crate::rom::ROM_ADDR;
use dagon_port::{BusError, LinkKind, Nrst, RelayLinkDef};
use dagon_proto::mailbox::{State, STATE_LEN};
use dagon_proto::payload::{boot0, nrst, I2cRaw, Relay, SubCtl};
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
                Err(_) => {
                    self.buf.drain(..1);
                }
            }
        }
        frames
    }
}

/// The node behind one UART port, and that port's byte stream.
pub struct SimStream {
    pub link: SimLink,
    pub node: SimNode,
    input: FrameCutter,
    output: Vec<u8>,
}

impl SimStream {
    pub fn new(link: SimLink, node: SimNode) -> Self {
        SimStream {
            link,
            node,
            input: FrameCutter::default(),
            output: Vec::new(),
        }
    }

    /// Request out, response back; returns it with the round-trip delay.
    fn carry(&mut self, frame: &[u8], now: u64) -> (Option<Vec<u8>>, u64) {
        let d = self.link.delay_ms;
        let resp = self
            .link
            .carry(frame)
            .filter(|_| !self.link.silent)
            .and_then(|f| self.node.handle_frame(&f, now + d))
            .and_then(|r| self.link.carry(&r));
        (resp, 2 * d)
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
}

impl SimBus {
    pub fn new() -> Self {
        SimBus {
            now_ms: 0,
            i2c: BTreeMap::new(),
            uart: BTreeMap::new(),
            sub: None,
        }
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.now_ms += ms;
        let now = self.now_ms;
        self.i2c.values_mut().flatten().for_each(|n| n.tick(now));
        self.uart.values_mut().for_each(|s| s.node.tick(now));
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
}

type Answer = (Status, Vec<u8>);

impl SimRelay {
    /// The relaying node with the built-in definition of its MCU (for
    /// when no definitions are given).
    pub fn new(mut node: SimNode) -> Self {
        let def = crate::store::simulated_mcu(node.store().layout.mcu);
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
        SimRelay {
            node,
            bus: SimBus::new(),
            usb_in: FrameCutter::default(),
            usb_out: Vec::new(),
            mcu: def.id.clone(),
            ports: def.ports.clone(),
            links: def.relay_link_defs(),
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

    pub fn now_ms(&self) -> u64 {
        self.bus.now_ms
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.bus.advance_ms(ms);
        self.node.tick(self.bus.now_ms);
    }

    /// Adds a node on I2C port `port` at its `i2c_addr`; `wired` connects
    /// NRST/BOOT0 to it.
    pub fn add_i2c(&mut self, port: &str, node: SimNode, wired: bool) -> usize {
        let p = self.port(port, LinkKind::I2c);
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
        let (f, _) = Frame::decode(frame, max_payload::USB).ok()?;
        if f.flags & (flags::RESP | flags::EVENT) != 0 {
            return None;
        }
        let (status, body) = match Cmd::from_u8(f.cmd) {
            Some(Cmd::Relay) => self.relay(f.payload),
            Some(Cmd::SubCtl) => self.subctl(f.payload),
            Some(Cmd::I2cRaw) => self.i2c_raw(f.payload),
            _ => return self.node.handle_frame(frame, self.bus.now_ms),
        };
        let mut out = vec![0u8; frame_len(1 + body.len())];
        out[8] = status as u8;
        out[9..9 + body.len()].copy_from_slice(&body);
        dagon_proto::finish(&mut out, flags::RESP, f.seq, f.cmd, 1 + body.len()).ok()?;
        Some(out)
    }

    /// RELAY over the link its id names: `Arg` for an id the table does not
    /// list (or an address or size the link does not take), `Unsupported`
    /// for a link kind the simulator has no medium for.
    fn relay(&mut self, p: &[u8]) -> Answer {
        let r = match Relay::parse(p) {
            Ok(r) => r,
            Err(s) => return (s, Vec::new()),
        };
        let t = r.timeout_ms as u64;
        let Some(link) = self
            .links
            .iter()
            .find(|l| l.id == r.link)
            .filter(|l| l.fits(r.addr, r.inner.len()))
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
            Some(f) => (Status::Ok, f),
            None => (Status::Timeout, Vec::new()),
        }
    }

    fn subctl(&mut self, p: &[u8]) -> Answer {
        let s = match SubCtl::parse(p) {
            Ok(s) => s,
            Err(e) => return (e, Vec::new()),
        };
        let line = match s.nrst {
            nrst::LOW => Nrst::Low,
            nrst::RELEASE => Nrst::Release,
            _ => Nrst::Pulse,
        };
        match self.bus.set_lines(line, s.boot0 == boot0::HIGH) {
            Ok(()) => (Status::Ok, Vec::new()),
            Err(_) => (Status::Unsupported, Vec::new()),
        }
    }

    /// Raw transactions to the ROM loader on the wired node's port,
    /// accepted only while BOOT0 is high, each within the buffer of that
    /// port's relay link (one frame of its max_payload, as the relay's
    /// `cmd::buffer_len`): E_ARG for longer data or reads.
    fn i2c_raw(&mut self, p: &[u8]) -> Answer {
        let raw = match I2cRaw::parse(p) {
            Ok(r) => r,
            Err(e) => return (e, Vec::new()),
        };
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
        let limit = frame_len(link.max_payload.into());
        if usize::from(raw.read_len) > limit || raw.data.len() > limit {
            return (Status::Arg, Vec::new());
        }
        if !raw.data.is_empty() && self.bus.i2c_write(port, ROM_ADDR, raw.data).is_err() {
            return (Status::Timeout, Vec::new());
        }
        let mut buf = vec![0u8; raw.read_len as usize];
        if !buf.is_empty() && self.bus.i2c_read(port, ROM_ADDR, &mut buf).is_err() {
            return (Status::Timeout, Vec::new());
        }
        (Status::Ok, buf)
    }
}
