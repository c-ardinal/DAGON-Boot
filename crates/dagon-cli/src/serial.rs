//! Serial port transport and wall clock for dagon-host.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use dagon_host::{Clock, Host, Transport};
use serialport::{SerialPort, SerialPortType};

/// USB IDs and product string of the hub's CDC interface.
pub const HUB_VID: u16 = 0xC0DE;
pub const HUB_PID: u16 = 0xCAFE;
pub const HUB_PRODUCT: &str = "DAGON hub";
/// Ignored by USB CDC; set for adapters that care.
const BAUD: u32 = 115_200;
/// How long one read waits for bytes.
const READ_WAIT: Duration = Duration::from_millis(10);

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

    /// Drops the port after an I/O error so the next reopen starts clean.
    fn check<T>(&mut self, r: io::Result<T>) -> io::Result<T> {
        if r.is_err() {
            self.port = None;
        }
        r
    }
}

impl Transport for Serial {
    type Error = io::Error;

    async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let r = self.port().and_then(|p| p.write_all(bytes));
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

impl Clock for Wall {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }

    async fn sleep_ms(&mut self, ms: u32) {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

pub type SerialHost = Host<Serial, Wall>;

pub fn host(port: Option<&str>) -> Result<SerialHost, String> {
    Ok(Host::new(Serial::open(port)?, Wall(Instant::now())))
}
