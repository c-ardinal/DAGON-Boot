//! Cooperative ROM path (an MCU whose ROM has a serial loader, the
//! ESP32-S3): the node switches itself to the loader on ENTER_ROM, and the
//! relaying node's UART_RAW carries the loader's bytes. The recovery agent
//! (image kind `agent`) is rewritten through it: its body first, its
//! header last, each checked by the loader's MD5 of the region.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use dagon_ed25519::Ed25519Compact;
use dagon_format::header::{self, image_kind, ImageHeader, McuId, HEADER_SIZE};
use dagon_format::topology::{iface, rom_entry, TopologyView};
use dagon_format::{sha256, FormatError};
use dagon_port::{BusError, StreamPort};
use dagon_proto::payload::{self, phase, UartRaw};
use dagon_proto::Cmd;
use dagon_romlink::esp::EspLink;
use dagon_romlink::{HostClock, Region};

use crate::rom::RomSlot;
use crate::{Error, Event, Hop, Host, Target, Transport};

/// The most bytes one UART_RAW sends or reads: within a UART link's
/// buffer and what the connected node's uplink carries around it.
pub const RAW_CHUNK: usize = 1000;
/// The longest one UART_RAW waits for the first byte; a longer wait of
/// the loader's client is several of them.
pub const RAW_WAIT_MAX_MS: u32 = 10_000;
/// How much longer than a UART_RAW's own wait its answer may take: the
/// bytes on the line both ways and the relaying node's turn.
pub const RAW_MARGIN_MS: u32 = 1_500;
/// How long SYNC is sent while a person brings a node that does not
/// answer into its loader by hand.
pub const MANUAL_SYNC_MS: u32 = 60_000;

/// Where the serial ROM path of one MCU writes the recovery agent (the
/// caller builds it from the Registry: no std here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialRomMap {
    /// Image header `mcu_id` of the MCU this map is for.
    pub mcu_id: McuId,
    /// The flash size the loader is told.
    pub flash_bytes: u32,
    /// The recovery agent's slot.
    pub agent: RomSlot,
}

/// The maps of every MCU in `reg` whose recovery agent the serial ROM
/// path rewrites
/// ([`McuDef::has_serial_rom`](dagon_topology::defs::McuDef::has_serial_rom)).
#[cfg(feature = "defs")]
pub fn serial_rom_maps(reg: &dagon_topology::defs::Registry) -> Vec<SerialRomMap> {
    use dagon_topology::defs::ImageKind;
    reg.mcus()
        .iter()
        .filter(|m| m.has_serial_rom())
        .filter_map(|m| {
            let s = m.slot_a(ImageKind::Agent)?;
            Some(SerialRomMap {
                mcu_id: m.mcu_id(),
                flash_bytes: m.flash_maps.first()?.bytes,
                agent: RomSlot {
                    header: s.header_addr,
                    load: s.load_addr,
                    max_payload: s.max_payload,
                },
            })
        })
        .collect()
}

/// Checks a signed recovery agent image for `map`'s MCU: signature under
/// `pubkey`, kind, MCU, payload digest and size (it must fit its slot).
pub fn check_agent(
    img: &[u8],
    pubkey: &[u8; 32],
    map: &SerialRomMap,
) -> Result<ImageHeader, FormatError> {
    let raw: &[u8; HEADER_SIZE] = img
        .get(..HEADER_SIZE)
        .and_then(|h| h.try_into().ok())
        .ok_or(FormatError::Truncated)?;
    let h = header::verify_header(raw, &Ed25519Compact::new(*pubkey))?;
    let body = &img[HEADER_SIZE..];
    let fits = body.len() as u64 <= u64::from(map.agent.max_payload);
    if h.image_kind != image_kind::AGENT || h.mcu_id != map.mcu_id || !fits {
        return Err(FormatError::BadField);
    }
    if h.payload_size as usize != body.len() || sha256(body) != h.payload_sha256 {
        return Err(FormatError::DigestMismatch);
    }
    Ok(h)
}

/// The regions the loader writes for the agent image `img`, in order: the
/// body, then the header, so that a write cut short leaves no header over
/// a partial body. A header that sits right before its body shares the
/// body's first erase sector, and the two go as one region.
pub fn regions<'a>(img: &'a [u8], map: &SerialRomMap) -> Vec<Region<'a>> {
    let (head, body) = img.split_at(HEADER_SIZE);
    let r = |addr, data| Region { addr, data };
    if map.agent.header + HEADER_SIZE as u32 == map.agent.load {
        return alloc::vec![r(map.agent.header, img)];
    }
    alloc::vec![r(map.agent.load, body), r(map.agent.header, head)]
}

