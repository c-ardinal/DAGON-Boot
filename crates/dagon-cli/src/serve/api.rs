//! The `/v1` API. Each endpoint runs a fixed CLI function; paths come from
//! dagon.toml only, never from the browser.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dagon_format::header::{image_kind, slot_binding, ImageHeader, HEADER_SIZE};
use dagon_sign::to_hex;
use dagon_topology::defs::{page_safe as plain_name, Registry};
use dagon_topology::schema::{Uplink, UplinkHost};
use serde_json::{json, Value};

use super::http::{Request, Response};
use super::jobs::{Job, Server};
use crate::commands::{image_for_node, version, SignOpts};
use crate::config::{load_key, read_file, read_topology, BuildInputs, Config};
use crate::nodebuild;

const SIGN_DIR: &str = "out/sign";
const RELEASE_DIR: &str = "out/release";

/// Node ids, release names: what may become a file name.
pub(crate) fn plain(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !s.starts_with('.')
        && !crate::paths::device_name(s)
}

/// `p` resolved inside the work area (symlinks and junctions followed).
pub(super) fn inside(cfg: &Config, p: &Path) -> Result<PathBuf, String> {
    crate::paths::contained(&cfg.base, p)
}

/// `dagon gen`, one node at a time: a cancel takes effect before the next
/// node; those made stay. Each line goes out once as it is made.
fn gen_nodes(
    config: &Path,
    only: &[String],
    force: bool,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<String>, String> {
    let ids = if only.is_empty() {
        let cfg = Config::load(Some(&config.to_string_lossy()))?;
        let t = read_topology(&cfg, &inside(&cfg, &cfg.topology_path(None)?)?)?;
        t.nodes.iter().map(|n| n.id.clone()).collect()
    } else {
        only.to_vec()
    };
    let mut lines: Vec<String> = Vec::new();
    for id in &ids {
        stop_if(cancel)?;
        for l in crate::scaffold::gen_project(config, std::slice::from_ref(id), force)? {
            if !lines.contains(&l) {
                log(&l);
                lines.push(l);
            }
        }
    }
    Ok(lines)
}

fn stop_if(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

/// A key file named in dagon.toml (or DAGON_SIGNING_KEY), resolved; it
/// must lie in the work area or a key folder. Messages carry no paths.
fn key_file(cfg: &Config, p: &Path, what: &str) -> Result<PathBuf, String> {
    let p = std::fs::canonicalize(p).map_err(|_| format!("the {what} file is not there"))?;
    let base = std::fs::canonicalize(&cfg.base).unwrap_or_else(|_| cfg.base.clone());
    if super::project::in_dir(&p, &base)
        || cfg.key_roots.iter().any(|r| super::project::in_dir(&p, r))
    {
        Ok(p)
    } else {
        Err(format!(
            "the {what} file is outside the project and the key folders"
        ))
    }
}

/// The project's public key from its pubkey file: only a `.pub` file that
/// is not a copy of the secret key beside it, holding a valid key.
fn public_key(cfg: &Config, p: &Path) -> Result<[u8; 32], String> {
    let p = key_file(cfg, p, "pubkey")?;
    let is_pub = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pub"));
    if !is_pub {
        return Err("pubkey must name a .pub file (from dagon key gen)".into());
    }
    crate::config::checked_pubkey(cfg, &p)
}

/// The project's signing key; refused when it does not match its pubkey.
fn project_key(cfg: &Config) -> Result<ed25519_compact::KeyPair, String> {
    let path = crate::config::signing_key_path(cfg).ok_or_else(|| {
        format!(
            "no signing key: set signing_key in dagon.toml or {}",
            crate::config::KEY_ENV
        )
    })?;
    let path = key_file(cfg, &path, "signing key")?;
    let key = load_key(Some(&path.to_string_lossy())).map_err(|_| {
        "the signing key file cannot be used (malformed, inside a git work tree, or the test key)"
            .to_string()
    })?;
    if let Some(p) = &cfg.pubkey {
        if public_key(cfg, p)? != *key.pk {
            return Err(format!(
                "the signing key (key id {}) does not match the project's pubkey",
                to_hex(&dagon_format::key_id(&key.pk))
            ));
        }
    }
    Ok(key)
}

/// The key the page verifies the project's releases with: its pubkey file,
/// else the public half of its signing key.
pub(super) fn release_key_of(cfg: &Config) -> Result<[u8; 32], String> {
    match &cfg.pubkey {
        Some(p) => public_key(cfg, p),
        None => Ok(*project_key(cfg)?.pk),
    }
}

/// The signing key for a job. Its file path never goes to the page: the
/// reason is printed in the serve terminal only.
pub(super) fn serve_key(cfg: &Config) -> Result<ed25519_compact::KeyPair, String> {
    page_safe(project_key(cfg))
}

fn page_safe<T>(r: Result<T, String>) -> Result<T, String> {
    r.map_err(|e| {
        eprintln!("dagon serve: signing key: {e}");
        "the signing key is not available; see the dagon serve terminal".to_string()
    })
}

fn config(server: &Server) -> Result<Config, String> {
    config_at(server, server.config_path().as_deref())
}

/// The project of dagon.toml `path`, with the key folders it may take key
/// files from.
/// Its error goes to the page (routes and jobs): the project's folder and
/// home in it are hidden.
pub(super) fn config_at(server: &Server, path: Option<&Path>) -> Result<Config, String> {
    let p = path.map(|p| p.to_string_lossy().into_owned());
    let mut c = Config::load(p.as_deref()).map_err(|e| {
        let swaps = crate::flash::base_swaps(path.and_then(Path::parent));
        crate::flash::hidden(&e, &swaps)
    })?;
    c.key_roots = super::project::home(server)
        .map(|h| super::project::key_dirs(&h))
        .unwrap_or_default();
    Ok(c)
}

fn body_json(req: &Request) -> Result<Value, Response> {
    serde_json::from_slice(&req.body).map_err(|_| Response::error(400, "body must be JSON"))
}

fn accepted(id: u64) -> Response {
    Response::json(202, &json!({ "job": id }))
}

fn nodes_of(v: &Value) -> Result<Vec<String>, Response> {
    let list = v["nodes"].as_array().cloned().unwrap_or_default();
    list.iter()
        .map(|n| match n.as_str() {
            Some(s) if plain(s) => Ok(s.to_string()),
            _ => Err(Response::error(400, "nodes must be node ids")),
        })
        .collect()
}

/// Routes one authorized API request.
pub fn handle(server: &Arc<Server>, req: &Request) -> Response {
    let path = req.path.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_start_matches("/v1/").split('/').collect();
    if server.sim.get().is_some() && demo_refuses(&parts) {
        return Response::error(403, "not in dagon demo (a simulator with its own project)");
    }
    if parts.first() == Some(&"ai") && !agent_linked(server) {
        return consent_refusal(server);
    }
    let r = match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["hello"]) => Ok(hello(server)),
        ("GET", ["agent"]) => Ok(agent_get(server)),
        ("POST", ["agent", "link"]) => agent_link(server, req),
        ("DELETE", ["agent", "link"]) => agent_unlink(server),
        ("PUT", ["agent", "key", agent, way]) => key_put(server, agent, way, req),
        ("DELETE", ["agent", "key", agent, way]) => key_delete(server, agent, way),
        ("POST", ["agent", "ask"]) => agent_ask(server, req),
        ("GET", ["definitions"]) => definitions(server),
        ("GET", ["topology"]) => topology_get(server),
        ("PUT", ["topology"]) => topology_put(server, req),
        ("GET", ["nodes", id, "build"]) => build_get(server, id),
        ("PUT", ["nodes", id, "build"]) => build_put(server, id, req),
        ("POST", ["build"]) => start(server, req, "build", job_build),
        ("POST", ["sign"]) => start(server, req, "sign", job_sign),
        ("POST", ["release"]) => start(server, req, "release", job_release),
        ("GET", ["release", "latest"]) => release_latest(server),
        ("GET", ["release", id]) => release_get(server, id),
        ("GET", ["kept"]) => kept_list(server),
        ("GET", ["kept", id]) => kept_get(server, id),
        ("POST", ["kept"]) => kept_post(server, req),
        ("GET", ["bootcore", node]) => bootcore_get(server, node),
        ("GET", ["stage0", node]) => stage0_get(server, node),
        ("GET", ["layout"]) => layout_get(server),
        ("PUT", ["layout"]) => layout_put(server, req),
        ("POST", ["conform"]) => start(server, req, "conform", job_conform),
        ("GET", ["conform", node]) => conform_get(server, node),
        ("POST", ["ai", "topology"]) => ai_topology_start(server, req),
        // Porting DAGON to a board: answers 501, this dagon has no porting job.
        ("POST", ["ai", "port"]) => Ok(Response::error(501, "porting is not available here")),
        ("GET", ["jobs", id]) => job_get(server, id),
        ("POST", ["jobs", id, "answer"]) => answer(server, id, req),
        ("POST", ["jobs", id, "cancel"]) => cancel(server, id),
        ("POST", ["device"]) => device(server, req),
        ("GET", ["project"]) => Ok(super::project::get(server)),
        ("POST", ["project", "open"]) => Ok(super::project::open(server, req)),
        ("POST", ["project", "new"]) => Ok(super::project::new(server, req)),
        ("POST", ["project", "init"]) => Ok(super::project::init(server, req)),
        ("GET", ["fs"]) => Ok(super::project::list(server, req)),
        ("POST", ["gen"]) => gen_start(server, req),
        ("POST", ["flash"]) => start(server, req, "flash", super::devjobs::job_flash),
        ("POST", ["doctor"]) => start(server, req, "doctor", super::devjobs::job_doctor),
        ("POST", ["sim", "write"]) => sim_write(server, req),
        ("GET", ["sim", "read"]) => sim_read(server, req),
        _ => Ok(Response::error(404, "no such endpoint")),
    };
    r.unwrap_or_else(|e| e)
}

/// What dagon demo refuses: other projects (opening one would load its
/// signing key) and everything that touches real hardware or the machine.
/// Never a real agent in a demo either: linking, questions, AI routes.
const DEMO_REFUSED: [&str; 12] = [
    "agent/link",
    "agent/key",
    "agent/ask",
    "ai",
    "project/open",
    "project/new",
    "project/init",
    "fs",
    "flash",
    "doctor",
    "conform",
    "device",
];

fn demo_refuses(parts: &[&str]) -> bool {
    let route = parts.join("/");
    DEMO_REFUSED
        .iter()
        .any(|r| route == *r || route.starts_with(&format!("{r}/")))
}

/// Features dagon demo leaves out of hello (the page hides them).
const DEMO_HIDDEN: [&str; 5] = ["project", "flash", "doctor", "conform", "ai_topology"];

/// The simulated medium a 構成's 親機 uplink names (None: not one the
/// demo simulates, or no 親機).
/// The medium `dagon demo` simulates for a 構成's 親機: Ok(None) without a
/// 親機, Err(uplink) for one it cannot simulate (a kind other than USB, UART
/// or Wi-Fi, or a custom device driving it), so the link on the screen is
/// always the one in use.
fn demo_uplink(yaml: &str) -> Result<Option<super::sim::Medium>, Uplink> {
    let Some(hub) = dagon_topology::parse_str(yaml).ok().and_then(|t| {
        t.nodes
            .into_iter()
            .find(|n| n.role == dagon_topology::schema::Role::Coordinator)
    }) else {
        return Ok(None);
    };
    let u = dagon_topology::Topology::uplink(&hub);
    match super::sim::Medium::of(u.kind) {
        Some(m) if u.host != UplinkHost::Embedded => Ok(Some(m)),
        _ => Err(u),
    }
}

