//! How long the links and the flash take, so a simulated write runs about
//! as long as it does on hardware: the uplink between the PC and the node
//! it is connected to (USB CDC, a USB-UART adapter, Wi-Fi TCP), each relay
//! link at the rate of its port's kind, and the sector erase and final
//! check of a transfer.
//!
//! [`TimedRelay`] holds each answer of a [`SimRelay`] back until its
//! simulated arrival time. `dagon demo` (serve) and the browser demo both
//! drive the simulator through it; only their clocks differ (serve follows
//! the wall clock, the browser advances the simulator while it waits).

use std::collections::VecDeque;

use dagon_proto::payload::{Relay, XferBegin, XferChunk};
use dagon_proto::{max_payload, Cmd, HEADER_LEN};
use dagon_topology::schema::LinkKind;

use crate::SimRelay;
use dagon_port::RelayLinkDef;

/// The uplink simulated between the PC and the node it is connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// USB CDC: fast, gone while the node reboots.
    UsbCdc,
    /// A USB-UART adapter at 115200 baud: slow, smaller frames, stays.
    Uart,
    /// A TCP connection over Wi-Fi: some latency, reconnects after a reboot.
    WifiTcp,
}

impl Medium {
    pub const ALL: [Medium; 3] = [Medium::UsbCdc, Medium::Uart, Medium::WifiTcp];

    /// The medium for an uplink kind; None for kinds the simulator does not have.
    pub fn of(kind: LinkKind) -> Option<Medium> {
        match kind {
            LinkKind::UsbCdc => Some(Medium::UsbCdc),
            LinkKind::Uart => Some(Medium::Uart),
            LinkKind::WifiTcp => Some(Medium::WifiTcp),
            _ => None,
        }
    }

    pub fn kind(self) -> LinkKind {
        match self {
            Medium::UsbCdc => LinkKind::UsbCdc,
            Medium::Uart => LinkKind::Uart,
            Medium::WifiTcp => LinkKind::WifiTcp,
        }
    }

    pub fn max_payload(self) -> usize {
        match self {
            Medium::UsbCdc | Medium::WifiTcp => max_payload::USB,
            Medium::Uart => max_payload::UART,
        }
    }

    /// Whether the link stays open while the node reboots (a USB-UART
    /// adapter does; USB CDC and TCP go away and are opened again).
    pub fn stays_on_reboot(self) -> bool {
        self == Medium::Uart
    }

    /// Bytes per simulated ms, and the fixed cost of one exchange.
    fn rate_and_latency(self) -> (f64, f64) {
        match self {
            Medium::UsbCdc => (USB_BYTES_PER_MS, EXCHANGE_MS),
            Medium::Uart => (UART_BYTES_PER_MS, EXCHANGE_MS),
            Medium::WifiTcp => (WIFI_BYTES_PER_MS, WIFI_LATENCY_MS),
        }
    }
}

/// Bytes per simulated ms on each link (USB CDC; I2C at 400 kHz and the
/// UART at 115200 baud, with their framing).
const USB_BYTES_PER_MS: f64 = 1_000.0;
/// Wi-Fi TCP: a few hundred kB/s, with a round trip of several ms.
const WIFI_BYTES_PER_MS: f64 = 300.0;
const WIFI_LATENCY_MS: f64 = 8.0;
const I2C_BYTES_PER_MS: f64 = 25.0;
const UART_BYTES_PER_MS: f64 = 11.0;
/// Fixed cost of one exchange (USB round trip, the relaying node's work).
const EXCHANGE_MS: f64 = 2.0;
/// The final check after a transfer (the erase before it is the node's
/// own, [`SimNode::erase_ms`](crate::SimNode::erase_ms)).
const DIGEST_CHECK_MS: f64 = 150.0;

/// Bytes per simulated ms over a relay link of `kind`; `None` for a kind
/// the simulator has no medium for (its RELAY is refused at once).
fn link_rate(kind: dagon_port::LinkKind) -> Option<f64> {
    match kind {
        dagon_port::LinkKind::I2c => Some(I2C_BYTES_PER_MS),
        dagon_port::LinkKind::Uart => Some(UART_BYTES_PER_MS),
        _ => None,
    }
}

