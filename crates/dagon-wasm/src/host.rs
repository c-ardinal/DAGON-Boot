//! dagon-host for the page: a [`Transport`] and [`Clock`] over a JavaScript
//! link object (Web Serial, or the simulated hub in development), and the
//! procedures the page calls. The page only moves bytes; every request,
//! timeout and retry is dagon-host's.

use alloc::rc::Rc;
use core::cell::RefCell;

use dagon_host::{
    check_release, reason, Clock, ConfirmOutcome, Discovery, Error, Event, Host, NodeOutcome,
    NodeState, Outcome, Target, Transport,
};
use dagon_proto::payload::{HelloResp, SlotInfo, StatusResp};
use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{future_to_promise, JsFuture};

use crate::watch::Watch;

#[wasm_bindgen(typescript_custom_section)]
const LINK_TS: &str = r#"
/** Byte link to the hub plus the clock the procedures run on. */
export interface HostLink {
  /** Sends all bytes. */
  write(bytes: Uint8Array): Promise<void>;
  /** Bytes received so far (at most `max`); empty when nothing arrived. */
  read(max: number): Promise<Uint8Array>;
  /** Closes and opens the link again (the hub's USB device leaves on reboot). */
  reopen(): Promise<void>;
  /** Monotonic milliseconds. */
  now(): number;
  sleep(ms: number): Promise<void>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "HostLink")]
    #[derive(Clone)]
    pub type HostLink;
    #[wasm_bindgen(method, catch)]
    fn write(this: &HostLink, bytes: &[u8]) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn read(this: &HostLink, max: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn reopen(this: &HostLink) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method)]
    fn now(this: &HostLink) -> f64;
    #[wasm_bindgen(method)]
    fn sleep(this: &HostLink, ms: u32) -> Promise;
}

pub struct JsTransport(HostLink);
pub struct JsClock(HostLink);

impl Transport for JsTransport {
    type Error = JsValue;
    async fn write(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        JsFuture::from(self.0.write(bytes)?).await.map(|_| ())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, JsValue> {
        let got = Uint8Array::new(&JsFuture::from(self.0.read(buf.len() as u32)?).await?);
        let n = (got.length() as usize).min(buf.len());
        got.subarray(0, n as u32).copy_to(&mut buf[..n]);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), JsValue> {
        JsFuture::from(self.0.reopen()?).await.map(|_| ())
    }
}

impl Clock for JsClock {
    fn now_ms(&self) -> u64 {
        self.0.now() as u64
    }
    async fn sleep_ms(&mut self, ms: u32) {
        let _ = JsFuture::from(self.0.sleep(ms)).await;
    }
}

type Inner = Host<JsTransport, JsClock>;

/// One session with one hub. Calls run one at a time; a call made while
/// another is running rejects with `busy`.
#[wasm_bindgen]
pub struct WasmHost {
    host: Rc<RefCell<Inner>>,
    watch: Rc<RefCell<Watch>>,
}

/// `kind` is `hub`, `i2c` (with the 7-bit `addr`) or `uart`.
fn target(kind: &str, addr: u8) -> Result<Target, JsValue> {
    match kind {
        "hub" => Ok(Target::Hub),
        "i2c" => Ok(Target::I2c(addr)),
        "uart" => Ok(Target::Uart),
        _ => Err(JsError::new("unknown target").into()),
    }
}

fn busy() -> JsValue {
    JsError::new("busy").into()
}

fn fail<E>(e: &Error<E>) -> JsValue {
    JsError::new(reason(e)).into()
}

// Holding the borrow across awaits is the one-call-at-a-time guard: an
// overlapping call fails `try_borrow_mut` and rejects with `busy`.
#[allow(clippy::await_holding_refcell_ref)]
#[wasm_bindgen]
impl WasmHost {
    #[wasm_bindgen(constructor)]
    pub fn new(link: HostLink) -> WasmHost {
        WasmHost {
            host: Rc::new(RefCell::new(Host::new(
                JsTransport(link.clone()),
                JsClock(link),
            ))),
            watch: Rc::new(RefCell::new(Watch::default())),
        }
    }

    fn run<F>(&self, f: impl FnOnce(Rc<RefCell<Inner>>, Rc<RefCell<Watch>>) -> F) -> Promise
    where
        F: core::future::Future<Output = Result<JsValue, JsValue>> + 'static,
    {
        future_to_promise(f(self.host.clone(), self.watch.clone()))
    }