fn host_name(h: UplinkHost) -> &'static str {
    match h {
        UplinkHost::PcCli => "pc-cli",
        UplinkHost::Browser => "browser",
        UplinkHost::Embedded => "embedded",
    }
}

/// The longest GET /v1/sim/read waits for an answer.
const SIM_READ_MAX_MS: u64 = 1_000;

fn sim_of(server: &Server) -> Result<&Arc<super::SimDevice>, Response> {
    server
        .sim
        .get()
        .ok_or_else(|| Response::error(404, "no simulator (dagon demo)"))
}

/// POST /v1/sim/write: bytes for the simulated hub's USB port.
fn sim_write(server: &Server, req: &Request) -> Res {
    sim_of(server)?.write(&req.body);
    Ok(json_ok(json!({})))
}

/// GET /v1/sim/read?wait=<ms>: what the simulated hub answered (empty
/// when nothing came within the wait).
fn sim_read(server: &Server, req: &Request) -> Res {
    let sim = sim_of(server)?;
    let wait = req
        .path
        .split_once('?')
        .and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("wait=")))
        .and_then(|w| w.parse::<u64>().ok())
        .unwrap_or(0)
        .min(SIM_READ_MAX_MS);
    let bytes = sim.read(std::time::Duration::from_millis(wait));
    Ok(Response::bytes("application/octet-stream", bytes))
}

/// What this serve offers the page. `flash` and `doctor` join when this
/// dagon has those commands.
const FEATURES: [&str; 13] = [
    "topology_save",
    "ai_topology",
    "build",
    "sign",
    "release",
    "conform",
    "gen",
    "project",
    "bootcore",
    "kept",
    "layout",
    "flash",
    "doctor",
];

/// Largest layout file (呼び名 and editor positions) accepted.
const MAX_LAYOUT: usize = 256 * 1024;

type Res = Result<Response, Response>;

/// POST /v1/gen {nodes?, force_main?}: `dagon gen` for the open project;
/// dagon.toml is edited under the files lock.
fn gen_start(server: &Arc<Server>, req: &Request) -> Res {
    let v = body_json(req)?;
    let only = match v.get("nodes") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => nodes_of(&v)?,
    };
    let force = v["force_main"].as_bool().unwrap_or(false);
    if server.config_path().is_none() {
        return Err(Response::error(409, "no project is open"));
    }
    let s = Arc::clone(server);
    match server.spawn("gen", move |job| {
        let (config, _, _) = job.project()?;
        let config = config.ok_or("no project is open")?;
        let cancel = job.cancel_flag();
        let lines = {
            let _lock = s.files.lock().map_err(|_| "lock".to_string())?;
            gen_nodes(&config, &only, force, &mut |l| job.log(l), &cancel)?
        };
        let skipped: Vec<&String> = lines.iter().filter(|l| l.starts_with("skipped")).collect();
        Ok(json!({ "lines": lines.len(), "skipped": skipped }))
    }) {
        Some(id) => Ok(accepted(id)),
        None => Err(Response::error(429, "too many jobs running")),
    }
}
type Work = fn(&mut Job, Config, Value) -> Result<Value, String>;

fn start(server: &Arc<Server>, req: &Request, kind: &str, work: Work) -> Res {
    let v = body_json(req)?;
    // The project is the one registered with the job (under the jobs lock),
    // so a project switch never lands between the two. Job::project loads
    // it (a project whose definitions do not load runs no job) and from
    // then on every line (cargo's, probe-rs', espflash's) and error reach
    // the page through the job's one scrubbing logger.
    match server.spawn(kind, move |job| {
        let (_, cfg, _) = job.project()?;
        work(job, cfg, v)
    }) {
        Some(id) => Ok(accepted(id)),
        None => Err(Response::error(429, "too many jobs running")),
    }
}

/// An error the page tells apart by `code`.
fn coded(status: u16, msg: &str, code: &str) -> Response {
    Response::json(status, &json!({ "error": msg, "code": code }))
}

/// Why no call may start: a consent file of a version this dagon does not
/// read (AGENT_CONSENT_VERSION), else not linked.
fn consent_refusal(server: &Server) -> Response {
    let unknown = super::project::home(server)
        .ok()
        .and_then(|h| crate::consent::unknown_version(&h));
    match unknown {
        Some(e) => coded(409, &e, crate::agents::codes::CONSENT_VERSION),
        None => not_linked(),
    }
}

fn not_linked() -> Response {
    coded(
        409,
        crate::consent::NOT_LINKED,
        crate::consent::NOT_LINKED_CODE,
    )
}

/// The linked agent, its linked way: the stand-in when set (tests), else
/// [`crate::agents::call`]. Every call, the stand-in's too, passes
/// [`crate::agents::before_call`] first and holds the consent through the
/// call. Err: the linked way cannot be used here (no program, no key).
fn agent_api(
    server: &Server,
    timeout: std::time::Duration,
) -> Result<Box<crate::agents::Api<'static>>, String> {
    let home = super::project::home(server).map_err(|_| crate::consent::NOT_LINKED.to_string())?;
    if let Some(f) = server.agent_api.get() {
        let f = Arc::clone(f);
        return Ok(Box::new(move |p: &crate::agents::Prompt| {
            // Held through the stand-in's whole call, like a real one.
            let _held = crate::agents::before_call(&home, p)?;
            f(p)
        }));
    }
    let keys = server.keys().clone();
    crate::agents::ready(&home, &keys)?;
    Ok(Box::new(move |p: &crate::agents::Prompt| {
        crate::agents::call(&home, &keys, p, timeout)
    }))
}

/// A model call's error as the page tells it apart.
fn agent_failure(e: &str) -> Response {
    match e {
        crate::consent::NOT_LINKED => not_linked(),
        e if e.starts_with(crate::consent::VERSION_REFUSAL) => {
            coded(409, e, crate::agents::codes::CONSENT_VERSION)
        }
        crate::agents::TOO_LARGE => coded(413, e, crate::agents::codes::CONTEXT_TOO_LARGE),
        crate::agents::TIMED_OUT => coded(504, e, crate::agents::codes::TIMEOUT),
        _ => coded(502, e, crate::agents::codes::FAILED),
    }
}

/// POST /v1/agent/ask: one question to DAGON, answered in this request.
fn agent_ask(server: &Server, req: &Request) -> Res {
    if !agent_linked(server) {
        return Err(consent_refusal(server));
    }
    let q = crate::ask::parse(&body_json(req)?).map_err(|e| Response::error(400, &e))?;
    let _turn = server.ask_turn().ok_or_else(|| {
        coded(
            429,
            "DAGON is answering another question",
            crate::agents::codes::BUSY,
        )
    })?;
    let mut api = agent_api(server, crate::ask::ASK_TIMEOUT).map_err(|e| {
        if e == crate::consent::NOT_LINKED || e.starts_with(crate::consent::VERSION_REFUSAL) {
            consent_refusal(server)
        } else {
            coded(409, &e, crate::agents::codes::UNAVAILABLE)
        }
    })?;
    let reg = registry_of(server).map_err(|e| Response::error(500, &e))?;
    let yaml = match topology_path(server) {
        Ok(p) => crate::ask::read_topology(&p).map_err(|e| agent_failure(&e))?,
        Err(_) => None,
    };
    let facts = crate::ask::Facts {
        reg: &reg,
        topology: yaml.as_deref(),
        jobs: server.recent_outcomes(crate::ask::MAX_JOBS),
    };
    let r = crate::ask::ask(&q, &facts, &mut *api).map_err(|e| agent_failure(&e))?;
    // Unlinked while the answer was on its way: it is not shown either.
    if !agent_linked(server) {
        return Err(consent_refusal(server));
    }
    Ok(json_ok(r.to_json()))
}

/// The agents in use: builtin, and the user's own (~/.dagon/agents).
fn agents_of(server: &Server) -> crate::agents::Agents {
    let home = super::project::home(server).ok();
    crate::agents::Agents::load(home.as_deref())
}

fn agent_linked(server: &Server) -> bool {
    super::project::home(server)
        .is_ok_and(|h| crate::consent::linked(&h, &agents_of(server)).is_some())
}

/// GET /v1/agent: the agents and their ways (whether each can be used
/// here; never a key, path or command), the consent, what linking sends.
fn agent_get(server: &Server) -> Response {
    let home = super::project::home(server).ok();
    json_ok(crate::consent::status(
        home.as_deref(),
        &agents_of(server),
        server.keys(),
    ))
}

/// The defined way `agent`/`way` that takes a key: 404 for no such way,
/// 400 for one that takes none (its login stays with the agent).
fn key_way(server: &Server, agent: &str, way: &str) -> Result<(), Response> {
    let agents = agents_of(server);
    let (_, w) = agents
        .way(agent, way)
        .ok_or_else(|| Response::error(404, "no such agent and way"))?;
    if w.transport.takes_key() {
        Ok(())
    } else {
        Err(Response::error(400, "this way takes no key"))
    }
}

/// A key store error: 503 AGENT_KEY_STORE when the store cannot be used,
/// with its reason (never the key).
fn key_store_failure(e: &str) -> Response {
    coded(503, e, crate::agents::codes::KEY_STORE)
}

/// PUT /v1/agent/key/<agent>/<way> {key}: registers the way's key in the
/// OS credential store. The answer is GET /v1/agent's body (whether a key
/// is registered, never the key).
fn key_put(server: &Server, agent: &str, way: &str, req: &Request) -> Res {
    key_way(server, agent, way)?;
    let v = body_json(req)?;
    let key = v["key"]
        .as_str()
        .ok_or_else(|| Response::error(400, "key must be a string"))?;
    let key = crate::keystore::check_key(key).map_err(|e| Response::error(400, &e))?;
    server
        .keys()
        .set(agent, way, key)
        .map_err(|e| key_store_failure(&e))?;
    Ok(agent_get(server))
}

/// DELETE /v1/agent/key/<agent>/<way>: removes the way's key (fine when
/// none was registered).
fn key_delete(server: &Server, agent: &str, way: &str) -> Res {
    key_way(server, agent, way)?;
    server
        .keys()
        .delete(agent, way)
        .map_err(|e| key_store_failure(&e))?;
    Ok(agent_get(server))
}

/// POST /v1/agent/link {agent, way, consent:true}: records the user's
/// consent to send to that agent that way; anything short of an explicit
/// yes for a defined agent and way records nothing.
fn agent_link(server: &Server, req: &Request) -> Res {
    let v = body_json(req)?;
    let agents = agents_of(server);
    let (a, w) = match (v["agent"].as_str(), v["way"].as_str()) {
        (Some(a), Some(w)) => agents.way(a, w),
        _ => None,
    }
    .ok_or_else(|| Response::error(400, "agent and way must name a defined agent and way"))?;
    if v["consent"] != true {
        return Err(Response::error(400, "consent must be true"));
    }
    let home = super::project::home(server)?;
    crate::consent::link(&home, &a.id, &w.id).map_err(|e| Response::error(500, &e))?;
    Ok(agent_get(server))
}

