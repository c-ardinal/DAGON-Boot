//! Shared state of `dagon serve`: the event stream to the pages, jobs, and
//! the hand-over of the hub's serial port between the page and the CLI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Logs go out at most this often (10 per second).
const LOG_EVERY: Duration = Duration::from_millis(100);
const WAIT_STEP: Duration = Duration::from_millis(100);
/// Jobs running at once; more are refused until one ends.
pub const MAX_RUNNING: usize = 4;
/// Jobs kept for GET /v1/jobs/{id}; the oldest finished ones go first.
pub const MAX_KEPT: usize = 64;
/// Events queued for one page; a page that falls this far behind is cut off
/// (it reconnects and reads state again).
pub const EVENT_QUEUE: usize = 256;
/// Log lines one job may send, and the length of one line.
pub const MAX_LOG_LINES: usize = 5_000;
pub const MAX_LOG_LINE: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Device {
    /// The page may hold the port.
    Page,
    /// The page said it closed the port.
    Released,
}

struct JobEntry {
    kind: String,
    state: &'static str,
    cancel: Arc<AtomicBool>,
    answer: Option<mpsc::Sender<String>>,
    result: Option<Value>,
}

pub struct Server {
    pub pass: String,
    pub port: u16,
    /// dagon.toml of the open project (None: no file, defaults only);
    /// changed by opening or creating a project.
    config: std::sync::RwLock<Option<PathBuf>>,
    /// Folder of the page; empty: the page built into the executable.
    pub web: PathBuf,
    /// The folder projects may be opened or created under; unset: the
    /// user's home.
    pub home: std::sync::OnceLock<PathBuf>,
    /// Public key the page verifies releases with, given at start.
    pub release_key: std::sync::OnceLock<[u8; 32]>,
    /// Pages connected to the event stream (passphrase checked).
    pub pages: AtomicUsize,
    events: Mutex<Vec<mpsc::SyncSender<String>>>,
    jobs: Mutex<BTreeMap<u64, JobEntry>>,
    /// Serialises writes to dagon.toml and the topology file.
    pub files: Mutex<()>,
    next: AtomicU64,
    device: Mutex<Device>,
    device_cv: Condvar,
    /// One job uses the hub at a time.
    hub: Mutex<()>,
}

impl Server {
    pub fn new(pass: String, port: u16, config: Option<PathBuf>, web: PathBuf) -> Arc<Self> {
        Arc::new(Server {
            pass,
            port,
            config: std::sync::RwLock::new(config),
            web,
            home: std::sync::OnceLock::new(),
            release_key: std::sync::OnceLock::new(),
            pages: AtomicUsize::new(0),
            events: Mutex::default(),
            jobs: Mutex::default(),
            files: Mutex::new(()),
            next: AtomicU64::new(1),
            device: Mutex::new(Device::Page),
            device_cv: Condvar::new(),
            hub: Mutex::new(()),
        })
    }

    /// dagon.toml of the open project.
    pub fn config_path(&self) -> Option<PathBuf> {
        self.config.read().ok().and_then(|c| c.clone())
    }

    /// Switches to the project whose dagon.toml is `path`.
    pub fn set_config(&self, path: PathBuf) {
        if let Ok(mut c) = self.config.write() {
            *c = Some(path);
        }
    }

    /// Sends `event` to every connected page.
    pub fn emit(&self, event: Value) {
        let text = event.to_string();
        let mut subs = self.events.lock().expect("events");
        subs.retain(|s| s.try_send(text.clone()).is_ok());
    }

