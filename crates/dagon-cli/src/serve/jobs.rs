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
    /// dagon.toml of the work area (None: no file, defaults only).
    pub config: Option<PathBuf>,
    pub web: PathBuf,
    /// Pages connected to the event stream (passphrase checked).
    pub pages: AtomicUsize,
    events: Mutex<Vec<mpsc::Sender<String>>>,
    jobs: Mutex<BTreeMap<u64, JobEntry>>,
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
            config,
            web,
            pages: AtomicUsize::new(0),
            events: Mutex::default(),
            jobs: Mutex::default(),
            next: AtomicU64::new(1),
            device: Mutex::new(Device::Page),
            device_cv: Condvar::new(),
            hub: Mutex::new(()),
        })
    }

    /// Sends `event` to every connected page.
    pub fn emit(&self, event: Value) {
        let text = event.to_string();
        let mut subs = self.events.lock().expect("events");
        subs.retain(|s| s.send(text.clone()).is_ok());
    }

    pub fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
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

    /// Starts `work` on its own thread; returns the job id.
    pub fn spawn<F>(self: &Arc<Self>, kind: &str, work: F) -> u64
    where
        F: FnOnce(&mut Job) -> Result<Value, String> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.lock().expect("jobs").insert(
            id,
            JobEntry {
                kind: kind.into(),
                state: "queued",
                cancel: cancel.clone(),
                answer: None,
                result: None,
            },
        );
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
        id
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
        self.lines.push(line.to_string());
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