/// DELETE /v1/agent/link: withdraws the consent (done also when none was).
fn agent_unlink(server: &Server) -> Res {
    let home = super::project::home(server)?;
    crate::consent::unlink(&home).map_err(|e| Response::error(500, &e))?;
    // Running topology drafts stop too; every call checks the consent anyway.
    server.cancel_kind("ai_topology");
    Ok(agent_get(server))
}

/// Versions, work area, abilities; whether keys exist (never the keys).
fn hello(server: &Server) -> Response {
    let cfg = config(server).ok();
    let workspace = cfg
        .as_ref()
        .and_then(|c| std::fs::canonicalize(&c.base).ok())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let key = cfg
        .as_ref()
        .and_then(|c| project_key(c).ok())
        .map(|k| to_hex(&dagon_format::key_id(&k.pk)));
    // Definitions that do not load offer nothing to flash (GET
    // /v1/definitions says why).
    let mut defined: Vec<_> = registry_of(server).map_or(Vec::new(), |r| r.mcus().to_vec());
    defined.sort_by_key(|m| m.wire_id);
    let flash_mcus: Vec<String> = defined
        .into_iter()
        .map(|m| m.id)
        .filter(|m| plain_name(m))
        .collect();
    let sim = server.sim.get();
    let mut features = FEATURES.to_vec();
    if sim.is_some() {
        features.retain(|f| !DEMO_HIDDEN.contains(f));
        features.push("sim");
    }
    json_ok(json!({
        "cli": env!("CARGO_PKG_VERSION"),
        // The serve API number (the page refuses one it does not expect)
        // and every format number this dagon reads and writes.
        "api": crate::versions::SERVE_API,
        "formats": crate::versions::formats().into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        "workspace": workspace,
        "features": features,
        "key": key.is_some(),
        "key_id": key,
        "release_key": server.release_key().map(|k| to_hex(&k)),
        // The simulator of dagon demo: the page shows 模擬 and reaches
        // it through /v1/sim.
        "sim": sim.map(|s| {
            let m = s.medium();
            json!({
                "speed": s.speed,
                "fail": s.fail,
                // The simulated uplink: its kind, frame size and reboot behaviour.
                "uplink": m.kind().name(),
                "max_payload": m.max_payload(),
                "reconnect": if m.stays_on_reboot() { "stays" } else { "reopen" },
            })
        }),
        // The nodes with a [build.*] in dagon.toml: the others have no skeleton yet.
        "build_nodes": cfg.as_ref().map(|c| c.build.keys().cloned().collect::<Vec<_>>()),
        // The MCU families POST /v1/flash can write.
        "flash_mcus": flash_mcus,
        // null until a project is open (the page shows the start screen).
        "project": server.config_path().map(|_| workspace.clone()),
    }))
}

fn json_ok(v: Value) -> Response {
    Response::json(200, &v)
}

/// What the page is told when the open project's definitions do not load
/// (the reason, which may name its files, goes to the terminal only).
pub(super) const DEFS_ERROR: &str =
    "the project's definitions do not load; see the dagon serve terminal";

/// The definitions of project `cfg` (its DAGON source and its own); when
/// they do not load, the reason goes to the terminal and [`DEFS_ERROR`] to
/// the caller. Never the builtin set instead: a project MCU must not just
/// vanish.
pub(super) fn project_registry(cfg: &Config) -> Result<Registry, String> {
    crate::defs::registry(cfg).map_err(|e| {
        eprintln!("dagon serve: {e}");
        DEFS_ERROR.to_string()
    })
}

/// The open project's definitions; the builtin set only while no project
/// is open.
fn registry_of(server: &Server) -> Result<Registry, String> {
    if server.config_path().is_none() {
        return Ok(Registry::builtin().clone());
    }
    project_registry(&config(server)?)
}

/// GET /v1/definitions: the definitions in use as `Registry::to_json`
/// gives them, for the page's dagon-wasm (`load_definitions`).
fn definitions(server: &Server) -> Res {
    let reg = registry_of(server).map_err(|e| Response::error(500, &e))?;
    Ok(Response::bytes(
        "application/json",
        reg.to_json().into_bytes(),
    ))
}

fn topology_path(server: &Server) -> Result<PathBuf, Response> {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = cfg
        .topology
        .clone()
        .ok_or_else(|| Response::error(409, "dagon.toml names no topology"))?;
    inside(&cfg, &p).map_err(|e| Response::error(403, &e))
}

/// The project's 構成 YAML as it is on disk (404 before the first save).
fn topology_get(server: &Server) -> Res {
    let p = topology_path(server)?;
    let yaml = std::fs::read(&p).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Response::error(404, "no topology file yet"),
        // A file not read is not a project without one: the page must not save over it.
        _ => Response::error(500, &e.to_string()),
    })?;
    Ok(Response::bytes("application/yaml; charset=utf-8", yaml))
}

/// Writes the topology YAML after the same check `dagon topo check` runs.
// Writes the project's topology file, the user's file the page edits.
#[allow(clippy::disallowed_methods)]
fn topology_put(server: &Server, req: &Request) -> Res {
    let p = topology_path(server)?;
    let yaml =
        std::str::from_utf8(&req.body).map_err(|_| Response::error(400, "YAML must be UTF-8"))?;
    let reg = registry_of(server).map_err(|e| Response::error(500, &e))?;
    let report = dagon_topology::check_str_with(&reg, yaml);
    let report_json: Value = serde_json::from_str(&report.to_json()).unwrap_or(Value::Null);
    // A part updating does not reach yet (V-23) is information, not an
    // error: the 構成 is saved and its updatable part compiled.
    if !report.is_pushable() {
        return Err(Response::json(422, &json!({ "report": report_json })));
    }
    let cut = dagon_topology::parse_str(yaml)
        .ok()
        .map(|t| dagon_topology::cut_with(&reg, &t));
    // dagon demo: the 構成's uplink picks the simulated medium; one the
    // demo cannot simulate is not saved, so the 構成 and the medium agree.
    let medium = match server.sim.get().map(|_| demo_uplink(yaml)) {
        Some(Err(u)) => {
            return Err(Response::json(
                422,
                &json!({
                    "report": report_json,
                    "demo_uplink": { "kind": u.kind.name(), "host": host_name(u.host) },
                }),
            ))
        }
        Some(Ok(m)) => m,
        None => None,
    };
    let _lock = server
        .files
        .lock()
        .map_err(|_| Response::error(500, "lock"))?;
    crate::paths::write_atomic(&p, yaml.as_bytes()).map_err(|e| Response::error(500, &e))?;
    if let (Some(sim), Some(m)) = (server.sim.get(), medium) {
        sim.set_medium(m);
    }
    Ok(json_ok(
        json!({ "report": report_json, "updatable": cut.as_ref().is_some_and(|c| c.topology.is_some()), "reach": cut.as_ref().map(|c| c.reach.clone()).unwrap_or_default(), "skipped": skipped_of(cut.as_ref()) }),
    ))
}

/// `{id: why}` of the nodes updating skips (`Skip::code`); every
/// node (`cut-invalid` where nothing else applies) when no part is updatable.
fn skipped_of(cut: Option<&dagon_topology::Cut>) -> Value {
    Value::Object(
        cut.map(|c| c.skipped.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|(id, why)| (id.clone(), Value::from(why.code())))
            .collect(),
    )
}

/// The node's `[build.<id>]` table of dagon.toml (404 when it has none).
fn build_get(server: &Server, id: &str) -> Res {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let b = cfg
        .build
        .get(id)
        .ok_or_else(|| Response::error(404, "no [build] table for this node"))?;
    Ok(json_ok(serde_json::to_value(b).unwrap_or(Value::Null)))
}

/// Replaces `[build.<id>]` in dagon.toml, keeping the rest of the file.
// Updates the project's dagon.toml ([build.<id>]), the user's file.
#[allow(clippy::disallowed_methods)]
fn build_put(server: &Server, id: &str, req: &Request) -> Res {
    if !plain(id) {
        return Err(Response::error(400, "bad node id"));
    }
    let b: BuildInputs = serde_json::from_slice(&req.body)
        .map_err(|e| Response::error(400, &format!("bad build settings: {e}")))?;
    b.validate().map_err(|e| Response::error(400, &e))?;
    let path = server
        .config_path()
        .ok_or_else(|| Response::error(409, "no dagon.toml in the work area"))?;
    // Read, edit and replace under one lock so concurrent PUTs all land.
    let _lock = server
        .files
        .lock()
        .map_err(|_| Response::error(500, "lock"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| Response::error(500, &e.to_string()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| Response::error(500, "dagon.toml does not parse"))?;
    let table: toml_edit::DocumentMut = toml::to_string(&b)
        .map_err(|e| Response::error(500, &e.to_string()))?
        .parse()
        .map_err(|_| Response::error(500, "cannot encode the settings"))?;
    let builds = doc
        .entry("build")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| Response::error(409, "build in dagon.toml is not a table"))?;
    builds.set_implicit(true);
    builds.insert(id, toml_edit::Item::Table(table.as_table().clone()));
    crate::paths::write_atomic(&path, doc.to_string().as_bytes())
        .map_err(|e| Response::error(500, &e))?;
    Ok(json_ok(serde_json::to_value(&b).unwrap_or(Value::Null)))
}

/// Builds the applications of `nodes` (all when left out) as a job; the answer lists each built slot and its size.
fn job_build(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let nodes = nodes_of(&v).map_err(|_| "nodes must be node ids".to_string())?;
    if let Some(p) = &cfg.pubkey {
        page_safe(public_key(&cfg, p))?;
    }
    let cancel = job.cancel_flag();
    let built = nodebuild::build_nodes(&cfg, &nodes, &mut |l| job.log(l), &cancel)?;
    let list: Vec<Value> = built
        .iter()
        .map(|b| {
            let slots: Vec<Value> = b
                .slots
                .iter()
                .map(|(s, _, size)| json!({"slot": s.to_string(), "bytes": size}))
                .collect();
            json!({"node": b.node, "slots": slots})
        })
        .collect();
    Ok(json!({ "nodes": list }))
}

#[cfg(test)]
fn sign_path(cfg: &Config, node: &str, slot: char) -> PathBuf {
    cfg.base.join(SIGN_DIR).join(format!("{node}-{slot}.img"))
}

#[cfg(test)]
fn sign_record_path(cfg: &Config, node: &str) -> PathBuf {
    cfg.base.join(SIGN_DIR).join(format!("{node}.json"))
}

/// What the last successful signing of a node produced: (file name,
/// SHA-256 hex) per image, and the version.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct SignRecord {
    version: String,
    images: Vec<(String, String)>,
}

fn sha_hex(b: &[u8]) -> String {
    to_hex(&dagon_format::sha256(b))
}

/// Signs the built applications of `nodes` with the CLI's key.
fn job_sign(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let nodes = nodes_of(&v).map_err(|_| "nodes must be node ids".to_string())?;
    let t = read_topology(&cfg, &inside(&cfg, &cfg.topology_path(None)?)?)?;
    let key = serve_key(&cfg)?;
    let cancel = job.cancel_flag();
    sign_nodes(&cfg, &t, &nodes, &key, &mut |l| job.log(l), &cancel)
}