    pub fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::sync_channel(EVENT_QUEUE);
        self.events.lock().expect("events").push(tx);
        rx
    }

    fn pages(&self) -> usize {
        self.pages.load(Ordering::Relaxed)
    }

    /// The page closed Web Serial.
    pub fn device_released(&self) {
        *self.device.lock().expect("device") = Device::Released;
        self.device_cv.notify_all();
    }

    fn set_state(&self, id: u64, state: &'static str) {
        let kind = {
            let mut jobs = self.jobs.lock().expect("jobs");
            let Some(j) = jobs.get_mut(&id) else { return };
            j.state = state;
            j.kind.clone()
        };
        self.emit(json!({"type": "job.state", "job": id, "kind": kind, "state": state}));
    }

    /// Starts `work` on its own thread; returns the job id, or None when
    /// [`MAX_RUNNING`] jobs are already running.
    pub fn spawn<F>(self: &Arc<Self>, kind: &str, work: F) -> Option<u64>
    where
        F: FnOnce(&mut Job) -> Result<Value, String> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut jobs = self.jobs.lock().expect("jobs");
        let finished = |j: &JobEntry| matches!(j.state, "succeeded" | "failed" | "cancelled");
        if jobs.values().filter(|j| !finished(j)).count() >= MAX_RUNNING {
            return None;
        }
        while jobs.len() >= MAX_KEPT {
            let Some(old) = jobs.iter().find(|(_, j)| finished(j)).map(|(k, _)| *k) else {
                break;
            };
            jobs.remove(&old);
        }
        jobs.insert(
            id,
            JobEntry {
                kind: kind.into(),
                state: "queued",
                cancel: cancel.clone(),
                answer: None,
                result: None,
            },
        );
        drop(jobs);
        self.set_state(id, "queued");
        let server = self.clone();
        std::thread::spawn(move || {
            server.set_state(id, "running");
            let mut job = Job {
                id,
                server: server.clone(),
                cancel,
                lines: Vec::new(),
                flushed: Instant::now(),
                sent: 0,
            };
            let r = work(&mut job);
            job.flush();
            let (state, data) = match r {
                Ok(v) => ("succeeded", v),
                Err(_) if job.cancelled() => ("cancelled", Value::Null),
                Err(e) => ("failed", json!({ "error": e })),
            };
            if let Some(j) = server.jobs.lock().expect("jobs").get_mut(&id) {
                j.result = Some(data.clone());
                j.answer = None;
            }
            server.emit(json!({"type": "job.result", "job": id, "data": data}));
            server.set_state(id, state);
        });
        Some(id)
    }

    /// Asks job `id` to stop. `false`: no such job.
    pub fn cancel(&self, id: u64) -> bool {
        let jobs = self.jobs.lock().expect("jobs");
        let Some(j) = jobs.get(&id) else { return false };
        j.cancel.store(true, Ordering::Relaxed);
        drop(jobs);
        self.device_cv.notify_all();
        true
    }

    /// Hands `text` to job `id`'s open question. `false`: none open.
    pub fn answer(&self, id: u64, text: String) -> bool {
        let jobs = self.jobs.lock().expect("jobs");
        jobs.get(&id)
            .and_then(|j| j.answer.as_ref())
            .is_some_and(|tx| tx.send(text).is_ok())
    }

    pub fn job_json(&self, id: u64) -> Option<Value> {
        let jobs = self.jobs.lock().expect("jobs");
        jobs.get(&id)
            .map(|j| json!({"job": id, "kind": j.kind, "state": j.state, "result": j.result}))
    }
}

/// A running job's handle on the server.
pub struct Job {
    pub id: u64,
    server: Arc<Server>,
    cancel: Arc<AtomicBool>,
    lines: Vec<String>,
    flushed: Instant,
    /// Log lines taken so far (for [`MAX_LOG_LINES`]).
    sent: usize,
}

impl Job {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn progress(&self, stage: &str, percent: Option<u8>) {
        self.server.emit(
            json!({"type": "job.progress", "job": self.id, "stage": stage, "percent": percent}),
        );
    }

