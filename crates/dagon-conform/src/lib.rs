//! DAGON conformance tests: the 12 normal-path checks a port must pass.
//!
//! Everything is synchronous: a test runs to completion before it answers.
//!
//! - [`node`]: the CONF_RUN handler a conformance build of the node app
//!   serves (CT-01, CT-02, CT-03, CT-07, CT-08 run inside the node).
//! - [`pc`]: the sequence the PC drives through the hub (CT-04, CT-09,
//!   CT-10, CT-11, CT-12, CT-18, CT-20), plus the CONF_RUN calls for the
//!   node-side tests.
//! - [`timeout`]: per-stage time limits.
//! - [`report`]: results and the result file (JSON) the CLI and the browser
//!   read.
//!
//! The test list, the procedures and the limits are fixed here; a port only
//! supplies the board traits and the images.
#![no_std]
#![forbid(unsafe_code)]

pub mod node;
pub mod pc;
pub mod report;
pub mod timeout;

use core::fmt;

/// Where a test runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Inside the node, started by CONF_RUN.
    Node,
    /// Driven by the PC through the hub with ordinary commands.
    Pc,
}

impl Side {
    pub const fn as_str(self) -> &'static str {
        match self {
            Side::Node => "node",
            Side::Pc => "pc",
        }
    }
}

/// One entry of [`CTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtInfo {
    /// `nn` of `CT-nn`; also the CONF_RUN `test_id` for node-side tests.
    pub id: u8,
    pub name: &'static str,
    pub side: Side,
    /// Unit of the `measured` value.
    pub unit: &'static str,
}

impl CtInfo {
    /// `CT-nn`.
    pub fn label(&self) -> Label {
        Label(self.id)
    }
}

/// Displays as `CT-nn`.
#[derive(Clone, Copy, Debug)]
pub struct Label(pub u8);

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CT-{:02}", self.0)
    }
}

const fn ct(id: u8, name: &'static str, side: Side, unit: &'static str) -> CtInfo {
    CtInfo {
        id,
        name,
        side,
        unit,
    }
}

/// The 12 tests, in run and report order.
pub const CTS: [CtInfo; 12] = [
    ct(1, "flash geometry", Side::Node, "regions"),
    ct(2, "erase", Side::Node, "bytes"),
    ct(3, "aligned write", Side::Node, "bytes"),
    ct(7, "ed25519", Side::Node, "rejected"),
    ct(8, "sha-256", Side::Node, "vectors"),
    ct(4, "active slot protected", Side::Pc, "rejected"),
    ct(20, "compatibility reject", Side::Pc, "rejected"),
    ct(9, "link through hub", Side::Pc, "bytes/s"),
    ct(10, "reboot", Side::Pc, "ms"),
    ct(11, "trial boot and confirm", Side::Pc, "ms"),
    ct(12, "rollback", Side::Pc, "ms"),
    ct(18, "hello and status", Side::Pc, "fields"),
];

pub fn ct_info(id: u8) -> Option<&'static CtInfo> {
    CTS.iter().find(|c| c.id == id)
}

/// Short ASCII text (at most 64 bytes, the CONF_RUN `detail` limit); longer
/// writes are cut.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Detail {
    buf: [u8; dagon_proto::payload::CONF_DETAIL_MAX],
    len: u8,
}

impl Detail {
    pub const fn new() -> Self {
        Detail {
            buf: [0; dagon_proto::payload::CONF_DETAIL_MAX],
            len: 0,
        }
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        let mut d = Detail::new();
        d.push(b);
        d
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    fn push(&mut self, b: &[u8]) {
        let at = self.len as usize;
        let n = b.len().min(self.buf.len() - at);
        self.buf[at..at + n].copy_from_slice(&b[..n]);
        self.len += n as u8;
    }
}

impl Default for Detail {
    fn default() -> Self {
        Detail::new()
    }
}

impl fmt::Write for Detail {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push(s.as_bytes());
        Ok(())
    }
}

impl fmt::Debug for Detail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", core::str::from_utf8(self.as_bytes()))
    }
}

/// `Detail` from format arguments.
#[macro_export]
macro_rules! detail {
    ($($arg:tt)*) => {{
        let mut d = $crate::Detail::new();
        let _ = core::fmt::Write::write_fmt(&mut d, format_args!($($arg)*));
        d
    }};
}