/// Signs exactly what each node's last successful build recorded, with the
/// version recorded there. All or nothing: every node's inputs are checked
/// and every image signed before a file changes, and the new images and
/// records replace the old ones together (see [`commit_files`]); any
/// failure or cancel leaves the old images and records as they were.
fn sign_nodes(
    cfg: &Config,
    t: &dagon_topology::Topology,
    nodes: &[String],
    key: &ed25519_compact::KeyPair,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Value, String> {
    // Every node's inputs are checked before anything is removed: a typo
    // in the list or a missing build leaves the signed images as they are.
    // One sign at a time: they share the .new/.old names in out/sign.
    let _one = SIGN_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut ready = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if nodes[..i].contains(node) {
            return Err(format!("node '{node}' is listed twice"));
        }
        if t.node(node).is_none() {
            return Err(format!("no updatable node '{node}' in the topology"));
        }
        let (built, elfs) = nodebuild::last_build(cfg, node)?;
        let ver = built
            .version
            .ok_or_else(|| format!("{node} was built without a version; build it again"))?;
        let parsed = version(&ver)?;
        ready.push((node, ver, parsed, elfs));
    }
    let mut flat = Vec::new();
    for (node, ver, parsed, elfs) in ready {
        let mut built = crate::deploy::release_inputs(cfg, t, vec![(node.clone(), parsed, elfs)])
            .map_err(|e| format!("{node}: {e}"))?;
        let (_, _, elfs) = built.pop().ok_or("no build")?;
        flat.push((node, ver, parsed, elfs));
    }
    // All images are signed in memory first; files change only after
    // every one succeeded, so a bad ELF, a signing error or a cancel
    // leaves the old images and records as they were.
    let reg = crate::defs::registry(cfg)?;
    let mut out = Vec::new();
    let mut signed = Vec::new();
    for (node, ver, parsed, elfs) in flat {
        let opts = SignOpts {
            version: parsed,
            release_seq: 1,
            min_bootcore: Default::default(),
            proto: 1,
            bits: None,
        };
        let mut images = Vec::new();
        for (slot, elf) in elfs {
            stop_if(cancel)?;
            let binding = if slot == 'a' {
                slot_binding::A
            } else {
                slot_binding::B
            };
            let img = image_for_node(
                &reg,
                t,
                node,
                image_kind::APP,
                binding,
                &elf,
                Some(&opts),
                key,
            )
            .map_err(|e| format!("{node}: {e}"))?;
            log(&format!(
                "{node}: slot {slot} signed, {} bytes, {ver}",
                img.len()
            ));
            out.push(
                json!({"node": node, "slot": slot.to_string(), "bytes": img.len(), "version": ver}),
            );
            images.push((slot, img));
        }
        let cores = sign_cores(cfg, &reg, t, node, key, log, cancel)?;
        signed.push((node, ver, images, cores));
    }
    stop_if(cancel)?;
    let mut files = Vec::new();
    for (node, ver, images, cores) in &signed {
        files.extend(sign_swaps(node, ver, images, cores)?);
    }
    let base = cfg.base.clone();
    commit_files(&base, &files, &mut |from, to| {
        crate::genfs::rename(&base, from, to)
    })?;
    Ok(json!({ "images": out }))
}

/// Node `node`'s boot core, built from its boot crate (as `dagon deploy`
/// does) and signed as the boot crate's Cargo.toml version, for every boot
/// core slot its MCU declares; none without a boot crate. The node writes it
/// only when that version differs from the boot core it runs.
fn sign_cores(
    cfg: &Config,
    reg: &Registry,
    t: &dagon_topology::Topology,
    node: &str,
    key: &ed25519_compact::KeyPair,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<(char, Vec<u8>)>, String> {
    let dir = cfg.base.join(crate::extgen::BOOT_CRATES).join(node);
    let Some(n) = t.node(node) else {
        return Ok(Vec::new());
    };
    if !dir.join("Cargo.toml").is_file() {
        return Ok(Vec::new());
    }
    let opts = SignOpts {
        version: crate::srcver::crate_version(&dir)?,
        release_seq: 1,
        min_bootcore: Default::default(),
        proto: 1,
        bits: None,
    };
    let built = crate::flash::build_core_slots(cfg, reg, n, &to_hex(&*key.pk), log, cancel)?;
    built
        .into_iter()
        .map(|(slot, elf)| {
            stop_if(cancel)?;
            let binding = if slot == 'b' {
                slot_binding::B
            } else {
                slot_binding::A
            };
            image_for_node(
                reg,
                t,
                node,
                image_kind::BOOTCORE,
                binding,
                &elf,
                Some(&opts),
                key,
            )
            .map(|img| (slot, img))
        })
        .collect()
}

/// Held for a whole sign (stage and commit).
static SIGN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One file of a sign: its path in the work area and new bytes (None:
/// the old file goes, a slot no longer signed).
type FileSwap = (crate::genfs::Rel, Option<Vec<u8>>);

/// A node's new images and record, and its old images of slots no longer
/// signed (to remove); the record last. Paths stay as written (never
/// resolved): every file operation of the swap checks them for links.
fn sign_swaps(
    node: &str,
    ver: &str,
    images: &[(char, Vec<u8>)],
    cores: &[(char, Vec<u8>)],
) -> Result<Vec<FileSwap>, String> {
    let mut record = SignRecord {
        version: ver.to_string(),
        images: Vec::new(),
    };
    let mut files = Vec::new();
    // The app's images, then its boot core's (`bootcore-<node>-<slot>.img`).
    for (prefix, set) in [("", images), ("bootcore-", cores)] {
        for slot in ['a', 'b'] {
            let rel = crate::genfs::Rel::new(format!("{SIGN_DIR}/{prefix}{node}-{slot}.img"))?;
            let Some((_, img)) = set.iter().find(|(s, _)| *s == slot) else {
                files.push((rel, None));
                continue;
            };
            record
                .images
                .push((rel.file_name().to_string(), sha_hex(img)));
            files.push((rel, Some(img.clone())));
        }
    }
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    files.push((
        crate::genfs::Rel::new(format!("{SIGN_DIR}/{node}.json"))?,
        Some(text.into_bytes()),
    ));
    Ok(files)
}

/// Replaces `files` all together, every write, rename and removal through
/// genfs (a link on the way refuses it). Every new file is written as
/// `<name>.new` first (a failure removes them all; nothing changed). Then
/// each old file becomes `<name>.old` and its `.new` takes its name; if a
/// `rename` fails, every step so far is undone (the `.old` files back, no
/// `.new` left) and the error returned. Only when all are in place do the
/// `.old` files go.
fn commit_files(
    base: &Path,
    files: &[FileSwap],
    rename: &mut dyn FnMut(&crate::genfs::Rel, &crate::genfs::Rel) -> Result<(), String>,
) -> Result<(), String> {
    use crate::genfs;
    let new_of = |p: &genfs::Rel| p.with_suffix(".new");
    let old_of = |p: &genfs::Rel| p.with_suffix(".old");
    let remove_new = || {
        for (p, _) in files {
            let _ = genfs::remove(base, &new_of(p));
        }
    };
    for (path, bytes) in files {
        if let Some(b) = bytes {
            if let Err(e) = genfs::write(base, &new_of(path), b) {
                remove_new();
                return Err(e);
            }
        }
    }
    // (path, moved to .old, new in place) for every step taken.
    let mut done: Vec<(&genfs::Rel, bool, bool)> = Vec::new();
    let mut failed = None;
    for (path, bytes) in files {
        let moved = match genfs::checked(base, path) {
            Ok(p) => p.exists(),
            Err(e) => {
                failed = Some(e);
                break;
            }
        };
        if moved {
            if let Err(e) = rename(path, &old_of(path)) {
                failed = Some(format!("cannot replace {path}: {e}"));
                break;
            }
        }
        done.push((path, moved, false));
        if bytes.is_some() {
            if let Err(e) = rename(&new_of(path), path) {
                failed = Some(format!("cannot replace {path}: {e}"));
                break;
            }
            done.last_mut().expect("pushed").2 = true;
        }
    }
    if let Some(e) = failed {
        let mut undo = Vec::new();
        for (path, moved, placed) in done.into_iter().rev() {
            if placed {
                if let Err(x) = genfs::remove(base, path) {
                    undo.push(format!("{path}: {x}"));
                }
            }
            if moved {
                if let Err(x) = genfs::rename(base, &old_of(path), path) {
                    undo.push(format!("{path} not restored: {x}"));
                }
            }
        }
        remove_new();
        if undo.is_empty() {
            return Err(e);
        }
        return Err(format!("{e}; undoing it failed too ({})", undo.join("; ")));
    }
    for (path, moved, _) in done {
        if moved {
            let _ = genfs::remove(base, &old_of(path));
        }
    }
    Ok(())
}

/// `path` with `suffix` added to its file name (same folder).
#[cfg(test)]
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Builds a release from every signed image, each of the version its node's
/// crate gave it (`version`, if given, must be every image's).
fn job_release(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let want = v["version"].as_str().map(version).transpose()?;
    // Without a name the release is named by its newest version (v1.2.0).
    let named;
    let name = match v.get("name").and_then(Value::as_str) {
        Some(n) => Some(n),
        None => {
            named = newest_name(&cfg, want)?;
            Some(named.as_str())
        }
    };
    let name = name.filter(|n| plain(n) && n.len() <= 16);
    let name = name.ok_or("name must be up to 16 plain characters")?;
    let t = read_topology(&cfg, &inside(&cfg, &cfg.topology_path(None)?)?)?;
    let key = serve_key(&cfg)?;
    let cancel = job.cancel_flag();
    release_from(&cfg, &t, want, name, &key, &mut |l| job.log(l), &cancel)
}

/// `v<newest version>` of the signed images.
fn newest_name(cfg: &Config, want: Option<dagon_format::Version>) -> Result<String, String> {
    let newest = signed_images(cfg, want)?
        .iter()
        .filter_map(|img| {
            let raw: &[u8; HEADER_SIZE] = img.get(..HEADER_SIZE)?.try_into().ok()?;
            ImageHeader::parse(raw).ok().map(|h| h.version)
        })
        .max()
        .unwrap_or_default();
    Ok(format!("v{newest}"))
}

/// Images listed by the sign records only (each checked against its
/// recorded SHA-256), all of version `want` when one is given.
fn signed_images(
    cfg: &Config,
    want: Option<dagon_format::Version>,
) -> Result<Vec<Vec<u8>>, String> {
    let dir = inside(cfg, &cfg.base.join(SIGN_DIR))?;
    let mut records: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|_| "nothing signed yet".to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    records.sort();
    let mut images = Vec::new();
    for r in &records {
        let r = &inside(cfg, r)?;
        let rec: SignRecord =
            serde_json::from_slice(&read_file(r)?).map_err(|e| format!("{}: {e}", r.display()))?;
        for (file, sha) in &rec.images {
            if !plain(file) {
                return Err(format!("{}: bad file name", r.display()));
            }
            let img = read_file(&inside(cfg, &dir.join(file))?)?;
            if sha_hex(&img) != *sha {
                return Err(format!("{file} changed since it was signed"));
            }
            let raw: &[u8; HEADER_SIZE] = img
                .get(..HEADER_SIZE)
                .and_then(|h| h.try_into().ok())
                .ok_or_else(|| format!("{file}: not an image"))?;
            crate::versions::check_number("image-header", file, raw)?;
            let h = ImageHeader::parse(raw).map_err(|e| format!("{file}: {e:?}"))?;
            if let Some(want) = want.filter(|w| h.version != *w) {
                return Err(format!("{file} is {}, not {want}", h.version));
            }
            images.push(img);
        }
    }
    if images.is_empty() {
        return Err("nothing signed yet".into());
    }
    Ok(images)
}

fn release_from(
    cfg: &Config,
    t: &dagon_topology::Topology,
    want: Option<dagon_format::Version>,
    name: &str,
    key: &ed25519_compact::KeyPair,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Value, String> {
    stop_if(cancel)?;
    let reg = crate::defs::registry(cfg)?;
    let topo_bin = dagon_topology::compile_with(&reg, t).map_err(|e| e.to_string())?;
    let images = signed_images(cfg, want)?;
    stop_if(cancel)?;
    let r = dagon_sign::release::build(&reg, t, &topo_bin, &images, name, key)
        .map_err(|e| e.to_string())?;
    stop_if(cancel)?;
    let rel = crate::genfs::Rel::new(format!("{RELEASE_DIR}/{name}.dgb"))?;
    crate::genfs::write(&cfg.base, &rel, &r.bundle)?;
    let sha = to_hex(&dagon_format::sha256(&topo_bin));
    log(&format!(
        "release {name}: {} images, {} bytes",
        images.len(),
        r.bundle.len()
    ));
    Ok(json!({ "release": name, "topology_sha256": sha, "bytes": r.bundle.len() }))
}

/// A release of the project's out/release by name, once it verifies with the project's key.
fn release_get(server: &Server, id: &str) -> Res {
    if !plain(id) {
        return Err(Response::error(400, "bad release id"));
    }
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = inside(&cfg, &cfg.base.join(RELEASE_DIR).join(format!("{id}.dgb")))
        .map_err(|e| Response::error(403, &e))?;
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no such release"))?;
    verified(server, bytes, |b, k| {
        dagon_format::bundle::BundleView::parse(b)
            .and_then(|v| v.verify(k))
            .is_ok()
    })
}

/// The bytes, once they verify under the open project's key (the one the
/// page verifies with): a file signed otherwise is not handed out.
fn verified(server: &Server, bytes: Vec<u8>, ok: impl Fn(&[u8], &[u8; 32]) -> bool) -> Res {
    let key = server
        .release_key()
        .ok_or_else(|| match server.config_path() {
            Some(_) => Response::error(409, "the open project has no public key to verify with"),
            None => Response::error(409, "no project is open"),
        })?;
    if !ok(&bytes, &key) {
        return Err(Response::error(
            422,
            "the file does not verify with the project's key",
        ));
    }
    Ok(Response::bytes("application/octet-stream", bytes))
}

/// GET /v1/release/latest: the newest .dgb of the project's out/release.
fn release_latest(server: &Server) -> Res {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let dir = inside(&cfg, &cfg.base.join(RELEASE_DIR)).map_err(|e| Response::error(403, &e))?;
    let newest = std::fs::read_dir(&dir)
        .map_err(|_| Response::error(404, "no release yet"))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".dgb")?.to_string();
            let at = e.metadata().and_then(|m| m.modified()).ok()?;
            plain(&id).then_some((at, id))
        })
        .max()
        .ok_or_else(|| Response::error(404, "no release yet"))?;
    release_get(server, &newest.1)
}