    /// Adds a log line; lines go out in batches.
    pub fn log(&mut self, line: &str) {
        self.sent += 1;
        if self.sent > MAX_LOG_LINES {
            if self.sent == MAX_LOG_LINES + 1 {
                self.lines.push("(log truncated)".into());
            }
            return;
        }
        let mut cut = line.len().min(MAX_LOG_LINE);
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        self.lines.push(line[..cut].to_string());
        if self.flushed.elapsed() >= LOG_EVERY {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if !self.lines.is_empty() {
            let lines = std::mem::take(&mut self.lines);
            self.server
                .emit(json!({"type": "job.log", "job": self.id, "lines": lines}));
        }
        self.flushed = Instant::now();
    }

    /// Asks the page a question and waits for the answer (or cancel).
    pub fn ask(&mut self, qid: usize, text: &str) -> Result<String, String> {
        self.flush();
        let (tx, rx) = mpsc::channel();
        if let Some(j) = self.server.jobs.lock().expect("jobs").get_mut(&self.id) {
            j.answer = Some(tx);
        }
        self.server.set_state(self.id, "question");
        self.server.emit(json!({
            "type": "job.question", "job": self.id, "id": qid, "text": text,
            "answer": {"kind": "text"},
        }));
        let answer = loop {
            if self.cancelled() {
                return Err("cancelled".into());
            }
            match rx.recv_timeout(WAIT_STEP) {
                Ok(a) => break a,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("no answer".into()),
            }
        };
        if let Some(j) = self.server.jobs.lock().expect("jobs").get_mut(&self.id) {
            j.answer = None;
        }
        self.server.set_state(self.id, "running");
        Ok(answer)
    }

    /// Runs `f` with the hub's serial port: when a page is connected it is
    /// asked to close the port first (device.need, then the page's
    /// released), and gets it back afterwards (device.returned).
    pub fn with_device<T>(
        &mut self,
        f: impl FnOnce(&mut Job) -> Result<T, String>,
    ) -> Result<T, String> {
        let server = self.server.clone();
        let _hub = server
            .hub
            .lock()
            .map_err(|_| "hub lock poisoned".to_string())?;
        if server.pages() > 0 {
            *server.device.lock().expect("device") = Device::Page;
            server.emit(json!({"type": "device.need", "job": self.id}));
            let mut d = server.device.lock().expect("device");
            while *d != Device::Released {
                if self.cancelled() {
                    return Err("cancelled".into());
                }
                d = server
                    .device_cv
                    .wait_timeout(d, WAIT_STEP)
                    .expect("device")
                    .0;
            }
        }
        let r = f(self);
        *server.device.lock().expect("device") = Device::Page;
        server.emit(json!({"type": "device.returned", "job": self.id}));
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Arc<Server> {
        Server::new("p".into(), 1, None, PathBuf::new())
    }

    #[test]
    fn running_jobs_are_capped() {
        let s = server();
        let gate = Arc::new(AtomicBool::new(false));
        let ids: Vec<Option<u64>> = (0..MAX_RUNNING)
            .map(|_| {
                let g = gate.clone();
                s.spawn("wait", move |_| {
                    while !g.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(Value::Null)
                })
            })
            .collect();
        assert!(ids.iter().all(Option::is_some));
        assert!(s.spawn("one more", |_| Ok(Value::Null)).is_none());
        gate.store(true, Ordering::Relaxed);
    }

    #[test]
    fn finished_jobs_are_evicted() {
        let s = server();
        let first = s.spawn("quick", |_| Ok(Value::Null)).unwrap();
        while s.job_json(first).unwrap()["state"] != "succeeded" {
            std::thread::sleep(Duration::from_millis(5));
        }
        for _ in 0..MAX_KEPT {
            let id = s.spawn("quick", |_| Ok(Value::Null)).unwrap();
            while s.job_json(id).unwrap()["state"] != "succeeded" {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(s.job_json(first).is_none());
        assert!(s.jobs.lock().unwrap().len() <= MAX_KEPT);
    }

    #[test]
    fn a_page_that_does_not_read_is_cut_off() {
        let s = server();
        let rx = s.subscribe();
        for i in 0..EVENT_QUEUE + 10 {
            s.emit(json!({ "n": i }));
        }
        assert_eq!(s.events.lock().unwrap().len(), 0, "dropped");
        assert_eq!(rx.try_iter().count(), EVENT_QUEUE);
    }

    #[test]
    fn job_logs_are_bounded() {
        let s = server();
        let rx = s.subscribe();
        let id = s
            .spawn("chatty", |job| {
                for _ in 0..MAX_LOG_LINES + 100 {
                    job.log(&"x".repeat(MAX_LOG_LINE + 50));
                }
                Ok(Value::Null)
            })
            .unwrap();
        let mut lines = Vec::new();
        loop {
            let ev: Value = serde_json::from_str(&rx.recv().unwrap()).unwrap();
            if ev["type"] == "job.log" {
                lines.extend(
                    ev["lines"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|l| l.as_str().unwrap().len()),
                );
            }
            if ev["type"] == "job.state" && ev["job"] == id && ev["state"] == "succeeded" {
                break;
            }
        }
        assert_eq!(
            lines.len(),
            MAX_LOG_LINES + 1,
            "lines plus the truncation note"
        );
        assert!(lines.iter().all(|&n| n <= MAX_LOG_LINE));
    }
}
