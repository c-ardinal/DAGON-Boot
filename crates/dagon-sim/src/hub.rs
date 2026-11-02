//! Simulated hub: its own agent on USB, an I2C bus of SimNodes (absent
//! addresses NACK), one UART node behind a SimLink, the NRST/BOOT0 lines of
//! one I2C node, and the hub commands RELAY, SUBCTL, I2C_RAW and SHOW.
//!
//! Time is simulated: [`SimBus::now_ms`] only moves when something waits
//! (mailbox polls, link delays, ROM work, [`SimHub::advance_ms`]) and by
//! [`PC_ROUND_TRIP_MS`] per request.

use crate::link::SimLink;
use crate::node::SimNode;
use crate::rom::ROM_ADDR;
use dagon_format::Version;
use dagon_port::BusError;
use dagon_proto::mailbox::{State, STATE_LEN};
use dagon_proto::payload::{link, nrst, I2cRaw, Relay, Show, SubCtl};
use dagon_proto::{find_sof, flags, frame_len, max_payload, Cmd, Frame, FrameError, Status};

/// Cost of one PC <-> hub request on the simulated clock.
pub const PC_ROUND_TRIP_MS: u64 = 2;
/// Mailbox poll interval on I2C.
pub const POLL_MS: u64 = 2;
/// Low time of SUBCTL `nrst = PULSE`.
pub const NRST_PULSE_MS: u64 = 10;

/// What the hub's LCD shows (last SHOW).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub node: u8,
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
    pub text: Vec<u8>,
}

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

pub struct SimBus {
    pub now_ms: u64,
    pub i2c: Vec<SimNode>,
    pub uart: Option<(SimLink, SimNode)>,
    /// Index into `i2c` of the node wired to NRST and BOOT0.
    pub sub: Option<usize>,
    uart_in: FrameCutter,
    uart_out: Vec<u8>,
}

impl SimBus {
    pub fn new() -> Self {
        SimBus {
            now_ms: 0,
            i2c: Vec::new(),
            uart: None,
            sub: None,
            uart_in: FrameCutter::default(),
            uart_out: Vec::new(),
        }
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.now_ms += ms;
        let now = self.now_ms;
        self.i2c.iter_mut().for_each(|n| n.tick(now));
        if let Some((_, n)) = &mut self.uart {
            n.tick(now);
        }
    }

    fn target(&mut self, addr: u8) -> Option<&mut SimNode> {
        self.i2c.iter_mut().find(|n| n.i2c_present(addr))
    }