/// The open project, its 構成 and the key its packages verify with.
fn kept_ctx(server: &Server) -> Result<(Config, dagon_topology::Topology, [u8; 32]), Response> {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let key = server
        .release_key()
        .ok_or_else(|| Response::error(409, "the open project has no public key to verify with"))?;
    let path = inside(
        &cfg,
        &cfg.topology_path(None)
            .map_err(|e| Response::error(500, &e))?,
    )
    .map_err(|e| Response::error(403, &e))?;
    let t = read_topology(&cfg, &path).map_err(|e| Response::error(409, &e))?;
    Ok((cfg, t, key))
}

/// GET /v1/kept: the packages kept in the project (out/deploy), newest
/// version first, each with the version it gives each node.
fn kept_list(server: &Server) -> Res {
    let (cfg, t, key) = kept_ctx(server)?;
    let list: Vec<Value> = crate::kept::list(&cfg, &t, &key)
        .into_iter()
        .map(|k| {
            let versions: serde_json::Map<String, Value> = k
                .versions
                .iter()
                .map(|(n, v)| (n.clone(), json!(v.to_string())))
                .collect();
            json!({ "id": k.id, "versions": versions, "bytes": k.bytes })
        })
        .collect();
    Ok(json_ok(json!({ "kept": list })))
}

/// GET /v1/kept/{id}: a kept package's bytes, once they verify.
fn kept_get(server: &Server, id: &str) -> Res {
    if !plain(id) {
        return Err(Response::error(400, "bad package id"));
    }
    let (cfg, _, key) = kept_ctx(server)?;
    crate::kept::read(&cfg, &key, id)
        .map(|b| Response::bytes("application/octet-stream", b))
        .map_err(|_| Response::error(404, "no such kept package"))
}

/// POST /v1/kept (body: the package): keeps a package about to be written,
/// once it verifies with the project's key; answers its id.
fn kept_post(server: &Server, req: &Request) -> Res {
    let (cfg, t, key) = kept_ctx(server)?;
    let _lock = server
        .files
        .lock()
        .map_err(|_| Response::error(500, "lock"))?;
    crate::kept::store(&cfg, &t, &key, &req.body)
        .map(|id| json_ok(json!({ "id": id })))
        .map_err(|e| Response::error(422, &e))
}

/// The most `POST /v1/kept` takes: by the open project's definitions, which
/// may add larger MCUs, else the built-in ones.
pub(super) fn largest_package(server: &Server) -> usize {
    match config(server).and_then(|c| crate::defs::registry(&c)) {
        Ok(reg) => crate::kept::largest_package(&reg),
        Err(_) => crate::kept::largest_package(dagon_topology::defs::Registry::builtin()),
    }
}

/// GET /v1/bootcore/{node}: the node's signed boot core (the ROM path):
/// [provision.<node>] bootcore, else <provision_dir>/<node>/bootcore.img.
fn bootcore_get(server: &Server, node: &str) -> Res {
    if !plain(node) {
        return Err(Response::error(400, "bad node id"));
    }
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = match cfg.provision.get(node) {
        Some(inputs) => inputs.bootcore.clone(),
        None => cfg.provision_dir().join(node).join("bootcore.img"),
    };
    let p = inside(&cfg, &p).map_err(|e| Response::error(403, &e))?;
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no boot core for this node"))?;
    verified(server, bytes, |b, k| {
        let kind = dagon_sign::image::verify_image(b, k).map(|h| h.image_kind);
        kind.is_ok_and(|k| k == image_kind::BOOTCORE)
    })
}

/// GET /v1/stage0/{node}: stage0's bytes for the ROM path's full rewrite of
/// an MCU with a stage0, as provision wrote them:
/// <provision_dir>/<node>/stage0.bin. Stage0 carries no signature; the
/// host checks it fits the MCU's stage0 region before writing it.
fn stage0_get(server: &Server, node: &str) -> Res {
    if !plain(node) {
        return Err(Response::error(400, "bad node id"));
    }
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = cfg
        .provision_dir()
        .join(node)
        .join(crate::commands::STAGE0_FILE);
    let p = inside(&cfg, &p).map_err(|e| Response::error(403, &e))?;
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no stage0 for this node"))?;
    Ok(Response::bytes("application/octet-stream", bytes))
}

/// The layout file beside the 構成: `<topology>.layout.json`.
fn layout_path(server: &Server) -> Result<PathBuf, Response> {
    let p = topology_path(server)?;
    let name = p
        .file_stem()
        .map(|s| format!("{}.layout.json", s.to_string_lossy()))
        .ok_or_else(|| Response::error(409, "dagon.toml names no topology"))?;
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    inside(&cfg, &p.with_file_name(name)).map_err(|e| Response::error(403, &e))
}

/// GET /v1/layout: 呼び名 and editor positions (an empty layout before the first save).
fn layout_get(server: &Server) -> Res {
    let p = layout_path(server)?;
    if !p.exists() {
        return Ok(json_ok(json!({ "format": crate::versions::LAYOUT_FORMAT })));
    }
    let bytes = crate::paths::read_capped(&p, MAX_LAYOUT)
        .map_err(|e| Response::error(500, &e.to_string()))?
        .ok_or_else(|| Response::error(413, "the layout file is too large"))?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Response::error(422, "the layout file is not JSON"))?;
    check_layout(&v).map_err(|e| Response::error(422, &e))?;
    Ok(Response::bytes("application/json", bytes))
}

/// A layout file's shape: its format number (or none, read as this one),
/// `names` as strings and `positions` as `{x, y}` numbers, by node id.
fn check_layout(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("layout must be a JSON object")?;
    if let Some(n) = o.get("format") {
        let ours = u64::from(crate::versions::LAYOUT_FORMAT);
        if n.as_u64() != Some(ours) {
            return Err(format!("layout format {n}; this dagon reads {ours}"));
        }
    }
    if let Some(names) = o.get("names") {
        let ok = names
            .as_object()
            .is_some_and(|m| m.values().all(Value::is_string));
        if !ok {
            return Err("layout names must be strings by node id".into());
        }
    }
    if let Some(places) = o.get("positions") {
        let number =
            |p: &Value, k: &str| p.get(k).and_then(Value::as_f64).is_some_and(f64::is_finite);
        let ok = places
            .as_object()
            .is_some_and(|m| m.values().all(|p| number(p, "x") && number(p, "y")));
        if !ok {
            return Err("layout positions must be {x, y} numbers by node id".into());
        }
    }
    Ok(())
}

/// Why the layout file at `p` cannot be read; none when it can, or there is none.
fn unreadable_layout(p: &Path) -> Option<String> {
    let bytes = match crate::paths::read_capped(p, MAX_LAYOUT) {
        Ok(Some(b)) => b,
        Ok(None) => return Some("the layout file is too large".into()),
        Err(_) if !p.exists() => return None,
        Err(e) => return Some(e.to_string()),
    };
    let v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Some("the layout file is not JSON".into()),
    };
    check_layout(&v).err()
}

/// PUT /v1/layout: a layout of this format, written whole.
// Writes the project's layout file, the user's file the page edits.
#[allow(clippy::disallowed_methods)]
fn layout_put(server: &Server, req: &Request) -> Res {
    if req.body.len() > MAX_LAYOUT {
        return Err(Response::error(413, "layout too large"));
    }
    let v: Value = serde_json::from_slice(&req.body)
        .map_err(|_| Response::error(400, "layout must be JSON"))?;
    check_layout(&v).map_err(|e| Response::error(400, &e))?;
    let p = layout_path(server)?;
    let text = serde_json::to_vec_pretty(&v).map_err(|e| Response::error(500, &e.to_string()))?;
    let _lock = server
        .files
        .lock()
        .map_err(|_| Response::error(500, "lock"))?;
    // Replacing is whole: a file no reader could have taken is not lost to a page that did not see it.
    if let Some(e) = unreadable_layout(&p) {
        return Err(Response::error(409, &format!("not replaced: {e}")));
    }
    crate::paths::write_atomic(&p, &text).map_err(|e| Response::error(500, &e))?;
    Ok(json_ok(json!({ "saved": true })))
}

