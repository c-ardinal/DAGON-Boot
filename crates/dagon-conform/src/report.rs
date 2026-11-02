//! Results and the conformance result file.
//!
//! # Result file (JSON), schema `dagon-conform/1`
//!
//! One object per run, written whole (the porting loop rewrites it after
//! every iteration; readers can always parse the latest file):
//!
//! ```json
//! {
//!   "schema": "dagon-conform/1",
//!   "node": "watch",
//!   "mcu_id": 3,
//!   "port_kind": 2,
//!   "link": "uart",
//!   "iteration": 4,
//!   "elapsed_s": 1830,
//!   "passed": 11,
//!   "total": 12,
//!   "tests": [
//!     {"id": "CT-01", "test_id": 1, "name": "flash geometry", "side": "node",
//!      "verdict": "pass", "measured": 3, "unit": "regions", "duration_ms": 41,
//!      "detail": ""}
//!   ]
//! }
//! ```
//!
//! | Field | Type | Meaning |
//! |---|---|---|
//! | `schema` | string | always [`SCHEMA`] |
//! | `node` | string | topology node id under test |
//! | `mcu_id`, `port_kind` | number or null | from the node's HELLO; null if it never answered |
//! | `link` | string | `"i2c"`, `"uart"` or `"usb"` |
//! | `iteration`, `elapsed_s` | number or null | porting loop counters; null outside a porting run |
//! | `passed` | number | tests with verdict `"pass"` |
//! | `total` | number | always 12 |
//! | `tests` | array | exactly 12 entries, in [`CTS`] order |
//! | `tests[].id` | string | `"CT-nn"` |
//! | `tests[].test_id` | number | `nn` |
//! | `tests[].name`, `side`, `unit` | string | from [`CTS`]; `side` is `"node"` or `"pc"` |
//! | `tests[].verdict` | string | `"pass"`, `"fail"` or `"not_run"` |
//! | `tests[].measured` | number | value in `unit` (0 when not passed) |
//! | `tests[].duration_ms` | number | wall time of the test on the PC |
//! | `tests[].detail` | string | failure reason (ASCII, at most 64 bytes); empty on pass unless the node reported something |

use crate::{Detail, Label, CTS};
use core::fmt::{self, Write};
use dagon_port::LinkKind;
use dagon_proto::payload::HelloResp;

pub const SCHEMA: &str = "dagon-conform/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    NotRun,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::NotRun => "not_run",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtResult {
    /// `nn` of `CT-nn`.
    pub id: u8,
    pub verdict: Verdict,
    pub measured: u32,
    pub duration_ms: u32,
    pub detail: Detail,
}

impl CtResult {
    pub const fn not_run(id: u8) -> Self {
        CtResult {
            id,
            verdict: Verdict::NotRun,
            measured: 0,
            duration_ms: 0,
            detail: Detail::new(),
        }
    }
}

/// One run: results in [`CTS`] order.
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub hello: Option<HelloResp>,
    pub link: LinkKind,
    pub results: [CtResult; 12],
}

/// Porting loop counters for the result file.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub iteration: Option<u32>,
    pub elapsed_s: Option<u32>,
}

impl Report {
    /// A report with every test not run (node unreachable).
    pub fn unreachable(link: LinkKind) -> Self {
        Report {
            hello: None,
            link,
            results: CTS.map(|c| CtResult::not_run(c.id)),
        }
    }

    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.verdict == Verdict::Pass)
            .count()
    }

    /// Writes the result file (see the module docs).
    pub fn write_json<W: Write>(&self, w: &mut W, node: &str, progress: Progress) -> fmt::Result {
        let link = match self.link {
            LinkKind::I2cTarget => "i2c",
            LinkKind::Uart => "uart",
            LinkKind::UsbCdc => "usb",
        };
        w.write_str("{\n  \"schema\": ")?;
        string(w, SCHEMA.as_bytes())?;
        w.write_str(",\n  \"node\": ")?;
        string(w, node.as_bytes())?;
        write!(w, ",\n  \"mcu_id\": ")?;
        opt(w, self.hello.map(|h| h.mcu_id as u32))?;
        write!(w, ",\n  \"port_kind\": ")?;
        opt(w, self.hello.map(|h| h.port_kind as u32))?;
        write!(w, ",\n  \"link\": \"{}\",\n  \"iteration\": ", link)?;
        opt(w, progress.iteration)?;
        w.write_str(",\n  \"elapsed_s\": ")?;
        opt(w, progress.elapsed_s)?;
        write!(
            w,
            ",\n  \"passed\": {},\n  \"total\": {},\n  \"tests\": [",
            self.passed(),
            CTS.len()
        )?;
        for (i, c) in CTS.iter().enumerate() {
            let r = self
                .results
                .iter()
                .find(|r| r.id == c.id)
                .copied()
                .unwrap_or(CtResult::not_run(c.id));
            w.write_str(if i == 0 { "\n    " } else { ",\n    " })?;
            write!(
                w,
                "{{\"id\": \"{}\", \"test_id\": {}, \"name\": \"{}\", \"side\": \"{}\", \
                 \"verdict\": \"{}\", \"measured\": {}, \"unit\": \"{}\", \"duration_ms\": {}, \
                 \"detail\": ",
                Label(c.id),
                c.id,
                c.name,
                c.side.as_str(),
                r.verdict.as_str(),
                r.measured,
                c.unit,
                r.duration_ms
            )?;
            string(w, r.detail.as_bytes())?;
            w.write_char('}')?;
        }
        w.write_str("\n  ]\n}\n")
    }
}

fn opt<W: Write>(w: &mut W, v: Option<u32>) -> fmt::Result {
    match v {
        Some(v) => write!(w, "{}", v),
        None => w.write_str("null"),
    }
}

/// JSON string from untrusted bytes: printable ASCII as is (quote and
/// backslash escaped), everything else as `\u00XX`.
fn string<W: Write>(w: &mut W, b: &[u8]) -> fmt::Result {
    w.write_char('"')?;
    for &c in b {
        match c {
            b'"' => w.write_str("\\\"")?,
            b'\\' => w.write_str("\\\\")?,
            0x20..=0x7E => w.write_char(c as char)?,
            _ => write!(w, "\\u{:04x}", c)?,
        }
    }
    w.write_char('"')
}