    pub fn i2c_write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
        let mut now = self.now_ms;
        let r = match self.target(addr) {
            Some(n) => n.i2c_write(addr, data, &mut now),
            None => Err(BusError::Nack),
        };
        if now > self.now_ms {
            self.advance_ms(now - self.now_ms);
        }
        r
    }

    pub fn i2c_read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        let now = self.now_ms;
        match self.target(addr) {
            Some(n) => n.i2c_read(addr, buf, now),
            None => Err(BusError::Nack),
        }
    }

    /// 7-bit addresses 0x08..=0x77 that ACK an address-only write.
    pub fn scan(&mut self) -> Vec<u8> {
        (0x08..=0x77u8)
            .filter(|&a| self.i2c_write(a, &[]).is_ok())
            .collect()
    }

    /// RELAY over I2C: write the request (retrying NACK), poll the 3-byte
    /// state every [`POLL_MS`], read the response.
    pub fn i2c_exchange(&mut self, addr: u8, frame: &[u8], timeout_ms: u64) -> Option<Vec<u8>> {
        let deadline = self.now_ms + timeout_ms;
        while self.i2c_write(addr, frame).is_err() {
            if self.now_ms >= deadline {
                return None;
            }
            self.advance_ms(POLL_MS);
        }
        loop {
            self.advance_ms(POLL_MS);
            let mut st = [0u8; STATE_LEN];
            let state = self
                .i2c_read(addr, &mut st)
                .ok()
                .and_then(|_| State::parse(st));
            if let Some(State::Ready(n)) = state {
                let mut resp = vec![0u8; n as usize];
                return self.i2c_read(addr, &mut resp).ok().map(|_| resp);
            }
            if self.now_ms >= deadline {
                return None;
            }
        }
    }

    /// RELAY over UART: through the link both ways; delays count against
    /// `timeout_ms`.
    pub fn uart_exchange(&mut self, frame: &[u8], timeout_ms: u64) -> Option<Vec<u8>> {
        match self.uart_carry(frame) {
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

    /// Request out, response back; returns it with the round-trip delay.
    fn uart_carry(&mut self, frame: &[u8]) -> (Option<Vec<u8>>, u64) {
        let now = self.now_ms;
        let Some((l, node)) = &mut self.uart else {
            return (None, 0);
        };
        let d = l.delay_ms;
        let resp = l
            .carry(frame)
            .filter(|_| !l.silent)
            .and_then(|f| node.handle_frame(&f, now + d))
            .and_then(|r| l.carry(&r));
        (resp, 2 * d)
    }

    /// Byte-stream side of the UART (what the hub's PIO UART sees).
    pub fn uart_write(&mut self, bytes: &[u8]) {
        for f in self.uart_in.push(bytes, max_payload::UART) {
            let (resp, delay) = self.uart_carry(&f);
            self.advance_ms(delay);
            self.uart_out.extend(resp.unwrap_or_default());
        }
    }

    /// Bytes received from the UART node and not yet taken.
    pub fn uart_pending(&self) -> usize {
        self.uart_out.len()
    }

    /// Takes up to `buf.len()` received bytes; returns the count.
    pub fn uart_take(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.uart_out.len());
        buf[..n].copy_from_slice(&self.uart_out[..n]);
        self.uart_out.drain(..n);
        n
    }

    pub fn sub_mut(&mut self) -> Option<&mut SimNode> {
        self.sub.and_then(|i| self.i2c.get_mut(i))
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
}

impl Default for SimBus {
    fn default() -> Self {
        SimBus::new()
    }
}

pub struct SimHub {
    /// The hub's own agent (HELLO, STATUS, XFER_* to the hub itself).
    pub node: SimNode,
    pub bus: SimBus,
    pub screen: Option<Screen>,
    usb_in: FrameCutter,
    usb_out: Vec<u8>,
}

type Answer = (Status, Vec<u8>);

impl SimHub {
    pub fn new(node: SimNode) -> Self {
        SimHub {
            node,
            bus: SimBus::new(),
            screen: None,
            usb_in: FrameCutter::default(),
            usb_out: Vec::new(),
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.bus.now_ms
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.bus.advance_ms(ms);
        self.node.tick(self.bus.now_ms);
    }

    /// Adds an I2C node at its `i2c_addr`; `wired` connects NRST/BOOT0 to it.
    pub fn add_i2c(&mut self, node: SimNode, wired: bool) -> usize {
        self.bus.i2c.push(node);
        let i = self.bus.i2c.len() - 1;
        if wired {
            self.bus.sub = Some(i);
        }
        i
    }

    /// The I2C node configured at `addr` (present or not).
    pub fn i2c_node(&mut self, addr: u8) -> Option<&mut SimNode> {
        self.bus.i2c.iter_mut().find(|n| n.i2c_addr == Some(addr))
    }

    pub fn set_uart(&mut self, link: SimLink, node: SimNode) {
        self.bus.uart = Some((link, node));
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
    /// not a request, or the hub's own agent is not answering).
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
            Some(Cmd::Show) => self.show(f.payload),
            _ => return self.node.handle_frame(frame, self.bus.now_ms),
        };
        let mut out = vec![0u8; frame_len(1 + body.len())];
        out[8] = status as u8;
        out[9..9 + body.len()].copy_from_slice(&body);
        dagon_proto::finish(&mut out, flags::RESP, f.seq, f.cmd, 1 + body.len()).ok()?;
        Some(out)
    }

    fn relay(&mut self, p: &[u8]) -> Answer {
        let r = match Relay::parse(p) {
            Ok(r) => r,
            Err(s) => return (s, Vec::new()),
        };
        let t = r.timeout_ms as u64;
        let resp = match (r.link, r.addr) {
            (link::I2C0, a) if a < 0x80 && r.inner.len() <= frame_len(max_payload::I2C) => {
                self.bus.i2c_exchange(a, r.inner, t)
            }
            (link::PIO_UART, link::UART_ADDR) if r.inner.len() <= frame_len(max_payload::UART) => {
                self.bus.uart_exchange(r.inner, t)
            }
            _ => return (Status::Arg, Vec::new()),
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
        if self.bus.sub.is_none() {
            return (Status::Unsupported, Vec::new());
        }
        self.bus
            .set_boot(s.boot0 == dagon_proto::payload::boot0::HIGH);
        match s.nrst {
            nrst::LOW => self.bus.set_reset(true),
            nrst::RELEASE => self.bus.set_reset(false),
            _ => {
                self.bus.set_reset(true);
                self.advance_ms(NRST_PULSE_MS);
                self.bus.set_reset(false);
            }
        }
        (Status::Ok, Vec::new())
    }

    /// Raw transactions to the ROM loader, accepted only while BOOT0 is high.
    fn i2c_raw(&mut self, p: &[u8]) -> Answer {
        let raw = match I2cRaw::parse(p) {
            Ok(r) => r,
            Err(e) => return (e, Vec::new()),
        };
        if !self.bus.sub_mut().is_some_and(|n| n.boot0()) {
            return (Status::State, Vec::new());
        }
        if raw.read_len as usize > max_payload::USB - 1 {
            return (Status::Arg, Vec::new());
        }
        if !raw.data.is_empty() && self.bus.i2c_write(ROM_ADDR, raw.data).is_err() {
            return (Status::Timeout, Vec::new());
        }
        let mut buf = vec![0u8; raw.read_len as usize];
        if !buf.is_empty() && self.bus.i2c_read(ROM_ADDR, &mut buf).is_err() {
            return (Status::Timeout, Vec::new());
        }
        (Status::Ok, buf)
    }

    fn show(&mut self, p: &[u8]) -> Answer {
        match Show::parse(p) {
            Ok(s) => {
                self.screen = Some(Screen {
                    node: s.node,
                    phase: s.phase,
                    percent: s.percent,
                    version: s.version,
                    text: s.text.to_vec(),
                });
                (Status::Ok, Vec::new())
            }
            Err(e) => (e, Vec::new()),
        }
    }
}