/// The 12 conformance tests on `node` through the hub, after the page
/// hands over the port.
fn job_conform(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let node = v["node"]
        .as_str()
        .filter(|n| plain(n))
        .ok_or("node is required")?;
    let t = read_topology(&cfg, &inside(&cfg, &cfg.topology_path(None)?)?)?;
    let to = crate::conform::node_target(&crate::defs::registry(&cfg)?, &t, node)?;
    // Read and written as written (never resolved); the writes go through
    // genfs, which refuses a link on the way.
    let dir = cfg.base.join(crate::conform::CONFORM_DIR).join(node);
    let out = crate::genfs::Out::Work(crate::genfs::Rel::new(format!(
        "{}/{node}",
        crate::conform::CONFORM_DIR
    ))?);
    let key = serve_key(&cfg)?;
    job.progress("waiting for the hub", None);
    let report = job.with_device(|job| {
        job.progress("running", None);
        crate::conform::conform_files(node, to, &dir, &out, &cfg.base, &key, None)
    })?;
    let file = crate::conform::json(&report, node);
    Ok(serde_json::from_str(&file).unwrap_or(Value::Null))
}

/// The node's last conformance result (404 before the first run).
fn conform_get(server: &Server, node: &str) -> Res {
    if !plain(node) {
        return Err(Response::error(400, "bad node id"));
    }
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = cfg
        .base
        .join(crate::conform::CONFORM_DIR)
        .join(node)
        .join(crate::conform::RESULT_FILE);
    let p = inside(&cfg, &p).map_err(|e| Response::error(403, &e))?;
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no result yet"))?;
    Ok(Response::bytes("application/json", bytes))
}

/// POST /v1/ai/topology {description}: a job; the model's questions go to
/// the page.
fn ai_topology_start(server: &Arc<Server>, req: &Request) -> Res {
    let v = body_json(req)?;
    let s = Arc::clone(server);
    match server.spawn("ai_topology", move |job| {
        let (_, _, reg) = job.project()?;
        job_ai_topology(job, &s, &reg, &v)
    }) {
        Some(id) => Ok(accepted(id)),
        None => Err(Response::error(429, "too many jobs running")),
    }
}

fn job_ai_topology(job: &mut Job, s: &Server, reg: &Registry, v: &Value) -> Result<Value, String> {
    let description = v["description"]
        .as_str()
        .ok_or("description is required")?
        .to_string();
    let mut post = agent_api(s, crate::ai::DRAFT_TIMEOUT)?;
    let cancel = job.cancel_flag();
    let mut ask = |qs: &[String]| -> Result<Vec<String>, String> {
        qs.iter().enumerate().map(|(i, q)| job.ask(i, q)).collect()
    };
    let (yaml, report) = crate::ai::topology_for(&description, reg, &mut ask, &cancel, &mut *post)?;
    let report: Value = serde_json::from_str(&report).unwrap_or(Value::Null);
    Ok(json!({ "yaml": yaml, "report": report }))
}

fn job_id(id: &str) -> Result<u64, Response> {
    id.parse().map_err(|_| Response::error(400, "bad job id"))
}

/// A job's state, stage, progress, log lines, question and result.
fn job_get(server: &Server, id: &str) -> Res {
    server
        .job_json(job_id(id)?)
        .map(json_ok)
        .ok_or_else(|| Response::error(404, "no such job"))
}

/// `{answer}` to the job's open question (409 when it has none).
fn answer(server: &Server, id: &str, req: &Request) -> Res {
    let id = job_id(id)?;
    let v = body_json(req)?;
    let text = v["answer"]
        .as_str()
        .ok_or_else(|| Response::error(400, "answer is required"))?;
    if server.answer(id, text.to_string()) {
        Ok(Response::json(202, &json!({ "job": id })))
    } else {
        Err(Response::error(409, "no open question"))
    }
}

/// Stops a running job.
fn cancel(server: &Server, id: &str) -> Res {
    let id = job_id(id)?;
    if server.cancel(id) {
        Ok(accepted(id))
    } else {
        Err(Response::error(404, "no such job"))
    }
}

/// `{state: "released"}`: the page has closed the hub's port, so the CLI job waiting for it can use it.
fn device(server: &Server, req: &Request) -> Res {
    let v = body_json(req)?;
    if v["state"] != "released" {
        return Err(Response::error(400, "state must be released"));
    }
    server.device_released();
    Ok(json_ok(json!({ "state": "released" })))
}

