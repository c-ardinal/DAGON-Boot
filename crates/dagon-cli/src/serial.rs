//! Serial port transport and wall clock for dagon-host.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use dagon_host::{Host, HostClock, Transport};
use serialport::{SerialPort, SerialPortType};

/// USB IDs and product string of the hub's CDC interface.
pub const HUB_VID: u16 = dagon_proto::usb_id::VID;
pub const HUB_PID: u16 = dagon_proto::usb_id::PID;
pub const HUB_PRODUCT: &str = "DAGON hub";
/// Ignored by USB CDC; set for adapters that care.
const BAUD: u32 = 115_200;
/// How long one read waits for bytes (also the port's bound on one write
/// call, which [`write_within`] repeats).
const READ_WAIT: Duration = Duration::from_millis(10);
/// How long a request's bytes may wait for the hub to take them: a hub
/// takes the next request once it answered the last, and answers within
/// what the host waits (`dagon_port::timeout::RELAY_ANSWER_MARGIN_MS`), so
/// one plain request's time is the most it stays busy.
const WRITE_WAIT: Duration = Duration::from_millis(dagon_port::timeout::REQUEST_MS as u64);

/// Name of the first port that looks like the hub.
pub fn find_hub() -> Result<String, String> {
    let ports = serialport::available_ports().map_err(|e| format!("cannot list ports: {e}"))?;
    ports
        .into_iter()
        .find(|p| match &p.port_type {
            SerialPortType::UsbPort(u) => {
                (u.vid, u.pid) == (HUB_VID, HUB_PID) || u.product.as_deref() == Some(HUB_PRODUCT)
            }
            _ => false,
        })
        .map(|p| p.port_name)
        .ok_or_else(|| "no DAGON hub found; plug it in or pass --port <name>".into())
}

pub struct Serial {
    /// Port given on the command line; otherwise searched again on reopen.
    fixed: Option<String>,
    port: Option<Box<dyn SerialPort>>,
}

impl Serial {
    pub fn open(fixed: Option<&str>) -> Result<Serial, String> {
        let mut s = Serial {
            fixed: fixed.map(String::from),
            port: None,
        };
        s.connect().map_err(|e| e.to_string())?;
        Ok(s)
    }

    fn connect(&mut self) -> io::Result<()> {
        self.port = None;
        let name = match &self.fixed {
            Some(n) => n.clone(),
            None => find_hub().map_err(io::Error::other)?,
        };
        let port = serialport::new(&name, BAUD)
            .timeout(READ_WAIT)
            .dtr_on_open(true)
            .open()
            .map_err(|e| io::Error::other(format!("cannot open {name}: {e}")))?;
        self.port = Some(port);
        Ok(())
    }

    fn port(&mut self) -> io::Result<&mut Box<dyn SerialPort>> {
        self.port
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))
    }

    /// Drops the port after an I/O error so the next reopen starts clean;
    /// a timeout leaves it (the hub is busy, not gone: the next request,
    /// to it or to another node, goes over the same port).
    fn check<T>(&mut self, r: io::Result<T>) -> io::Result<T> {
        if r.as_ref()
            .is_err_and(|e| e.kind() != io::ErrorKind::TimedOut)
        {
            self.port = None;
        }
        r
    }
}

/// Writes all of `bytes` to `w`, whose own write gives up after
/// [`READ_WAIT`] (`TimedOut`, e.g. Windows' ERROR_SEM_TIMEOUT while the
/// hub takes no bytes), trying again until `wait` has passed.
fn write_within(w: &mut dyn Write, mut bytes: &[u8], wait: Duration) -> io::Result<()> {
    let start = Instant::now();
    while !bytes.is_empty() {
        match w.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::TimedOut && start.elapsed() < wait => {}
            Err(e) => return Err(e),
        }
    }
    w.flush()
}

impl Transport for Serial {
    type Error = io::Error;
    /// The hub's USB CDC port (a UART adapter would carry less and stay
    /// across a reboot).
    fn max_payload(&self) -> usize {
        dagon_proto::max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }

    async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let r = self
            .port()
            .and_then(|p| write_within(p.as_mut(), bytes, WRITE_WAIT));
        self.check(r)
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let r = match self.port()?.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            r => r,
        };
        self.check(r)
    }

    async fn reopen(&mut self) -> io::Result<()> {
        self.connect()
    }
}

pub struct Wall(Instant);

impl HostClock for Wall {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }

    async fn sleep_ms(&mut self, ms: u32) {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

pub type SerialHost = Host<Serial, Wall>;

pub fn host(port: Option<&str>) -> Result<SerialHost, String> {
    let mut h = Host::new(Serial::open(port)?, Wall(Instant::now()));
    h.note = Some(Box::new(|line| println!("{line}")));
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port whose writes time out `busy` times, then take `step` bytes
    /// each.
    struct Busy {
        busy: usize,
        step: usize,
        got: Vec<u8>,
    }

    impl Write for Busy {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if self.busy > 0 {
                self.busy -= 1;
                return Err(io::ErrorKind::TimedOut.into());
            }
            let n = b.len().min(self.step);
            self.got.extend_from_slice(&b[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A hub busy for a few of the port's write bounds still gets the whole
    /// request; one that takes nothing for the whole wait ends in a timeout.
    #[test]
    fn a_write_waits_out_a_busy_hub_within_its_bound() {
        // What the port gives on Windows while the hub takes no bytes.
        #[cfg(windows)]
        assert_eq!(
            io::Error::from_raw_os_error(121).kind(),
            io::ErrorKind::TimedOut
        );
        let mut w = Busy {
            busy: 3,
            step: 5,
            got: Vec::new(),
        };
        write_within(&mut w, b"0123456789ab", Duration::from_secs(5)).unwrap();
        assert_eq!(w.got, b"0123456789ab");
        let mut gone = Busy {
            busy: usize::MAX,
            step: 1,
            got: Vec::new(),
        };
        let e = write_within(&mut gone, b"x", Duration::from_millis(20)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    }
}