    /// HELLO of one target. Resolves to the HELLO object.
    pub fn hello(&self, kind: &str, addr: u8) -> Promise {
        let to = target(kind, addr);
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            h.hello(to?)
                .await
                .map(|r| hello_js(&r))
                .map_err(|e| fail(&e))
        })
    }

    /// HELLO and STATUS of one target: `{hello, status}`.
    pub fn status(&self, kind: &str, addr: u8) -> Promise {
        let to = target(kind, addr);
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            h.status(to?)
                .await
                .map(|s| state_js(&s))
                .map_err(|e| fail(&e))
        })
    }

    /// Hub HELLO and every node that answers: `{hub, nodes: [{kind, addr, hello}]}`.
    pub fn discover(&self) -> Promise {
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            h.discover()
                .await
                .map(|d| discovery_js(&d))
                .map_err(|e| fail(&e))
        })
    }

    /// Sends a topology canonical binary to the hub.
    pub fn push_topology(&self, bin: Vec<u8>) -> Promise {
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            h.push_topology(&bin)
                .await
                .map(|_| JsValue::UNDEFINED)
                .map_err(|e| fail(&e))
        })
    }

    /// One monitoring round for one target: HELLO with the round-trip time.
    /// Resolves to `{responding, rttMs, hello}`; `responding` turns false
    /// only after consecutive misses (see `watch_interval_ms`).
    pub fn watch(&self, kind: &str, addr: u8) -> Promise {
        let to = target(kind, addr);
        self.run(|h, w| async move {
            let to = to?;
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            let start = h.clock.now_ms();
            let r = h.hello(to).await;
            let rtt = h.clock.now_ms().saturating_sub(start);
            if let Err(Error::Transport(e)) = r {
                return Err(e);
            }
            let responding = w.borrow_mut().record(to, r.is_ok());
            let o = Object::new();
            set(&o, "responding", responding.into());
            let ok = r.ok();
            set(
                &o,
                "rttMs",
                ok.map_or(JsValue::NULL, |_| (rtt as f64).into()),
            );
            set(&o, "hello", ok.map_or(JsValue::NULL, |r| hello_js(&r)));
            Ok(o.into())
        })
    }
}

/// Interval between monitoring rounds.
#[wasm_bindgen]
pub fn watch_interval_ms() -> u32 {
    crate::watch::INTERVAL_MS
}

/// Release verification key, fixed at build time (`DAGON_PUBKEY`, see build.rs).
pub const PUBKEY: [u8; 32] = hex32(env!("DAGON_PUBKEY_HEX"));

const fn hex32(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("DAGON_PUBKEY: not hex"),
        }
    }
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nib(b[2 * i]) << 4) | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

/// Why dagon-host refuses a release before sending anything: `source`
/// (signature, digest or CRC), `key` (signed with another key), `topology`
/// (built for another configuration), `format` (not a release).
pub fn refusal<E>(e: &Error<E>) -> &'static str {
    use dagon_format::FormatError as F;
    match e {
        Error::TopologyMismatch { .. } => "topology",
        Error::Format(F::UnknownKey) => "key",
        Error::Format(F::BadSignature | F::DigestMismatch | F::BadCrc) => "source",
        Error::Format(_) => "format",
        _ => reason(e),
    }
}

/// The verdict on `bundle` for `topology` under [`PUBKEY`]; `None` when it passes.
pub fn release_refusal(bundle: &[u8], topology: &[u8]) -> Option<&'static str> {
    check_release::<()>(bundle, topology, Some(&PUBKEY))
        .err()
        .map(|e| refusal(&e))
}

/// dagon-host's release check, the one `apply` runs first. Throws with the
/// refusal kind (`source`, `key`, `topology`, `format`).
#[wasm_bindgen]
pub fn check_release_for(bundle: &[u8], topology: &[u8]) -> Result<(), JsError> {
    match release_refusal(bundle, topology) {
        None => Ok(()),
        Some(k) => Err(JsError::new(k)),
    }
}

fn outcome_js(r: &NodeOutcome<JsValue>) -> JsValue {
    let o = Object::new();
    set(&o, "node", r.node.into());
    target_js(&o, r.target);
    let (kind, version, why) = match &r.outcome {
        Outcome::Done(v) => ("done", JsValue::from(v.raw()), JsValue::NULL),
        Outcome::Failed(e) => ("failed", JsValue::NULL, failure_js(e)),
        Outcome::NotStarted => ("notStarted", JsValue::NULL, JsValue::NULL),
    };
    set(&o, "outcome", kind.into());
    set(&o, "version", version);
    set(&o, "failure", why);
    o.into()
}

/// `{reason, running, trial}`: `reason` is dagon-host's short text
/// (`NO ANSWER`, `ROLLED BACK`, `E_SIGNATURE`, ...); `running` the version
/// the node came back on, when it came back on another one.
fn failure_js(e: &Error<JsValue>) -> JsValue {
    let o = Object::new();
    set(&o, "reason", reason(e).into());
    let (running, trial) = match e {
        Error::WrongVersion { running, trial, .. } => (JsValue::from(running.raw()), *trial),
        _ => (JsValue::NULL, false),
    };
    set(&o, "running", running);
    set(&o, "trial", trial.into());
    o.into()
}

