//! The ESP32-S3 boot core (targets/mcu/esp32s3/bootcore, `qemu-layout`)
//! behind stage0 in the emulator, talked to over UART0 as a host talks to
//! a node: a STAY in its listen window keeps it in update mode, the app's
//! hand-over (an UPDATE_STAY in the app log) does so without a STAY, an
//! erased APP slot (or a lost app log) puts it in recovery mode, where the
//! host writes the app and the next start runs it; with nothing to serve
//! the core asks stage0 for the app. Its ELF is `DAGON_ESP32S3_BOOTCORE_ELF` (the CI job
//! `stage0-qemu` builds it with the test key and the tag [`TAG`]).

use super::*;
use dagon_proto::payload::{
    encode_reboot, mode, reboot_mode, target as xfer_target, HelloReq, HelloResp, SetPending,
    XferBegin, XferChunk,
};
use dagon_proto::{find_sof, flags, max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// How long one request may wait for its answer (an erase included).
const ANSWER: Duration = Duration::from_secs(20);
/// How long a request waits for its answer before it is sent again.
const RESEND: Duration = Duration::from_secs(2);
/// A body chunk of XFER_DATA.
const CHUNK: usize = 512;

/// A booted node: its UART0 as a byte stream both ways.
struct Running {
    child: std::process::Child,
    to: std::process::ChildStdin,
    from: Arc<Mutex<Vec<u8>>>,
    /// How far the host has read `from`.
    seen: usize,
    seq: u8,
    dir: std::path::PathBuf,
    rel: crate::genfs::Rel,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = crate::genfs::remove(&self.dir, &self.rel);
    }
}

impl Running {
    fn boot(node: &Node, case: &str) -> Running {
        let dir = std::env::temp_dir().join(format!("dagon-bootcore-qemu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rel = crate::genfs::Rel::new(format!("{case}.bin")).unwrap();
        crate::genfs::write(&dir, &rel, &node.flash()).unwrap();
        let path = dir.join(rel.as_str());
        let qemu = std::env::var("DAGON_QEMU").expect("DAGON_QEMU is not set");
        // UART0 on stdio without the monitor's multiplexer: frames are raw
        // bytes both ways.
        let mut child = Command::new(qemu)
            .args(["-display", "none", "-monitor", "none", "-serial", "stdio"])
            .args(["-machine", "esp32s3", "-drive"])
            .arg(format!("file={},if=mtd,format=raw", path.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("qemu starts");
        let to = child.stdin.take().unwrap();
        let mut out = child.stdout.take().unwrap();
        let from = Arc::new(Mutex::new(Vec::new()));
        let sink = from.clone();
        std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while let Ok(n) = out.read(&mut b) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&b[..n]);
            }
        });
        Running {
            child,
            to,
            from,
            seen: 0,
            seq: 0,
            dir,
            rel,
        }
    }

    fn send(&mut self, cmd: Cmd, payload: &[u8]) -> u8 {
        self.seq = self.seq.wrapping_add(1);
        self.put(self.seq, cmd, payload);
        self.seq
    }

    /// The request `seq` (`cmd`) written to the node.
    fn put(&mut self, seq: u8, cmd: Cmd, payload: &[u8]) {
        let mut b = vec![0u8; payload.len() + 64];
        let n = Frame {
            flags: 0,
            seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut b)
        .unwrap();
        self.to.write_all(&b[..n]).unwrap();
        self.to.flush().unwrap();
    }