mod page {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// Whether a page is built into this executable.
pub fn page_embedded() -> bool {
    page::ASSETS.iter().any(|(p, _)| *p == "index.html")
}

/// A file of the page: from the executable when `server.web` is empty,
/// otherwise from that folder. HTML gets the release key as
/// `<meta name="dagon-release-key">`; without a key no HTML is served.
pub fn page_file(server: &Server, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let (bytes, ct) = if server.web.as_os_str().is_empty() {
        embedded_file(path)?
    } else {
        static_file(&server.web, path)?
    };
    if !ct.starts_with("text/html") {
        return Some((bytes, ct));
    }
    match server.release_key() {
        Some(key) => Some((inject_key(&bytes, &key)?, ct)),
        // No project yet: the start screen, with nothing to verify with.
        None => Some((bytes, ct)),
    }
}

fn inject_key(html: &[u8], key: &[u8; 32]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(html).ok()?;
    let meta = format!(
        "<meta name=\"dagon-release-key\" content=\"{}\">",
        to_hex(key)
    );
    let at = text.find("<head>").map(|i| i + "<head>".len()).unwrap_or(0);
    Some(format!("{}{meta}{}", &text[..at], &text[at..]).into_bytes())
}

fn embedded_file(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = path.split(['?', '#']).next()?.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let (_, bytes) = page::ASSETS.iter().find(|(p, _)| *p == rel)?;
    Some((bytes.to_vec(), content_type(rel)))
}

fn content_type(rel: &str) -> &'static str {
    match rel.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "png" => "image/png",
        "yaml" => "application/yaml",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Static file under `web` for `path` (the built page), or None.
pub fn static_file(web: &Path, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = path.split(['?', '#']).next()?.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let safe = rel.split('/').all(|c| {
        !c.is_empty()
            && c != ".."
            && c != "."
            && !c.contains('\\')
            && !c.contains(':')
            && !crate::paths::device_name(c)
    });
    if !safe {
        return None;
    }
    // Symlinks and junctions are followed, and must stay in the page root.
    let root = std::fs::canonicalize(web).ok()?;
    let file = std::fs::canonicalize(web.join(rel)).ok()?;
    if !file.starts_with(&root) || !file.is_file() {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    Some((bytes, content_type(rel)))
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_is_of_this_format_with_names_and_finite_places() {
        assert!(check_layout(
            &json!({"format": 1, "names": {"a": "A"}, "positions": {"a": {"x": 1, "y": -2.5}}})
        )
        .is_ok());
        assert!(check_layout(&json!({"names": {}})).is_ok());
        for bad in [
            json!([]),
            json!({"format": 2}),
            json!({"format": "1"}),
            json!({"names": {"a": 1}}),
            json!({"positions": {"a": {"x": 1}}}),
            json!({"positions": {"a": {"x": "1", "y": 2}}}),
            json!({"positions": {"a": {"x": null, "y": 2}}}),
        ] {
            assert!(check_layout(&bad).is_err(), "{bad}");
        }
    }

    /// RFC 8032 section 7.1 TEST 1 key (public test data).
    fn test_key() -> ed25519_compact::KeyPair {
        let seed = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];
        ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed))
    }

    /// A work area whose seg node has a recorded build of 1.1.0 (raw
    /// payloads for both slots), as `dagon build` leaves it.
    fn built_area(name: &str) -> (Config, dagon_topology::Topology) {
        let base = std::env::temp_dir().join(format!("dagon-sign-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("out/build/seg");
        std::fs::create_dir_all(&dir).unwrap();
        let mut slots = Vec::new();
        for slot in ['a', 'b'] {
            let body = vec![slot as u8; 1500];
            std::fs::write(dir.join(format!("app-{slot}.elf")), &body).unwrap();
            slots.push((slot, sha_hex(&body)));
        }
        let rec = nodebuild::BuildRecord {
            version: Some("1.1.0".into()),
            slots,
        };
        std::fs::write(dir.join("build.json"), serde_json::to_string(&rec).unwrap()).unwrap();
        let cfg = Config {
            base,
            ..Config::default()
        };
        let yaml = include_str!("../../../../topology/sim-pair.yaml");
        (cfg, dagon_topology::parse_str(yaml).unwrap())
    }

    #[test]
    fn a_signed_boot_core_is_recorded_with_its_node_so_a_release_takes_it() {
        let files = sign_swaps(
            "seg",
            "1.1.0",
            &[('a', b"APP A".to_vec()), ('b', b"APP B".to_vec())],
            &[('a', b"CORE A".to_vec())],
        )
        .unwrap();
        let named = |n: &str| files.iter().find(|(r, _)| r.file_name() == n);
        assert!(named("bootcore-seg-a.img").unwrap().1.is_some());
        assert!(
            named("bootcore-seg-b.img").unwrap().1.is_none(),
            "a slot without one goes"
        );
        let (_, record) = files.last().unwrap();
        let rec: SignRecord = serde_json::from_slice(record.as_ref().unwrap()).unwrap();
        let listed: Vec<&str> = rec.images.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(listed, ["seg-a.img", "seg-b.img", "bootcore-seg-a.img"]);
    }

    /// 中止 of 雛形を生成する: the node being made finishes, the next is not started.
    #[test]
    fn gen_stops_between_nodes_when_cancelled() {
        let d = std::env::temp_dir().join(format!("dagon-gen-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let dir = d.join("proj");
        let here = std::fs::canonicalize(crate::dagonsrc::BUILT_FROM).unwrap();
        let src = crate::dagonsrc::NewSource::Clone(crate::config::plain_path(here));
        crate::scaffold::new_project(&dir, "proj", crate::scaffold::SAMPLE, &d.join("keys"), &src)
            .unwrap();
        let config = dir.join(crate::config::CONFIG_FILE);
        let cancel = AtomicBool::new(false);
        let r = gen_nodes(
            &config,
            &[],
            false,
            &mut |_| cancel.store(true, Ordering::Relaxed),
            &cancel,
        );
        assert_eq!(r.unwrap_err(), "cancelled");
        assert!(dir.join("nodes/hub/Cargo.toml").exists());
        assert!(!dir.join("nodes/seg").exists());
        // Not cancelled: every node, and each line once.
        let lines = gen_nodes(&config, &[], false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert!(dir.join("nodes/watch/Cargo.toml").exists());
        let mut once = lines.clone();
        once.dedup();
        assert_eq!(lines, once);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `dagon new` and `dagon gen`, built and signed once; then the app
    /// crate's and the boot crate's versions raised in their Cargo.toml,
    /// `dagon gen` and the Console's build, sign and release again: the
    /// package carries both new versions (real firmware builds).
    fn raised_versions_reach_the_package(node: &str) {
        let d = std::env::temp_dir().join(format!("dagon-raise-{node}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let dir = d.join("proj");
        let here = std::fs::canonicalize(crate::dagonsrc::BUILT_FROM).unwrap();
        let src = crate::dagonsrc::NewSource::Clone(crate::config::plain_path(here));
        let yaml = crate::scaffold::SAMPLE;
        crate::scaffold::new_project(&dir, "proj", yaml, &d.join("keys"), &src).unwrap();
        let config = dir.join(crate::config::CONFIG_FILE);
        let load = || Config::load(Some(&config.to_string_lossy())).unwrap();
        let t = dagon_topology::parse_str(yaml).unwrap();
        let key = test_key();
        let only = [node.to_string()];
        let mut log = Vec::new();
        let cycle = |log: &mut Vec<String>| {
            let lines = crate::scaffold::gen_project(&config, &only, false).unwrap();
            assert!(!lines.iter().any(|l| l.starts_with("skipped")), "{lines:?}");
            let cfg = load();
            let go = AtomicBool::new(false);
            nodebuild::build_nodes(&cfg, &only, &mut |l| log.push(l.into()), &go)
                .and_then(|_| sign_nodes(&cfg, &t, &only, &key, &mut |l| log.push(l.into()), &go))
                .unwrap_or_else(|e| panic!("{e}\n{}", log.join("\n")));
        };
        cycle(&mut log);
        let raise = |rel: &str, to: &str| {
            let path = dir.join(rel).join("Cargo.toml");
            let text = std::fs::read_to_string(&path).unwrap();
            let doc: toml_edit::DocumentMut = text.parse().unwrap();
            let from = doc["package"]["version"].as_str().unwrap().to_string();
            let raised = text.replacen(
                &format!("version = \"{from}\""),
                &format!("version = \"{to}\""),
                1,
            );
            assert_ne!(raised, text, "{rel}");
            std::fs::write(&path, raised).unwrap();
        };
        raise(&format!("nodes/{node}"), "1.3.0");
        raise(&format!("boot/{node}"), "0.2.0");
        cycle(&mut log);
        let go = AtomicBool::new(false);
        release_from(&load(), &t, None, "raised", &key, &mut |_| {}, &go).unwrap();
        let bundle = std::fs::read(dir.join(RELEASE_DIR).join("raised.dgb")).unwrap();
        let view = dagon_format::bundle::BundleView::parse(&bundle).unwrap();
        let versions: Vec<(u8, dagon_format::Version)> = (0..view.item_count())
            .filter(|&i| Some(i) != view.topology_item())
            .map(|i| {
                let raw = &view.item_bytes(i)[..HEADER_SIZE];
                let h = ImageHeader::parse(raw.try_into().unwrap()).unwrap();
                (h.image_kind, h.version)
            })
            .collect();
        let of = |kind: u8| -> Vec<dagon_format::Version> {
            versions
                .iter()
                .filter(|v| v.0 == kind)
                .map(|v| v.1)
                .collect()
        };
        assert_eq!(
            of(image_kind::APP),
            [dagon_format::Version::new(1, 3, 0); 2]
        );
        assert_eq!(
            of(image_kind::BOOTCORE),
            [dagon_format::Version::new(0, 2, 0); 2]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[ignore = "builds firmware for the STM32F401"]
    fn raised_versions_reach_the_package_f401() {
        raised_versions_reach_the_package("seg");
    }

    /// An ESP32-S3 app is signed as the flat image its OTA slots hold, as in
    /// `dagon deploy`, never as the ELF: the ELF's sections are mapped far from
    /// the slot. A stand-in ELF stops at that step, whether espflash is
    /// installed or not, and the failure names the node.
    #[test]
    fn signing_an_esp32s3_node_goes_through_its_esp_image() {
        let (cfg, _) = built_area("esp");
        let t =
            dagon_topology::parse_str(include_str!("../../../../topology/sample.yaml")).unwrap();
        let dir = cfg.base.join("out/build/watch");
        std::fs::create_dir_all(&dir).unwrap();
        let body = vec![0x41u8; 1500];
        std::fs::write(dir.join("app-a.elf"), &body).unwrap();
        let rec = nodebuild::BuildRecord {
            version: Some("0.1.0".into()),
            slots: vec![('a', sha_hex(&body))],
        };
        std::fs::write(dir.join("build.json"), serde_json::to_string(&rec).unwrap()).unwrap();
        let watch = ["watch".to_string()];
        let e = sign_nodes(
            &cfg,
            &t,
            &watch,
            &test_key(),
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(e.starts_with("watch: ") && e.contains("espflash"), "{e}");
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn signing_uses_only_the_recorded_build() {
        let (cfg, t) = built_area("recorded");
        let key = test_key();
        let seg = ["seg".to_string()];
        sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert!(sign_path(&cfg, "seg", 'a').exists() && sign_path(&cfg, "seg", 'b').exists());

        // A stale image that no record lists never reaches a release.
        std::fs::write(cfg.base.join(SIGN_DIR).join("hub-a.img"), b"stale").unwrap();
        let v = dagon_format::Version::new(1, 1, 0);
        assert_eq!(signed_images(&cfg, Some(v)).unwrap().len(), 2);
        let r = release_from(
            &cfg,
            &t,
            Some(v),
            "r1",
            &key,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(r["release"], "r1");
        assert!(signed_images(&cfg, Some(dagon_format::Version::new(1, 0, 0))).is_err());
        // Without a version given, the images go as their crates versioned them.
        assert_eq!(signed_images(&cfg, None).unwrap().len(), 2);
        // Without a name the release is named by the newest of them.
        assert_eq!(newest_name(&cfg, None).unwrap(), "v1.1.0");

        // A signed image changed afterwards is refused.
        std::fs::write(sign_path(&cfg, "seg", 'b'), b"changed").unwrap();
        assert!(signed_images(&cfg, Some(v)).is_err());

        // Signed again; then requests that fail their checks remove
        // nothing: an unknown node, an ELF changed after its build.
        sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let before = std::fs::read(sign_path(&cfg, "seg", 'a')).unwrap();
        let typo = ["seg".to_string(), "sge".to_string()];
        let e =
            sign_nodes(&cfg, &t, &typo, &key, &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("sge"), "{e}");
        std::fs::write(nodebuild::elf_path(&cfg, "seg", 'a'), b"later").unwrap();
        assert!(sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &AtomicBool::new(false)).is_err());
        assert_eq!(std::fs::read(sign_path(&cfg, "seg", 'a')).unwrap(), before);
        assert_eq!(
            signed_images(&cfg, Some(v)).unwrap().len(),
            2,
            "still the signed build"
        );
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    /// The signed images and record of seg, byte for byte.
    fn signed_files(cfg: &Config) -> Vec<Vec<u8>> {
        [
            sign_path(cfg, "seg", 'a'),
            sign_path(cfg, "seg", 'b'),
            sign_record_path(cfg, "seg"),
        ]
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect()
    }

    /// seg's slot A build replaced by `body` (recorded, so the build check
    /// passes).
    fn rebuild_slot_a(cfg: &Config, body: &[u8]) {
        let dir = cfg.base.join("out/build/seg");
        std::fs::write(dir.join("app-a.elf"), body).unwrap();
        let b = std::fs::read(dir.join("app-b.elf")).unwrap();
        let rec = nodebuild::BuildRecord {
            version: Some("1.1.0".into()),
            slots: vec![('a', sha_hex(body)), ('b', sha_hex(&b))],
        };
        std::fs::write(dir.join("build.json"), serde_json::to_string(&rec).unwrap()).unwrap();
    }

    /// hub gets a recorded build too (raw payloads, like seg's).
    fn build_hub(cfg: &Config) {
        let dir = cfg.base.join("out/build/hub");
        std::fs::create_dir_all(&dir).unwrap();
        let mut slots = Vec::new();
        for slot in ['a', 'b'] {
            let body = vec![slot as u8 ^ 0x30; 1500];
            std::fs::write(dir.join(format!("app-{slot}.elf")), &body).unwrap();
            slots.push((slot, sha_hex(&body)));
        }
        let rec = nodebuild::BuildRecord {
            version: Some("1.1.0".into()),
            slots,
        };
        std::fs::write(dir.join("build.json"), serde_json::to_string(&rec).unwrap()).unwrap();
    }

    /// Every file in the sign folder, by name.
    fn sign_folder(cfg: &Config) -> std::collections::BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(cfg.base.join(SIGN_DIR))
            .unwrap()
            .flatten()
            .map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (name, std::fs::read(e.path()).unwrap_or_default())
            })
            .collect()
    }

    #[test]
    fn signing_several_nodes_is_all_or_nothing() {
        let (cfg, t) = built_area("allornothing");
        build_hub(&cfg);
        let key = test_key();
        let both = ["seg".to_string(), "hub".to_string()];
        let go = AtomicBool::new(false);
        sign_nodes(&cfg, &t, &both, &key, &mut |_| {}, &go).unwrap();
        let old = sign_folder(&cfg);
        assert_eq!(old.len(), 6, "{:?}", old.keys());
        rebuild_slot_a(&cfg, &[0x42; 1500]);

        // The second node's file cannot be staged: nothing changes.
        let blocked = with_suffix(&sign_path(&cfg, "hub", 'a'), ".new");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(sign_nodes(&cfg, &t, &both, &key, &mut |_| {}, &go).is_err());
        std::fs::remove_dir_all(&blocked).unwrap();
        assert_eq!(sign_folder(&cfg), old, "staging failure on node 2");

        // A rename fails in the middle of the commit (after seg is in place):
        // every step is undone.
        let mut images = Vec::new();
        let opts = SignOpts {
            version: version("1.1.0").unwrap(),
            release_seq: 1,
            min_bootcore: Default::default(),
            proto: 1,
            bits: None,
        };
        for node in ["seg", "hub"] {
            let (_, elfs) = nodebuild::last_build(&cfg, node).unwrap();
            let mut imgs = Vec::new();
            for (slot, elf) in elfs {
                let b = if slot == 'a' {
                    slot_binding::A
                } else {
                    slot_binding::B
                };
                imgs.push((
                    slot,
                    image_for_node(
                        Registry::builtin(),
                        &t,
                        node,
                        image_kind::APP,
                        b,
                        &elf,
                        Some(&opts),
                        &key,
                    )
                    .unwrap(),
                ));
            }
            images.push((node, imgs));
        }
        let mut files = Vec::new();
        for (node, imgs) in &images {
            files.extend(sign_swaps(node, "1.1.0", imgs, &[]).unwrap());
        }
        for fail_at in [1usize, 4, 7, 9] {
            let mut n = 0;
            let r = commit_files(&cfg.base, &files, &mut |from, to| {
                n += 1;
                if n == fail_at {
                    return Err("injected".into());
                }
                crate::genfs::rename(&cfg.base, from, to)
            });
            assert!(r.is_err(), "rename {fail_at}");
            assert_eq!(
                sign_folder(&cfg),
                old,
                "rename {fail_at} failed: all undone"
            );
        }

        // With nothing failing, both nodes change together.
        sign_nodes(&cfg, &t, &both, &key, &mut |_| {}, &go).unwrap();
        let new = sign_folder(&cfg);
        assert_eq!(new.len(), 6, "no .new or .old left: {:?}", new.keys());
        assert_ne!(new, old);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn signs_run_one_at_a_time_and_a_node_is_listed_once() {
        let (cfg, t) = built_area("concurrent");
        build_hub(&cfg);
        let key = test_key();
        let both = ["seg".to_string(), "hub".to_string()];
        let twice = ["seg".to_string(), "seg".to_string()];
        let go = AtomicBool::new(false);
        let e = sign_nodes(&cfg, &t, &twice, &key, &mut |_| {}, &go).unwrap_err();
        assert!(e.contains("twice"), "{e}");
        std::thread::scope(|sc| {
            for _ in 0..4 {
                sc.spawn(|| {
                    sign_nodes(&cfg, &t, &both, &key, &mut |_| {}, &AtomicBool::new(false)).unwrap()
                });
            }
        });
        let files = sign_folder(&cfg);
        assert_eq!(files.len(), 6, "no .new or .old left: {:?}", files.keys());
        let v = dagon_format::Version::new(1, 1, 0);
        assert_eq!(signed_images(&cfg, Some(v)).unwrap().len(), 4);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn a_failed_sign_leaves_the_old_images_and_record() {
        let (cfg, t) = built_area("keep");
        let key = test_key();
        let seg = ["seg".to_string()];
        let go = AtomicBool::new(false);
        sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &go).unwrap();
        let old = signed_files(&cfg);

        // A build that cannot be signed (an empty slot A).
        rebuild_slot_a(&cfg, b"");
        assert!(sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &go).is_err());
        assert_eq!(signed_files(&cfg), old, "malformed build");

        // Cancelled after slot A was signed, before slot B.
        rebuild_slot_a(&cfg, &[0x42; 1500]);
        let stop = AtomicBool::new(false);
        let r = sign_nodes(
            &cfg,
            &t,
            &seg,
            &key,
            &mut |_| stop.store(true, Ordering::Relaxed),
            &stop,
        );
        assert_eq!(r.unwrap_err(), "cancelled");
        assert_eq!(signed_files(&cfg), old, "cancelled");

        // A file that cannot be written.
        std::fs::create_dir_all(with_suffix(&sign_path(&cfg, "seg", 'b'), ".new")).unwrap();
        assert!(sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &go).is_err());
        assert_eq!(signed_files(&cfg), old, "write failure");
        assert!(
            !with_suffix(&sign_path(&cfg, "seg", 'a'), ".new").exists(),
            "no staged file left"
        );
        std::fs::remove_dir_all(with_suffix(&sign_path(&cfg, "seg", 'b'), ".new")).unwrap();

        // Then it goes through, and the release takes the new images.
        sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &go).unwrap();
        assert_ne!(signed_files(&cfg), old);
        let v = dagon_format::Version::new(1, 1, 0);
        assert_eq!(signed_images(&cfg, Some(v)).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    /// A server on `cfg`'s work area (dagon.toml there) verifying with `key`.
    fn server_on(cfg: &Config, key: [u8; 32]) -> Arc<Server> {
        let toml = cfg.base.join("dagon.toml");
        std::fs::write(&toml, "topology = \"t.yaml\"\n").unwrap();
        let s = Server::new("p".repeat(32), 1, Some(toml), PathBuf::new());
        s.set_release_key(key);
        s
    }

    fn status(r: Res) -> u16 {
        match r {
            Ok(r) | Err(r) => r.status,
        }
    }

    #[test]
    fn the_latest_release_is_served_only_when_it_verifies() {
        let (cfg, t) = built_area("latest");
        let key = test_key();
        let server = server_on(&cfg, *key.pk);
        assert_eq!(status(release_latest(&server)), 404, "none yet");
        sign_nodes(
            &cfg,
            &t,
            &["seg".into()],
            &key,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        let v = dagon_format::Version::new(1, 1, 0);
        let stop = AtomicBool::new(false);
        release_from(&cfg, &t, Some(v), "r1", &key, &mut |_| {}, &stop).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        release_from(&cfg, &t, Some(v), "r2", &key, &mut |_| {}, &stop).unwrap();
        let r2 = std::fs::read(cfg.base.join(RELEASE_DIR).join("r2.dgb")).unwrap();
        let got = release_latest(&server).unwrap();
        assert_eq!((got.status, got.body.clone()), (200, r2));
        // Signed with another key than the project's: not handed out.
        server.set_release_key([9; 32]);
        assert_eq!(status(release_latest(&server)), 422);
        assert_eq!(status(release_get(&server, "r1")), 422);
        // A file that is no release at all.
        server.set_release_key(*key.pk);
        std::fs::write(cfg.base.join(RELEASE_DIR).join("r1.dgb"), b"junk").unwrap();
        assert_eq!(status(release_get(&server, "r1")), 422);
        assert_eq!(status(release_get(&server, "..")), 400);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    fn bootcore_image(key: &ed25519_compact::KeyPair) -> Vec<u8> {
        use dagon_sign::image::{self, sign_image, ImageSpec};
        let (flags, caps) = image::default_bits(image_kind::BOOTCORE);
        let mcu = "stm32f401re";
        let spec = ImageSpec {
            image_kind: image_kind::BOOTCORE,
            port_kind: dagon_topology::port_kind_of("self-boot").unwrap(),
            mcu_id: dagon_topology::mcu_id_of(mcu).unwrap(),
            slot: image::slot(mcu, image_kind::BOOTCORE, 0).unwrap(),
            version: dagon_format::Version::new(1, 0, 0),
            build_flags: flags,
            release_seq: 1,
            min_bootcore_version: dagon_format::Version::new(0, 0, 0),
            target_tag: dagon_format::header::target_tag("seg-f401-7seg"),
            proto_version: 1,
            capabilities: caps,
        };
        sign_image(&spec, &[0xB0; 2000], key).unwrap()
    }

    #[test]
    fn without_a_key_the_reason_says_which_is_missing() {
        let msg = |s: &Server| {
            let r = verified(s, vec![1], |_, _| true).unwrap_err();
            (r.status, String::from_utf8_lossy(&r.body).into_owned())
        };
        let none = Server::new("p".repeat(32), 1, None, PathBuf::new());
        let (st, body) = msg(&none);
        assert!(st == 409 && body.contains("no project is open"), "{body}");
        let open = Server::new(
            "p".repeat(32),
            1,
            Some(PathBuf::from("dagon.toml")),
            PathBuf::new(),
        );
        let (st, body) = msg(&open);
        assert!(st == 409 && body.contains("no public key"), "{body}");
    }

    #[test]
    fn a_boot_core_is_served_only_when_it_verifies() {
        let (cfg, _) = built_area("bootcore");
        let key = test_key();
        let server = server_on(&cfg, *key.pk);
        assert_eq!(status(bootcore_get(&server, "seg")), 404);
        assert_eq!(status(bootcore_get(&server, "..")), 400);
        let dir = cfg.provision_dir().join("seg");
        std::fs::create_dir_all(&dir).unwrap();
        let img = bootcore_image(&key);
        std::fs::write(dir.join("bootcore.img"), &img).unwrap();
        let got = bootcore_get(&server, "seg").unwrap();
        assert_eq!((got.status, got.body.clone()), (200, img.clone()));
        server.set_release_key([9; 32]);
        assert_eq!(status(bootcore_get(&server, "seg")), 422, "another key");
        server.set_release_key(*key.pk);
        let mut bad = img.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        std::fs::write(dir.join("bootcore.img"), &bad).unwrap();
        assert_eq!(status(bootcore_get(&server, "seg")), 422, "changed payload");
        // [provision.seg] bootcore naming a file outside the project.
        std::fs::write(
            cfg.base.join("dagon.toml"),
            "topology = \"t.yaml\"\n[provision.seg]\nbootcore = \"../outside.img\"\napp = \"a\"\n",
        )
        .unwrap();
        assert_eq!(status(bootcore_get(&server, "seg")), 403);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn key_errors_reach_the_page_without_paths() {
        let r: Result<(), String> =
            Err("refusing to use a secret key stored inside a git work tree: C:/x/k.key".into());
        let e = page_safe(r).unwrap_err();
        assert!(!e.contains("k.key") && !e.contains("C:/"), "{e}");
    }

    #[test]
    fn cancelled_sign_and_release_write_nothing() {
        let (cfg, t) = built_area("cancel");
        let key = test_key();
        let stop = AtomicBool::new(true);
        let seg = ["seg".to_string()];
        assert_eq!(
            sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &stop).unwrap_err(),
            "cancelled"
        );
        assert!(!sign_path(&cfg, "seg", 'a').exists());
        sign_nodes(&cfg, &t, &seg, &key, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let v = dagon_format::Version::new(1, 1, 0);
        assert_eq!(
            release_from(&cfg, &t, Some(v), "r1", &key, &mut |_| {}, &stop).unwrap_err(),
            "cancelled"
        );
        assert!(!cfg.base.join(RELEASE_DIR).join("r1.dgb").exists());
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn device_names_are_not_plain_names() {
        for n in ["CON", "nul", "com1", "LPT3.txt", "aux.dgb"] {
            assert!(!plain(n), "{n}");
        }
        let dir = std::env::temp_dir().join(format!("dagon-webdev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(static_file(&dir, "/con").is_none());
        assert!(static_file(&dir, "/assets/NUL.js").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn static_files_do_not_follow_links_out_of_the_page() {
        let base = std::env::temp_dir().join(format!("dagon-weblink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("web")).unwrap();
        std::fs::create_dir_all(base.join("secret")).unwrap();
        std::fs::write(base.join("secret/k.txt"), "key").unwrap();
        let link = base.join("web").join("out");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(base.join("secret"))
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(base.join("secret"), &link).is_ok();
        assert!(made);
        assert!(static_file(&base.join("web"), "/out/k.txt").is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn no_build_record_means_nothing_to_sign() {
        let (cfg, t) = built_area("unbuilt");
        std::fs::remove_file(cfg.base.join("out/build/seg/build.json")).unwrap();
        let r = sign_nodes(
            &cfg,
            &t,
            &["seg".to_string()],
            &test_key(),
            &mut |_| {},
            &AtomicBool::new(false),
        );
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn names_that_may_become_files() {
        assert!(plain("seg") && plain("r1.1") && plain("hub_2"));
        assert!(!plain("") && !plain("..") && !plain("a/b") && !plain(".hidden"));
        assert!(!plain(&"x".repeat(33)));
    }

    #[test]
    fn static_files_stay_inside_the_page_directory() {
        let dir = std::env::temp_dir().join(format!("dagon-web-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<p>").unwrap();
        std::fs::write(dir.join("assets/a.wasm"), [0, 97]).unwrap();
        assert_eq!(
            static_file(&dir, "/").unwrap().1,
            "text/html; charset=utf-8"
        );
        assert_eq!(
            static_file(&dir, "/assets/a.wasm").unwrap().1,
            "application/wasm"
        );
        for bad in [
            "/../Cargo.toml",
            "/assets/../../x",
            "/a\\b",
            "//etc/passwd",
            "/C:/x",
        ] {
            assert!(static_file(&dir, bad).is_none(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sign_never_writes_or_removes_through_a_junction() {
        if !cfg!(windows) {
            return; // the link cases themselves are in genfs
        }
        let base = std::env::temp_dir().join(format!("dagon-sign-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("user")).unwrap();
        std::fs::create_dir_all(base.join("out")).unwrap();
        for f in ["seg-a.img", "seg-b.img", "seg.json"] {
            std::fs::write(base.join("user").join(f), "mine").unwrap();
        }
        let p = |r: &str| base.join(r).components().collect::<PathBuf>();
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(p(SIGN_DIR))
            .arg(p("user"))
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J");
        // Slot a signed, slot b no longer: without the guard this would
        // replace user/seg-a.img and seg.json and delete user/seg-b.img.
        let files = sign_swaps("seg", "1.1.0", &[('a', b"NEW A".to_vec())], &[]).unwrap();
        let e = commit_files(&base, &files, &mut |from, to| {
            crate::genfs::rename(&base, from, to)
        })
        .unwrap_err();
        assert!(e.contains("out/sign is a link"), "{e}");
        for f in ["seg-a.img", "seg-b.img", "seg.json"] {
            assert_eq!(
                std::fs::read_to_string(base.join("user").join(f)).unwrap(),
                "mine",
                "{f}"
            );
        }
        assert_eq!(std::fs::read_dir(base.join("user")).unwrap().count(), 3);
        let _ = std::fs::remove_dir_all(&base);
    }
}
