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
use crate::config::{load_key, read_file, read_topology, write_file, BuildInputs, Config};
use crate::nodebuild;

const SIGN_DIR: &str = "out/sign";
const RELEASE_DIR: &str = "out/release";
const CONFORM_DIR: &str = "out/conform";

/// Node ids, release names: what may become a file name.
pub(super) fn plain(s: &str) -> bool {
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
        return Response::error(
            403,
            "not in dagon demo (a simulated desk with its own project)",
        );
    }
    let r = match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["hello"]) => Ok(hello(server)),
        ("GET", ["targets"]) => targets(server),
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
        ("GET", ["bootcore", node]) => bootcore_get(server, node),
        ("GET", ["layout"]) => layout_get(server),
        ("PUT", ["layout"]) => layout_put(server, req),
        ("POST", ["conform"]) => start(server, req, "conform", job_conform),
        ("GET", ["conform", node]) => conform_get(server, node),
        ("POST", ["ai", "topology"]) => start(server, req, "ai_topology", job_ai_topology),
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
const DEMO_REFUSED: [&str; 8] = [
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
const DEMO_HIDDEN: [&str; 4] = ["project", "flash", "doctor", "conform"];

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
        .ok_or_else(|| Response::error(404, "no simulated desk (dagon demo)"))
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
const FEATURES: [&str; 12] = [
    "topology_save",
    "ai_topology",
    "build",
    "sign",
    "release",
    "conform",
    "gen",
    "project",
    "bootcore",
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
        let lines = {
            let _lock = s.files.lock().map_err(|_| "lock".to_string())?;
            crate::scaffold::gen_project(&config, &only, force)?
        };
        for l in &lines {
            job.log(l);
        }
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
        "workspace": workspace,
        "features": features,
        "api_key": crate::ai::api_key_present(),
        "key": key.is_some(),
        "key_id": key,
        "release_key": server.release_key().map(|k| to_hex(&k)),
        // The simulated desk of dagon demo: the page shows 模擬 and reaches
        // it through /v1/sim.
        "sim": sim.map(|s| {
            let m = s.medium();
            json!({
                "speed": s.speed,
                "fail": s.fail,
                // The simulated uplink: its kind, frame size and reboot behaviour.
                "uplink": m.kind().name(),
                "max_payload": m.max_payload(),
                "reconnect": if m.reconnect() == dagon_host::Reconnect::Stays { "stays" } else { "reopen" },
            })
        }),
        // The MCU families POST /v1/flash can write.
        "flash_mcus": flash_mcus,
        // null until a project is open (the page shows the start screen).
        "project": server.config_path().map(|_| workspace.clone()),
    }))
}

fn json_ok(v: Value) -> Response {
    Response::json(200, &v)
}

/// Firmware packages with their features, and the board table. Packages are
/// directories with a Cargo.toml: `targets/<pkg>`, one level deeper for the
/// support packages (`targets/mcu/<mcu>`, `targets/boards/<board>`), and the
/// sample apps under `samples/<pkg>`; in a generated project, `nodes/<id>`.
fn targets(server: &Server) -> Res {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let mut out = Vec::new();
    for rel in package_dirs(&cfg.base) {
        let Ok(text) = std::fs::read_to_string(cfg.base.join(&rel).join("Cargo.toml")) else {
            continue;
        };
        let Ok(v) = toml::from_str::<toml::Value>(&text) else {
            continue;
        };
        let features: Vec<String> = v
            .get("features")
            .and_then(|f| f.as_table())
            .map(|t| t.keys().filter(|k| *k != "default").cloned().collect())
            .unwrap_or_default();
        out.push(json!({
            "dir": rel,
            "name": v.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()),
            "features": features,
        }));
    }
    let reg = project_registry(&cfg).map_err(|e| Response::error(500, &e))?;
    let boards: Vec<Value> = reg
        .boards()
        .iter()
        // A project's own words were made safe when its definitions loaded.
        .map(|b| json!({"id": b.id, "product": b.product, "mcu": b.mcu}))
        .collect();
    Ok(json_ok(json!({ "targets": out, "boards": boards })))
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