    /// The next response to `seq` (`cmd`) from what the node sent past
    /// what was read, within `wait`: its status and body.
    fn answer(&mut self, cmd: Cmd, seq: u8, wait: Duration) -> Option<(Status, Vec<u8>)> {
        let until = Instant::now() + wait;
        loop {
            {
                let buf = self.from.lock().unwrap();
                while let Some(at) = find_sof(&buf, self.seen) {
                    match Frame::decode(&buf[at..], max_payload::UART) {
                        Ok((f, used)) => {
                            self.seen = at + used;
                            if f.flags & flags::RESP != 0 && f.seq == seq && f.cmd == cmd as u8 {
                                let (st, body) = split_status(f.payload)?;
                                return Some((st, body.to_vec()));
                            }
                        }
                        // A frame not whole yet: read again later.
                        Err(dagon_proto::FrameError::Incomplete) => break,
                        Err(_) => self.seen = at + 1,
                    }
                }
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ok(&mut self, cmd: Cmd, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.ok_within(cmd, payload, ANSWER)
    }

    /// `cmd` answered OK within `wait`. Like a host, it sends the request
    /// again (the same seq) every [`RESEND`] without an answer: the node
    /// drops a frame its bytes reach with a gap (20 ms), which a loaded
    /// emulator makes, and answers a repeat from its last reply.
    fn ok_within(&mut self, cmd: Cmd, payload: &[u8], wait: Duration) -> Result<Vec<u8>, String> {
        let until = Instant::now() + wait;
        let seq = self.send(cmd, payload);
        loop {
            let slice = until.saturating_duration_since(Instant::now()).min(RESEND);
            match self.answer(cmd, seq, slice) {
                Some((Status::Ok, body)) => return Ok(body),
                None if Instant::now() < until => self.put(seq, cmd, payload),
                other => return Err(format!("{cmd:?}: {other:?}")),
            }
        }
    }

    /// STAY every 10 ms from the start until one is answered.
    fn stay(&mut self, within: Duration) -> bool {
        let until = Instant::now() + within;
        while Instant::now() < until {
            let seq = self.send(Cmd::Stay, &[]);
            if let Some((Status::Ok, _)) = self.answer(Cmd::Stay, seq, Duration::from_millis(10)) {
                return true;
            }
        }
        false
    }

    /// HELLO, asked every half second until answered, within `within`.
    fn hello_until(&mut self, within: Duration) -> Result<HelloResp, String> {
        let until = Instant::now() + within;
        loop {
            match self.hello_within(Duration::from_millis(500)) {
                Ok(h) => return Ok(h),
                Err(e) if Instant::now() >= until => return Err(e),
                Err(_) => {}
            }
        }
    }

    fn hello(&mut self) -> Result<HelloResp, String> {
        self.hello_within(ANSWER)
    }

    fn hello_within(&mut self, wait: Duration) -> Result<HelloResp, String> {
        let mut p = [0u8; 16];
        let n = HelloReq {
            proto_ver: u32::from(PROTO_VERSION),
            max_payload: max_payload::UART as u16,
        }
        .encode(&mut p)
        .unwrap();
        let body = self.ok_within(Cmd::Hello, &p[..n], wait)?;
        HelloResp::parse(&body).map_err(|e| format!("HELLO: {e:?}"))
    }

    fn reboot(&mut self) -> Result<(), String> {
        let mut p = [0u8; 8];
        let n = encode_reboot(reboot_mode::NORMAL, &mut p).unwrap();
        self.ok(Cmd::Reboot, &p[..n]).map(|_| ())
    }

    /// The app written into APP slot 0 (its detached header, then its
    /// body) and set pending, as a host writes it.
    fn write_app(&mut self, hdr: &[u8; HEADER_SIZE], body: &[u8]) -> Result<(), String> {
        let mut p = vec![0u8; CHUNK + 64];
        let n = XferBegin {
            target: xfer_target::APP,
            slot: 0,
            total_len: (HEADER_SIZE + body.len()) as u32,
            hdr_len: HEADER_SIZE as u16,
        }
        .encode(&mut p)
        .unwrap();
        self.ok(Cmd::XferBegin, &p[..n])?;
        for (cmd, part) in [(Cmd::XferHdr, &hdr[..]), (Cmd::XferData, body)] {
            for (i, c) in part.chunks(CHUNK).enumerate() {
                let n = XferChunk {
                    offset: (i * CHUNK) as u32,
                    data: c,
                }
                .encode(&mut p)
                .unwrap();
                let chunk = p[..n].to_vec();
                self.ok(cmd, &chunk)?;
            }
        }
        self.ok(Cmd::XferEnd, &[])?;
        let n = SetPending {
            target: xfer_target::APP,
            slot: 0,
        }
        .encode(&mut p)
        .unwrap();
        self.ok(Cmd::SetPending, &p[..n]).map(|_| ())
    }

    /// Whether `text` appears in what the node sent, within `wait`.
    fn told(&self, text: &str, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            let buf = self.from.lock().unwrap().clone();
            if buf.windows(text.len()).any(|w| w == text.as_bytes()) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn node() -> Node {
    Node::with_core("DAGON_ESP32S3_BOOTCORE_ELF")
}

/// Each case: `Ok` or what went wrong.
type Case = fn() -> Result<(), String>;

fn window_then_app() -> Result<(), String> {
    let mut r = Running::boot(&node(), "window");
    if !r.stay(Duration::from_secs(20)) {
        return Err("no STAY answered".into());
    }
    let h = r.hello()?;
    if h.mode != mode::UPDATE {
        return Err(format!("HELLO mode {} in update mode", h.mode));
    }
    r.reboot()?;
    // The next start's window passes without a STAY; the core asks.
    r.told(APP_RUNS, Duration::from_secs(20))
        .then_some(())
        .ok_or_else(|| "the app does not run after REBOOT".into())
}

fn hand_over() -> Result<(), String> {
    let mut n = node();
    n.app_stay = true;
    let mut r = Running::boot(&n, "hand-over");
    // No STAY: the hand-over alone keeps the core.
    let h = r.hello_until(Duration::from_secs(20))?;
    if h.mode != mode::UPDATE {
        return Err(format!("HELLO mode {} after the hand-over", h.mode));
    }
    r.reboot()?;
    // The UPDATE_STAY is consumed: the next start goes on to the app.
    r.told(APP_RUNS, Duration::from_secs(20))
        .then_some(())
        .ok_or_else(|| "the app does not run after REBOOT".into())
}

fn repair_from_recovery() -> Result<(), String> {
    let mut n = node();
    n.app_named = false;
    n.app_erased = true;
    repair(n, "recovery")
}

/// Neither face of the app log valid: recovery mode creates the log and
/// takes the app all the same.
fn repair_without_a_log() -> Result<(), String> {
    let mut n = node();
    n.app_erased = true;
    n.app_log_erased = true;
    repair(n, "no-log")
}

fn repair(n: Node, case: &str) -> Result<(), String> {
    let (hdr, body) = (n.app_hdr, n.app.clone());
    let mut r = Running::boot(&n, case);
    let h = r.hello_until(Duration::from_secs(20))?;
    if h.mode != mode::RECOVERY {
        return Err(format!("HELLO mode {} with no app", h.mode));
    }
    r.write_app(&hdr, &body)?;
    r.reboot()?;
    r.told(APP_RUNS, Duration::from_secs(30))
        .then_some(())
        .ok_or_else(|| "the app written in recovery mode does not run".into())
}

fn nothing_to_serve() -> Result<(), String> {
    let r = Running::boot(&node(), "nothing");
    r.told(APP_RUNS, Duration::from_secs(20))
        .then_some(())
        .ok_or_else(|| "the app does not run".into())
}

#[test]
#[ignore = "needs QEMU and the images the CI job stage0-qemu builds"]
fn esp32s3_bootcore_in_qemu() {
    let cases: [(&str, Case); 5] = [
        (
            "nothing to serve: the core asks for the app",
            nothing_to_serve,
        ),
        ("a STAY in the window, then REBOOT", window_then_app),
        ("the app's hand-over", hand_over),
        (
            "recovery mode: the host writes the app",
            repair_from_recovery,
        ),
        ("recovery mode with the app log lost", repair_without_a_log),
    ];
    let mut failed = Vec::new();
    for (name, run) in cases {
        let r = run();
        eprintln!("{name}: {}", if r.is_ok() { "ok" } else { "FAILED" });
        if let Err(e) = r {
            failed.push(format!("{name}: {e}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