fn event_js(e: &Event) -> JsValue {
    let o = Object::new();
    set(&o, "node", e.node.into());
    set(&o, "phase", e.phase.into());
    set(&o, "percent", e.percent.into());
    set(&o, "version", e.version.raw().into());
    o.into()
}

#[allow(clippy::await_holding_refcell_ref)]
#[wasm_bindgen]
impl WasmHost {
    /// `dagon apply`: checks the release under the built-in key, pushes the
    /// topology, then updates the nodes (children in update order, the hub
    /// last, reconnecting after its reboot, CONFIRM in the windows).
    /// `nodes` limits the run to these topology node indices. `on_event`
    /// gets `{node, phase, percent, version}`. Resolves to one
    /// `{node, kind, addr, outcome, version, failure}` per planned node;
    /// rejects with the refusal kind when nothing was sent.
    pub fn apply(
        &self,
        bundle: Vec<u8>,
        topology: Vec<u8>,
        nodes: Option<Vec<u8>>,
        on_event: js_sys::Function,
    ) -> Promise {
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            let mut emit = |e: Event| {
                let _ = on_event.call1(&JsValue::NULL, &event_js(&e));
            };
            let out = h
                .apply(&bundle, &topology, &PUBKEY, nodes.as_deref(), &mut emit)
                .await
                .map_err(|e| JsValue::from(JsError::new(refusal(&e))))?;
            Ok(out.iter().map(outcome_js).collect::<Array>().into())
        })
    }

    /// The confirm rule after a manual reconnect: CONFIRM only when the
    /// target runs on trial the version this session applied (or `force`).
    /// Resolves to `{outcome: 'confirmed' | 'notOnTrial' | 'needsChoice', version}`.
    pub fn confirm(&self, kind: &str, addr: u8, force: bool) -> Promise {
        let to = target(kind, addr);
        self.run(|h, _| async move {
            let mut h = h.try_borrow_mut().map_err(|_| busy())?;
            let r = h.confirm(to?, force).await.map_err(|e| fail(&e))?;
            let (kind, v) = match r {
                ConfirmOutcome::Confirmed(v) => ("confirmed", v),
                ConfirmOutcome::NotOnTrial(v) => ("notOnTrial", v),
                ConfirmOutcome::NeedsChoice(v) => ("needsChoice", v),
            };
            let o = Object::new();
            set(&o, "outcome", kind.into());
            set(&o, "version", v.raw().into());
            Ok(o.into())
        })
    }
}

fn set(o: &Object, k: &str, v: JsValue) {
    Reflect::set(o, &k.into(), &v).expect("plain object");
}

fn hello_js(h: &HelloResp) -> JsValue {
    let o = Object::new();
    set(&o, "protoVer", h.proto_ver.into());
    set(&o, "role", h.role.into());
    set(&o, "portKind", h.port_kind.into());
    set(&o, "mcuId", h.mcu_id.into());
    set(&o, "mode", h.mode.into());
    set(&o, "activeVersion", h.active_version.raw().into());
    set(&o, "trial", h.trial.into());
    set(&o, "keyId", Uint8Array::from(&h.key_id[..]).into());
    set(&o, "maxPayload", h.max_payload.into());
    o.into()
}

fn slot_js(s: &SlotInfo) -> JsValue {
    let o = Object::new();
    set(&o, "state", (s.state as u8).into());
    set(&o, "version", s.version.raw().into());
    set(&o, "digest16", Uint8Array::from(&s.digest16[..]).into());
    o.into()
}

fn status_js(s: &StatusResp) -> JsValue {
    let o = Object::new();
    let slots: Array = s.slots.iter().map(slot_js).collect();
    set(&o, "slots", slots.into());
    set(&o, "active", s.active.into());
    set(&o, "confirmed", s.confirmed.into());
    set(&o, "attempts", s.attempts.into());
    set(&o, "lastReason", s.last_reason.into());
    set(&o, "logGeneration", s.log_generation.into());
    o.into()
}

fn state_js(s: &NodeState) -> JsValue {
    let o = Object::new();
    set(&o, "hello", hello_js(&s.hello));
    set(&o, "status", status_js(&s.status));
    o.into()
}

fn target_js(o: &Object, t: Target) {
    let (kind, addr) = match t {
        Target::Hub => ("hub", 0),
        Target::I2c(a) => ("i2c", a),
        Target::Uart => ("uart", 0),
    };
    set(o, "kind", kind.into());
    set(o, "addr", addr.into());
}

fn discovery_js(d: &Discovery) -> JsValue {
    let o = Object::new();
    set(&o, "hub", hello_js(&d.hub));
    let nodes: Array = d
        .nodes
        .iter()
        .map(|(t, h)| {
            let n = Object::new();
            target_js(&n, *t);
            set(&n, "hello", hello_js(h));
            JsValue::from(n)
        })
        .collect();
    set(&o, "nodes", nodes.into());
    o.into()
}