/// Package directories relative to `base`, `/`-separated and sorted:
/// `targets/*` and `samples/*` with a Cargo.toml, and for a directory under
/// `targets/` without one (a group such as `mcu` or `boards`), its children.
fn package_dirs(base: &std::path::Path) -> Vec<String> {
    fn children(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        v.retain(|p| p.is_dir());
        v.sort();
        v
    }
    let rel = |p: &std::path::Path| {
        p.strip_prefix(base)
            .unwrap_or(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut out = Vec::new();
    for d in children(&base.join("targets")) {
        if d.join("Cargo.toml").is_file() {
            out.push(rel(&d));
        } else {
            out.extend(
                children(&d)
                    .into_iter()
                    .filter(|c| c.join("Cargo.toml").is_file())
                    .map(|c| rel(&c)),
            );
        }
    }
    // A generated project's node crates.
    out.extend(
        children(&base.join("nodes"))
            .into_iter()
            .filter(|c| c.join("Cargo.toml").is_file())
            .map(|c| rel(&c)),
    );
    out.extend(
        children(&base.join("samples"))
            .into_iter()
            .filter(|c| c.join("Cargo.toml").is_file())
            .map(|c| rel(&c)),
    );
    out
}

fn topology_path(server: &Server) -> Result<PathBuf, Response> {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = cfg
        .topology
        .clone()
        .ok_or_else(|| Response::error(409, "dagon.toml names no topology"))?;
    inside(&cfg, &p).map_err(|e| Response::error(403, &e))
}

fn topology_get(server: &Server) -> Res {
    let p = topology_path(server)?;
    let yaml = std::fs::read(&p).map_err(|_| Response::error(404, "no topology file yet"))?;
    Ok(Response::bytes("application/yaml; charset=utf-8", yaml))
}

/// Writes the topology YAML after the same check `dagon topo check` runs.
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
        json!({ "report": report_json, "updatable": true, "skipped": skipped_of(yaml) }),
    ))
}

/// `{id: why}` of the nodes updating skips for now (`Skip::code`).
fn skipped_of(yaml: &str) -> Value {
    let skipped = dagon_topology::parse_str(yaml)
        .map(|t| dagon_topology::updatable(&t).1)
        .unwrap_or_default();
    Value::Object(
        skipped
            .into_iter()
            .map(|(id, why)| (id, Value::from(why.code())))
            .collect(),
    )
}

fn build_get(server: &Server, id: &str) -> Res {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let b = cfg
        .build
        .get(id)
        .ok_or_else(|| Response::error(404, "no [build] table for this node"))?;
    Ok(json_ok(serde_json::to_value(b).unwrap_or(Value::Null)))
}

/// Replaces `[build.<id>]` in dagon.toml, keeping the rest of the file.
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

fn sign_path(cfg: &Config, node: &str, slot: char) -> PathBuf {
    cfg.base.join(SIGN_DIR).join(format!("{node}-{slot}.img"))
}

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
            .ok_or_else(|| format!("{node} was built without a version in [build.{node}]"))?;
        let parsed = version(&ver)?;
        ready.push((node, ver, parsed, elfs));
    }
    // All images are signed in memory first; files change only after
    // every one succeeded, so a bad ELF, a signing error or a cancel
    // leaves the old images and records as they were.
    let reg = crate::defs::registry(cfg)?;
    let mut out = Vec::new();
    let mut signed = Vec::new();
    for (node, ver, parsed, elfs) in ready {
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
            )?;
            log(&format!(
                "{node}: slot {slot} signed, {} bytes, {ver}",
                img.len()
            ));
            out.push(
                json!({"node": node, "slot": slot.to_string(), "bytes": img.len(), "version": ver}),
            );
            images.push((slot, img));
        }
        signed.push((node, ver, images));
    }
    stop_if(cancel)?;
    let mut files = Vec::new();
    for (node, ver, images) in &signed {
        files.extend(sign_swaps(cfg, node, ver, images)?);
    }
    commit_files(&files, &mut |from, to| std::fs::rename(from, to))?;
    Ok(json!({ "images": out }))
}