/// The ENTER_ROM method the topology's UART link to `node` names
/// (`control.rom_entry`); `None` when no such link asks for one.
pub fn enter_method(topo: &TopologyView<'_>, node: u8) -> Option<u8> {
    (0..topo.link_count())
        .map(|i| topo.link(i))
        .filter(|l| l.iface != iface::I2C_BUS && l.end(node).is_some())
        .map(|l| l.rom_entry)
        .find(|&m| m != rom_entry::NONE && m != rom_entry::I2C_LOADER)
}

/// The byte stream to a node's ROM serial loader through the relaying
/// node it is linked to, with progress reported as events of phase ROM.
///
/// A write is kept until the next read and goes out with it in one
/// UART_RAW, so the relaying node listens from the moment the request's
/// last byte left: what the loader answers while nobody listens is lost.
pub(crate) struct HubSerial<'h, T: Transport, C: HostClock> {
    host: &'h mut Host<T, C>,
    on_event: &'h mut dyn FnMut(Event),
    node: u8,
    relay: Target,
    link: u8,
    chunk: usize,
    version: dagon_format::Version,
    shown: u8,
    out: Vec<u8>,
    got: VecDeque<u8>,
}

impl<'h, T: Transport, C: HostClock> HubSerial<'h, T, C> {
    pub fn new(
        host: &'h mut Host<T, C>,
        on_event: &'h mut dyn FnMut(Event),
        node: u8,
        relay: Target,
        hop: Hop,
        version: dagon_format::Version,
    ) -> Self {
        let buffer = dagon_proto::frame_len(usize::from(hop.max_payload));
        HubSerial {
            host,
            on_event,
            node,
            relay,
            link: hop.link,
            chunk: RAW_CHUNK.min(buffer),
            version,
            shown: 0,
            out: Vec::new(),
            got: VecDeque::new(),
        }
    }

    /// One UART_RAW: `data` out, up to `read_max` bytes back.
    async fn raw(&mut self, data: &[u8], read_max: usize, wait_ms: u32) -> Result<(), BusError> {
        let req = UartRaw {
            link: self.link,
            read_max: read_max as u16,
            wait_ms: wait_ms as u16,
            data,
        };
        let p = crate::request(|b| req.encode(b));
        let within = wait_ms + RAW_MARGIN_MS;
        let body = match self.host.call(self.relay, Cmd::UartRaw, &p, within).await {
            Ok(b) => b,
            Err(Error::Timeout | Error::Relay(_)) => return Err(BusError::Timeout),
            Err(_) => return Err(BusError::Io),
        };
        let read = payload::parse_bulk_resp(Cmd::UartRaw, &body).map_err(|_| BusError::Io)?;
        self.got.extend(read);
        Ok(())
    }
}

impl<T: Transport, C: HostClock> StreamPort for HubSerial<'_, T, C> {
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, BusError> {
        let until = self.host.clock.now_ms() + u64::from(timeout_ms);
        while self.got.is_empty() {
            let out = core::mem::take(&mut self.out);
            // All but the last piece go out without a read.
            let last = out.len().saturating_sub(1) / self.chunk * self.chunk;
            for piece in out[..last].chunks(self.chunk) {
                self.raw(piece, 0, 0).await?;
            }
            let left = until.saturating_sub(self.host.clock.now_ms());
            if left == 0 && out.is_empty() {
                return Err(BusError::Timeout);
            }
            let wait = (left as u32).min(RAW_WAIT_MAX_MS);
            self.raw(&out[last..], self.chunk, wait).await?;
        }
        let n = buf.len().min(self.got.len());
        for (b, g) in buf.iter_mut().zip(self.got.drain(..n)) {
            *b = g;
        }
        Ok(n)
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), BusError> {
        self.out.extend_from_slice(data);
        Ok(())
    }
}

impl<T: Transport, C: HostClock> HostClock for HubSerial<'_, T, C> {
    fn now_ms(&self) -> u64 {
        self.host.clock.now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.host.clock.sleep_ms(ms).await
    }
}

impl<T: Transport, C: HostClock> EspLink for HubSerial<'_, T, C> {
    async fn progress(&mut self, percent: u8) {
        const STEP: u8 = 10;
        if percent < self.shown + STEP {
            return;
        }
        self.shown = percent - percent % STEP;
        let (node, v, p) = (self.node, self.version, self.shown);
        self.host
            .report(&mut *self.on_event, node, phase::ROM, p, v);
    }
}
