//! The simulated desk behind `dagon demo`: dagon-sim's hub with its two
//! nodes, reached by the page through serve instead of Web Serial
//! (POST /v1/sim/write, GET /v1/sim/read). The page runs dagon-host as it
//! does with a real hub; only the bytes go another way.
//!
//! Simulated time follows the wall clock times `speed`, and each answer is
//! held back for about as long as the real link and flash take (USB to the
//! hub, I2C to the F401, the PIO UART to the watch, sector erase and final
//! check), so progress looks as it does on hardware.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use dagon_proto::payload::{link, Relay};
use dagon_proto::{max_payload, Cmd, HEADER_LEN};
use dagon_sim::SimHub;
use dagon_topology::schema::LinkKind;

/// The uplink the demo simulates between the page and the 親機.
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

    /// The medium for an uplink kind; None for kinds the demo does not simulate.
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

    pub fn reconnect(self) -> dagon_host::Reconnect {
        match self {
            Medium::Uart => dagon_host::Reconnect::Stays,
            Medium::UsbCdc | Medium::WifiTcp => dagon_host::Reconnect::Reopen,
        }
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
const ERASE_MS: f64 = 600.0;
const VERIFY_MS: f64 = 150.0;

pub struct SimDevice {
    state: Mutex<State>,
    answered: Condvar,
    /// Simulated ms per wall ms.
    pub speed: f64,
    /// The node set to fail its update (for the page and the log).
    pub fail: Option<String>,
    /// Lines for serve's log, under its banner (what is simulated, the
    /// demo key).
    pub notes: Vec<String>,
    /// The uplink simulated now (the demo 構成's, changed with it).
    medium: Mutex<Medium>,
}

struct State {
    hub: SimHub,
    wall0: Instant,
    sim0: u64,
    /// Answers and the simulated time they arrive at, in order.
    out: VecDeque<(u64, Vec<u8>)>,
}

impl SimDevice {
    pub fn new(hub: SimHub, speed: f64, fail: Option<String>) -> SimDevice {
        let sim0 = hub.now_ms();
        SimDevice {
            state: Mutex::new(State {
                hub,
                wall0: Instant::now(),
                sim0,
                out: VecDeque::new(),
            }),
            answered: Condvar::new(),
            speed,
            fail,
            notes: Vec::new(),
            medium: Mutex::new(Medium::UsbCdc),
        }
    }

    pub fn medium(&self) -> Medium {
        *self.medium.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Switches the simulated uplink (the demo 構成 names another).
    pub fn set_medium(&self, m: Medium) {
        *self.medium.lock().unwrap_or_else(|p| p.into_inner()) = m;
    }

    /// The simulated desk catches up with the wall clock.
    fn sync(&self, st: &mut State) {
        let elapsed = st.wall0.elapsed().as_secs_f64() * 1_000.0 * self.speed;
        let target = st.sim0 + elapsed as u64;
        let now = st.hub.now_ms();
        if target > now {
            st.hub.advance_ms(target - now);
        }
    }

    /// Bytes from the page to the hub's USB port.
    pub fn write(&self, bytes: &[u8]) {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.sync(&mut st);
        let medium = self.medium();
        // A frame larger than the uplink carries never reaches the hub.
        if bytes.len().saturating_sub(dagon_proto::frame_len(0)) > medium.max_payload() {
            return;
        }
        let due = st.hub.now_ms() + exchange_ms(bytes, medium) as u64;
        st.hub.usb_write(bytes);
        let answer = st.hub.usb_read();
        if !answer.is_empty() {
            st.out.push_back((due, answer));
        }
        self.answered.notify_all();
    }

    /// The answers that have arrived, waiting up to `wait` for one; empty
    /// when none came.
    pub fn read(&self, wait: Duration) -> Vec<u8> {
        let until = Instant::now() + wait;
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            self.sync(&mut st);
            let now = st.hub.now_ms();
            let mut got = Vec::new();
            while st.out.front().is_some_and(|(due, _)| *due <= now) {
                got.extend(st.out.pop_front().expect("checked").1);
            }
            let left = until.saturating_duration_since(Instant::now());
            if !got.is_empty() || left.is_zero() {
                return got;
            }
            let next = st
                .out
                .front()
                .map(|(due, _)| Duration::from_secs_f64((due - now) as f64 / self.speed / 1_000.0));
            let pause = next
                .map_or(left, |n| n.min(left))
                .max(Duration::from_millis(1));
            st = self
                .answered
                .wait_timeout(st, pause)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

/// Simulated ms the exchange of request `frame` takes on its links.
fn exchange_ms(frame: &[u8], medium: Medium) -> f64 {
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
        assert_eq!(Medium::Uart.reconnect(), dagon_host::Reconnect::Stays);
        assert_eq!(Medium::WifiTcp.reconnect(), dagon_host::Reconnect::Reopen);
        for m in Medium::ALL {
            assert_eq!(Medium::of(m.kind()), Some(m));
        }
        assert_eq!(Medium::of(LinkKind::Ble), None);
    }

    #[test]
    fn answers_come_when_their_time_has_passed() {
        let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]));
        let dev = SimDevice::new(crate::demo::desk(&key, None, 1).unwrap(), 50.0, None);
        let hello = frame(Cmd::Hello as u8, &[1, 0, 0, 0, 0x40, 0x04]);
        dev.write(&hello);
        let got = dev.read(Duration::from_millis(500));
        assert!(got.starts_with(&dagon_proto::SOF), "{got:?}");
        assert!(
            dev.read(Duration::from_millis(5)).is_empty(),
            "one answer only"
        );
    }
}