/// Bytes per simulated ms over a link of `kind`, whichever end it is:
/// the relay links' rate, or the uplink's for a kind only an uplink has.
pub(crate) fn kind_bytes_per_ms(kind: LinkKind) -> Option<f64> {
    dagon_port::LinkKind::from_name(kind.name())
        .and_then(link_rate)
        .or_else(|| Some(Medium::of(kind)?.rate_and_latency().0))
}

/// The node's own work on request `f`: the final check after a transfer,
/// and the ms `erase` gives for it (`relay` the RELAY carrying it).
fn work(
    f: &[u8],
    relay: Option<&Relay<'_>>,
    erase: impl FnOnce(Option<&Relay<'_>>, u8, &[u8]) -> u32,
) -> f64 {
    let cmd = f.get(5).copied().unwrap_or(0);
    if cmd == Cmd::XferEnd as u8 {
        return DIGEST_CHECK_MS;
    }
    let payload = f.get(HEADER_LEN..f.len().saturating_sub(4)).unwrap_or(&[]);
    f64::from(erase(relay, cmd, payload))
}

/// Simulated ms the exchange of request `frame` takes on its links: the
/// uplink, and for a RELAY the link of `links` its id names, at the rate of
/// that link's kind (an id `links` does not list is refused at once). The
/// request (its command and payload) adds the ms `erase` gives for it: to
/// the relaying node itself (`None`), or over the RELAY that carries it.
pub fn exchange_ms(
    frame: &[u8],
    medium: Medium,
    links: &[RelayLinkDef],
    erase: impl FnOnce(Option<&Relay<'_>>, u8, &[u8]) -> u32,
) -> f64 {
    let cmd = frame.get(5).copied().unwrap_or(0);
    let payload = frame
        .get(HEADER_LEN..frame.len().saturating_sub(4))
        .unwrap_or(&[]);
    let (up_rate, latency) = medium.rate_and_latency();
    // The request up and its answer back, over the uplink.
    let usb = 2.0 * frame.len() as f64 / up_rate;
    if cmd != Cmd::Relay as u8 {
        return latency + usb + work(frame, None, erase);
    }
    let Ok(r) = Relay::parse(payload) else {
        return latency + usb;
    };
    let Some(rate) = links
        .iter()
        .find(|l| l.id == r.link)
        .and_then(|l| link_rate(l.kind))
    else {
        return latency + usb;
    };
    // The request out and an answer of about the same size back.
    latency + usb + 2.0 * r.inner.len() as f64 / rate + work(r.inner, Some(&r), erase)
}

/// A [`SimRelay`] whose answers arrive when the links would have carried them.
pub struct TimedRelay {
    relay: SimRelay,
    medium: Medium,
    /// Answers and the simulated time they arrive at, in order.
    out: VecDeque<(u64, Vec<u8>)>,
    /// Transfers with a header whose last XFER_HDR chunk (the request the
    /// node erases on) is still to come: the node (`None` the relaying
    /// one, else its RELAY link and address) and its XFER_BEGIN.
    heading: Vec<(Option<(u8, u8)>, XferBegin)>,
}

/// The erase request `cmd` with `payload` sets off on the node `via`
/// reaches (`None`: the relaying node): an XFER_BEGIN without a header, or
/// the last header chunk of one with ([`SimNode::erase_ms`] of it).
fn erase_on(
    relay: &SimRelay,
    heading: &mut Vec<(Option<(u8, u8)>, XferBegin)>,
    via: Option<&Relay<'_>>,
    cmd: u8,
    payload: &[u8],
) -> u32 {
    let key = via.map(|r| (r.link, r.addr));
    let begin = if cmd == Cmd::XferBegin as u8 {
        heading.retain(|(k, _)| *k != key);
        match XferBegin::parse(payload) {
            Ok(b) if b.hdr_len > 0 => {
                heading.push((key, b));
                None
            }
            Ok(b) => Some(b),
            Err(_) => None,
        }
    } else if cmd == Cmd::XferHdr as u8 {
        let last = |b: &XferBegin| {
            XferChunk::parse(payload)
                .is_ok_and(|c| u64::from(c.offset) + c.data.len() as u64 >= u64::from(b.hdr_len))
        };
        heading
            .iter()
            .position(|(k, b)| *k == key && last(b))
            .map(|i| heading.remove(i).1)
    } else {
        None
    };
    let Some(b) = begin else { return 0 };
    match via {
        None => Some(&relay.node),
        Some(r) => relay.node_behind(r.link, r.addr),
    }
    .map_or(0, |n| n.erase_ms(&b))
}

impl TimedRelay {
    pub fn new(relay: SimRelay) -> TimedRelay {
        TimedRelay {
            relay,
            medium: Medium::UsbCdc,
            out: VecDeque::new(),
            heading: Vec::new(),
        }
    }

    pub fn relay_mut(&mut self) -> &mut SimRelay {
        &mut self.relay
    }

    pub fn medium(&self) -> Medium {
        self.medium
    }

    pub fn set_medium(&mut self, m: Medium) {
        self.medium = m;
    }

    pub fn now_ms(&self) -> u64 {
        self.relay.now_ms()
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.relay.advance_ms(ms);
    }

    /// Bytes from the PC to the relaying node; its answer is held until it
    /// arrives. A frame larger than the uplink carries never reaches it.
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.len().saturating_sub(dagon_proto::frame_len(0)) > self.medium.max_payload() {
            return;
        }
        self.relay.usb_write(bytes);
        // The node's own work may move its clock; the links' time comes on top.
        let links = self.relay.links();
        let (relay, heading) = (&self.relay, &mut self.heading);
        let mut eraser = None;
        let erase = |via: Option<&Relay<'_>>, cmd, payload: &[u8]| {
            let ms = erase_on(relay, heading, via, cmd, payload);
            if ms > 0 {
                eraser = Some(via.map(|r| (r.link, r.addr)));
            }
            ms
        };
        let due = self.relay.now_ms() + exchange_ms(bytes, self.medium, links, erase) as u64;
        // The node answers once its erase is done: its transfer is kept
        // from then, as a node's reply leaving keeps it.
        let node = match eraser {
            Some(None) => Some(&mut self.relay.node),
            Some(Some((link, addr))) => self.relay.node_behind_mut(link, addr),
            None => None,
        };
        if let Some(n) = node {
            n.reply_left(due);
        }
        let answer = self.relay.usb_read();
        if !answer.is_empty() {
            self.out.push_back((due, answer));
        }
    }

    /// When the next held answer arrives, if one is on its way.
    pub fn next_due(&self) -> Option<u64> {
        self.out.front().map(|(due, _)| *due)
    }

    /// The answers that have arrived by now; empty when none has.
    pub fn read(&mut self) -> Vec<u8> {
        let now = self.relay.now_ms();
        let mut got = Vec::new();
        while self.out.front().is_some_and(|(due, _)| *due <= now) {
            got.extend(self.out.pop_front().expect("checked").1);
        }
        got
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; dagon_proto::frame_len(payload.len())];
        dagon_proto::Frame {
            flags: 0,
            seq: 1,
            cmd,
            payload,
        }
        .encode(&mut out)
        .unwrap();
        out
    }

    fn relay(link: u8, inner: &[u8]) -> Vec<u8> {
        let r = Relay {
            link,
            addr: 0x42,
            timeout_ms: Some(300),
            inner,
        };
        let mut p = vec![0u8; Relay::OVERHEAD_MAX + inner.len()];
        let n = r.encode(&mut p).unwrap();
        p.truncate(n);
        frame(Cmd::Relay as u8, &p)
    }

    #[test]
    fn links_take_their_real_time() {
        let data = frame(Cmd::XferData as u8, &[0; 1000]);
        let mcu = crate::store::Layout::of("rp2040").mcu;
        let links = crate::store::relay_links(mcu);
        let none = |_: Option<&Relay<'_>>, _: u8, _: &[u8]| 0;
        let own = exchange_ms(&data, Medium::UsbCdc, &links, none);
        let i2c_link = crate::store::relay_link_id(mcu, "i2c0");
        let uart_link = crate::store::relay_link_id(mcu, "pio-uart");
        let i2c = exchange_ms(&relay(i2c_link, &data), Medium::UsbCdc, &links, none);
        let uart = exchange_ms(&relay(uart_link, &data), Medium::UsbCdc, &links, none);
        assert!(own < 5.0 && own < i2c && i2c < uart, "{own} {i2c} {uart}");
        // 1 KB over I2C and back: about 80 ms; the UART about 180 ms. Both
        // stay inside the 300 ms a node has to answer.
        assert!((70.0..100.0).contains(&i2c) && uart < 300.0, "{i2c} {uart}");
        // A request also takes the erase of the node the RELAY reaches.
        let begin = frame(Cmd::XferBegin as u8, &begin_payload(&APP_A));
        let erase = |r: Option<&Relay<'_>>, cmd: u8, p: &[u8]| {
            assert_eq!(r.map(|r| r.link), Some(i2c_link));
            assert_eq!(
                (cmd, XferBegin::parse(p)),
                (Cmd::XferBegin as u8, Ok(APP_A))
            );
            4_000
        };
        let t = exchange_ms(&relay(i2c_link, &begin), Medium::UsbCdc, &links, erase);
        assert!((4_000.0..4_100.0).contains(&t), "{t}");
    }

    /// 512 header bytes and a 20 KiB body into APP slot A.
    const APP_A: XferBegin = XferBegin {
        target: dagon_proto::payload::target::APP,
        slot: 0,
        total_len: 512 + 0x5000,
        hdr_len: 512,
    };

    fn begin_payload(b: &XferBegin) -> Vec<u8> {
        let mut p = vec![0u8; 16];
        let n = b.encode(&mut p).unwrap();
        p.truncate(n);
        p
    }

    /// Simulated ms from writing `f` to its answer.
    fn answer_ms(t: &mut TimedRelay, f: &[u8]) -> u64 {
        let start = t.now_ms();
        t.write(f);
        let due = t.next_due().expect("the node answers");
        t.advance_ms(due - start);
        t.read();
        due - start
    }

    fn chunk(cmd: Cmd, offset: u32, data: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; XferChunk::OVERHEAD + data.len()];
        let n = XferChunk { offset, data }.encode(&mut p).unwrap();
        p.truncate(n);
        frame(cmd as u8, &p)
    }

    #[test]
    fn a_transfer_waits_out_the_erase_its_node_plans_on_its_erase_request() {
        // The RP2040 hub's APP slot A starts at an erase unit: 0x5200 bytes
        // are 6 units of 4 KiB, each at its definition's 500 ms.
        let mut t = TimedRelay::new(crate::sample_hub(1));
        assert_eq!(t.relay_mut().node.erase_ms(&APP_A), 3_000);
        // With a header the node erases on the last header chunk, not on
        // XFER_BEGIN.
        let begin = frame(Cmd::XferBegin as u8, &begin_payload(&APP_A));
        assert!(answer_ms(&mut t, &begin) < 300);
        assert!(answer_ms(&mut t, &chunk(Cmd::XferHdr, 0, &[0; 256])) < 300);
        assert!(answer_ms(&mut t, &chunk(Cmd::XferHdr, 256, &[0; 256])) >= 3_000);
        // Without one, on XFER_BEGIN.
        let bare = XferBegin {
            total_len: 0x5000,
            hdr_len: 0,
            ..APP_A
        };
        let begin = frame(Cmd::XferBegin as u8, &begin_payload(&bare));
        assert!(answer_ms(&mut t, &begin) >= 2_500);
        // A transfer the definition plans no erase for takes none.
        let none = XferBegin { slot: 9, ..APP_A };
        assert_eq!(t.relay_mut().node.erase_ms(&none), 0);
    }

    #[test]
    fn each_uplink_has_its_own_size_speed_and_reboot() {
        let data = frame(Cmd::XferData as u8, &[0; 1000]);
        let t: Vec<f64> = Medium::ALL
            .iter()
            .map(|m| exchange_ms(&data, *m, &[], |_, _, _| 0))
            .collect();
        assert!(t[0] < t[2] && t[2] < t[1], "usb < wifi < uart: {t:?}");
        assert_eq!(Medium::Uart.max_payload(), max_payload::UART);
        assert!(Medium::Uart.stays_on_reboot());
        assert!(!Medium::WifiTcp.stays_on_reboot());
        for m in Medium::ALL {
            assert_eq!(Medium::of(m.kind()), Some(m));
        }
        assert_eq!(Medium::of(LinkKind::Ble), None);
    }

    #[test]
    fn an_answer_arrives_only_once_its_time_has_passed() {
        let mut t = TimedRelay::new(crate::sample_hub(1));
        t.write(&frame(Cmd::Hello as u8, &[1, 0, 0, 0, 0x40, 0x04]));
        let due = t.next_due().expect("the node answers HELLO");
        assert!(due > t.now_ms());
        assert!(t.read().is_empty(), "not before it arrives");
        t.advance_ms(due - t.now_ms());
        assert!(t.read().starts_with(&dagon_proto::SOF));
        assert_eq!(t.next_due(), None, "one answer only");
    }
}
