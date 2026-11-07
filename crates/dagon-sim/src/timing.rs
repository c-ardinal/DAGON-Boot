//! How long the links and the flash take, so a simulated write runs about
//! as long as it does on hardware: the uplink between the PC and the 親機
//! (USB CDC, a USB-UART adapter, Wi-Fi TCP), the I2C and UART links to the
//! 子機, and the sector erase and final check of a transfer.
//!
//! [`TimedHub`] holds each answer of a [`SimHub`] back until its simulated
//! arrival time. `dagon demo` (serve) and the browser demo both drive the
//! simulator through it; only their clocks differ (serve follows the wall
//! clock, the browser advances the simulator while it waits).

use std::collections::VecDeque;

use dagon_proto::payload::{link, Relay};
use dagon_proto::{max_payload, Cmd, HEADER_LEN};
use dagon_topology::schema::LinkKind;

use crate::SimHub;

/// The uplink simulated between the PC and the 親機.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// USB CDC: fast, gone while the hub reboots.
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

    /// Whether the link stays open while the 親機 reboots (a USB-UART
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
/// Fixed cost of one exchange (USB round trip, the hub's work).
const EXCHANGE_MS: f64 = 2.0;
/// Sector erase before a transfer, and the final check after it.
pub const ERASE_MS: f64 = 600.0;
const VERIFY_MS: f64 = 150.0;

/// Simulated ms the exchange of request `frame` takes on its links.
pub fn exchange_ms(frame: &[u8], medium: Medium) -> f64 {
    let cmd = frame.get(5).copied().unwrap_or(0);
    let payload = frame
        .get(HEADER_LEN..frame.len().saturating_sub(4))
        .unwrap_or(&[]);
    let (up_rate, latency) = medium.rate_and_latency();
    // The request up and its answer back, over the uplink.
    let usb = 2.0 * frame.len() as f64 / up_rate;
    let work = |inner_cmd: u8| match inner_cmd {
        c if c == Cmd::XferBegin as u8 => ERASE_MS,
        c if c == Cmd::XferEnd as u8 => VERIFY_MS,
        _ => 0.0,
    };
    if cmd != Cmd::Relay as u8 {
        return latency + usb + work(cmd);
    }
    let Ok(r) = Relay::parse(payload) else {
        return latency + usb;
    };
    let rate = if r.link == link::PIO_UART {
        UART_BYTES_PER_MS
    } else {
        I2C_BYTES_PER_MS
    };
    let inner_cmd = r.inner.get(5).copied().unwrap_or(0);
    // The request out and an answer of about the same size back.
    latency + usb + 2.0 * r.inner.len() as f64 / rate + work(inner_cmd)
}

/// A [`SimHub`] whose answers arrive when the links would have carried them.
pub struct TimedHub {
    hub: SimHub,
    medium: Medium,
    /// Answers and the simulated time they arrive at, in order.
    out: VecDeque<(u64, Vec<u8>)>,
}

impl TimedHub {
    pub fn new(hub: SimHub) -> TimedHub {
        TimedHub {
            hub,
            medium: Medium::UsbCdc,
            out: VecDeque::new(),
        }
    }

    pub fn hub_mut(&mut self) -> &mut SimHub {
        &mut self.hub
    }

    pub fn medium(&self) -> Medium {
        self.medium
    }

    pub fn set_medium(&mut self, m: Medium) {
        self.medium = m;
    }

    pub fn now_ms(&self) -> u64 {
        self.hub.now_ms()
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.hub.advance_ms(ms);
    }

    /// Bytes from the PC to the 親機; its answer is held until it arrives.
    /// A frame larger than the uplink carries never reaches the hub.
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.len().saturating_sub(dagon_proto::frame_len(0)) > self.medium.max_payload() {
            return;
        }
        self.hub.usb_write(bytes);
        // The hub's own work may move its clock; the links' time comes on top.
        let due = self.hub.now_ms() + exchange_ms(bytes, self.medium) as u64;
        let answer = self.hub.usb_read();
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
        let now = self.hub.now_ms();
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
            timeout_ms: 300,
            inner,
        };
        let mut p = vec![0u8; Relay::HEAD_LEN + inner.len()];
        r.encode(&mut p).unwrap();
        frame(Cmd::Relay as u8, &p)
    }

    #[test]
    fn links_take_their_real_time() {
        let data = frame(Cmd::XferData as u8, &[0; 1000]);
        let hub = exchange_ms(&data, Medium::UsbCdc);
        let i2c = exchange_ms(&relay(link::I2C0, &data), Medium::UsbCdc);
        let uart = exchange_ms(&relay(link::PIO_UART, &data), Medium::UsbCdc);
        assert!(hub < 5.0 && hub < i2c && i2c < uart, "{hub} {i2c} {uart}");
        // 1 KB over I2C and back: about 80 ms; the UART about 180 ms. Both
        // stay inside the 300 ms a node has to answer.
        assert!((70.0..100.0).contains(&i2c) && uart < 300.0, "{i2c} {uart}");
        let begin = frame(Cmd::XferBegin as u8, &[0; 8]);
        assert!(exchange_ms(&relay(link::I2C0, &begin), Medium::UsbCdc) >= ERASE_MS);
    }

    #[test]
    fn each_uplink_has_its_own_size_speed_and_reboot() {
        let data = frame(Cmd::XferData as u8, &[0; 1000]);
        let t: Vec<f64> = Medium::ALL.iter().map(|m| exchange_ms(&data, *m)).collect();
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
        let mut t = TimedHub::new(crate::sample_hub(1));
        t.write(&frame(Cmd::Hello as u8, &[1, 0, 0, 0, 0x40, 0x04]));
        let due = t.next_due().expect("the hub answers HELLO");
        assert!(due > t.now_ms());
        assert!(t.read().is_empty(), "not before it arrives");
        t.advance_ms(due - t.now_ms());
        assert!(t.read().starts_with(&dagon_proto::SOF));
        assert_eq!(t.next_due(), None, "one answer only");
    }
}