/// Held for a whole sign (stage and commit).
static SIGN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One file of a sign: its final path and new bytes (None: the old file
/// goes, a slot no longer signed).
type FileSwap = (PathBuf, Option<Vec<u8>>);

/// A node's new images and record, and its old images of slots no longer
/// signed (to remove); the record last.
fn sign_swaps(
    cfg: &Config,
    node: &str,
    ver: &str,
    images: &[(char, Vec<u8>)],
) -> Result<Vec<FileSwap>, String> {
    let mut record = SignRecord {
        version: ver.to_string(),
        images: Vec::new(),
    };
    let mut files = Vec::new();
    for slot in ['a', 'b'] {
        let path = inside(cfg, &sign_path(cfg, node, slot))?;
        let Some((_, img)) = images.iter().find(|(s, _)| *s == slot) else {
            files.push((path, None));
            continue;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        record.images.push((name, sha_hex(img)));
        files.push((path, Some(img.clone())));
    }
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    files.push((
        inside(cfg, &sign_record_path(cfg, node))?,
        Some(text.into_bytes()),
    ));
    Ok(files)
}

/// Replaces `files` all together. Every new file is written as `<name>.new`
/// first (a failure removes them all; nothing changed). Then each old file
/// becomes `<name>.old` and its `.new` takes its name; if a `rename` fails,
/// every step so far is undone (the `.old` files back, no `.new` left) and
/// the error returned. Only when all are in place do the `.old` files go.
fn commit_files(
    files: &[FileSwap],
    rename: &mut dyn FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let new_of = |p: &Path| with_suffix(p, ".new");
    let old_of = |p: &Path| with_suffix(p, ".old");
    let remove_new = || {
        for (p, _) in files {
            let _ = std::fs::remove_file(new_of(p));
        }
    };
    for (path, bytes) in files {
        if let Some(b) = bytes {
            if let Err(e) = write_file(&new_of(path), b) {
                remove_new();
                return Err(e);
            }
        }
    }
    // (path, moved to .old, new in place) for every step taken.
    let mut done: Vec<(&Path, bool, bool)> = Vec::new();
    let mut failed = None;
    for (path, bytes) in files {
        let moved = path.exists();
        if moved {
            if let Err(e) = rename(path, &old_of(path)) {
                failed = Some(format!("cannot replace {}: {e}", path.display()));
                break;
            }
        }
        done.push((path, moved, false));
        if bytes.is_some() {
            if let Err(e) = rename(&new_of(path), path) {
                failed = Some(format!("cannot replace {}: {e}", path.display()));
                break;
            }
            done.last_mut().expect("pushed").2 = true;
        }
    }
    if let Some(e) = failed {
        let mut undo = Vec::new();
        for (path, moved, placed) in done.into_iter().rev() {
            if placed {
                if let Err(x) = std::fs::remove_file(path) {
                    undo.push(format!("{}: {x}", path.display()));
                }
            }
            if moved {
                if let Err(x) = std::fs::rename(old_of(path), path) {
                    undo.push(format!("{} not restored: {x}", path.display()));
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
            let _ = std::fs::remove_file(old_of(path));
        }
    }
    Ok(())
}

/// `path` with `suffix` added to its file name (same folder).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Builds a release from every signed image, all of `version`.
fn job_release(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let name = v["name"].as_str().filter(|n| plain(n) && n.len() <= 16);
    let name = name.ok_or("name must be up to 16 plain characters")?;
    let want = version(v["version"].as_str().ok_or("version is required")?)?;
    let t = read_topology(&cfg, &inside(&cfg, &cfg.topology_path(None)?)?)?;
    let key = serve_key(&cfg)?;
    let cancel = job.cancel_flag();
    release_from(&cfg, &t, want, name, &key, &mut |l| job.log(l), &cancel)
}

/// Images listed by the sign records only (each checked against its
/// recorded SHA-256), all of version `want`.
fn signed_images(cfg: &Config, want: dagon_format::Version) -> Result<Vec<Vec<u8>>, String> {
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
            let h = ImageHeader::parse(raw).map_err(|e| format!("{file}: {e:?}"))?;
            if h.version != want {
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
    want: dagon_format::Version,
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
    let out = inside(cfg, &cfg.base.join(RELEASE_DIR).join(format!("{name}.dgb")))?;
    write_file(&out, &r.bundle)?;
    let sha = to_hex(&dagon_format::sha256(&topo_bin));
    log(&format!(
        "release {name}: {} images, {} bytes",
        images.len(),
        r.bundle.len()
    ));
    Ok(json!({ "release": name, "topology_sha256": sha, "bytes": r.bundle.len() }))
}

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

/// GET /v1/layout: 呼び名 and editor positions (404 before the first save).
fn layout_get(server: &Server) -> Res {
    let p = layout_path(server)?;
    let bytes = crate::paths::read_capped(&p, MAX_LAYOUT)
        .map_err(|_| Response::error(404, "no layout yet"))?
        .ok_or_else(|| Response::error(413, "the layout file is too large"))?;
    Ok(Response::bytes("application/json", bytes))
}

/// PUT /v1/layout: a JSON object, written whole.
fn layout_put(server: &Server, req: &Request) -> Res {
    if req.body.len() > MAX_LAYOUT {
        return Err(Response::error(413, "layout too large"));
    }
    let v: Value = serde_json::from_slice(&req.body)
        .map_err(|_| Response::error(400, "layout must be JSON"))?;
    if !v.is_object() {
        return Err(Response::error(400, "layout must be a JSON object"));
    }
    let p = layout_path(server)?;
    let text = serde_json::to_vec_pretty(&v).map_err(|e| Response::error(500, &e.to_string()))?;
    let _lock = server
        .files
        .lock()
        .map_err(|_| Response::error(500, "lock"))?;
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
    let dir = inside(&cfg, &cfg.base.join(CONFORM_DIR).join(node))?;
    let key = serve_key(&cfg)?;
    job.progress("waiting for the hub", None);
    let report = job.with_device(|job| {
        job.progress("running", None);
        crate::conform::conform_files(node, to, &dir, &dir, &key, None)
    })?;
    let file = crate::conform::json(&report, node);
    Ok(serde_json::from_str(&file).unwrap_or(Value::Null))
}

fn conform_get(server: &Server, node: &str) -> Res {
    if !plain(node) {
        return Err(Response::error(400, "bad node id"));
    }
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let p = cfg
        .base
        .join(CONFORM_DIR)
        .join(node)
        .join(crate::conform::RESULT_FILE);
    let p = inside(&cfg, &p).map_err(|e| Response::error(403, &e))?;
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no result yet"))?;
    Ok(Response::bytes("application/json", bytes))
}

/// Topology from a description; the model's questions go to the page.
fn job_ai_topology(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let description = v["description"]
        .as_str()
        .ok_or("description is required")?
        .to_string();
    let cancel = job.cancel_flag();
    let mut ask = |qs: &[String]| -> Result<Vec<String>, String> {
        qs.iter().enumerate().map(|(i, q)| job.ask(i, q)).collect()
    };
    let reg = project_registry(&cfg)?;
    let (yaml, report) = crate::ai::topology_for(&description, &reg, &mut ask, &cancel)?;
    let report: Value = serde_json::from_str(&report).unwrap_or(Value::Null);
    Ok(json!({ "yaml": yaml, "report": report }))
}

fn job_id(id: &str) -> Result<u64, Response> {
    id.parse().map_err(|_| Response::error(400, "bad job id"))
}

fn job_get(server: &Server, id: &str) -> Res {
    server
        .job_json(job_id(id)?)
        .map(json_ok)
        .ok_or_else(|| Response::error(404, "no such job"))
}

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

fn cancel(server: &Server, id: &str) -> Res {
    let id = job_id(id)?;
    if server.cancel(id) {
        Ok(accepted(id))
    } else {
        Err(Response::error(404, "no such job"))
    }
}

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
mod tests {
    use super::*;

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
    fn package_dirs_cover_support_packages_and_samples() {
        let base = std::env::temp_dir().join(format!("dagon-pkgdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in [
            "targets/f401-app",
            "targets/mcu/esp32s3",
            "targets/boards/m5stopwatch",
            "targets/esp32s3-2ndstage",
            "samples/esp32s3-app",
        ] {
            std::fs::create_dir_all(base.join(d)).unwrap();
            if d != "targets/esp32s3-2ndstage" {
                std::fs::write(
                    base.join(d).join("Cargo.toml"),
                    "[package]
",
                )
                .unwrap();
            }
        }
        assert_eq!(
            package_dirs(&base),
            [
                "targets/boards/m5stopwatch",
                "targets/f401-app",
                "targets/mcu/esp32s3",
                "samples/esp32s3-app",
            ]
        );
        let _ = std::fs::remove_dir_all(&base);
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
        assert_eq!(signed_images(&cfg, v).unwrap().len(), 2);
        let r = release_from(
            &cfg,
            &t,
            v,
            "r1",
            &key,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(r["release"], "r1");
        assert!(signed_images(&cfg, dagon_format::Version::new(1, 0, 0)).is_err());

        // A signed image changed afterwards is refused.
        std::fs::write(sign_path(&cfg, "seg", 'b'), b"changed").unwrap();
        assert!(signed_images(&cfg, v).is_err());

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
            signed_images(&cfg, v).unwrap().len(),
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
            files.extend(sign_swaps(&cfg, node, "1.1.0", imgs).unwrap());
        }
        for fail_at in [1usize, 4, 7, 9] {
            let mut n = 0;
            let r = commit_files(&files, &mut |from, to| {
                n += 1;
                if n == fail_at {
                    return Err(std::io::Error::other("injected"));
                }
                std::fs::rename(from, to)
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
        assert_eq!(signed_images(&cfg, v).unwrap().len(), 4);
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
        assert_eq!(signed_images(&cfg, v).unwrap().len(), 2);
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
        release_from(&cfg, &t, v, "r1", &key, &mut |_| {}, &stop).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        release_from(&cfg, &t, v, "r2", &key, &mut |_| {}, &stop).unwrap();
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
            release_from(&cfg, &t, v, "r1", &key, &mut |_| {}, &stop).unwrap_err(),
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
}
