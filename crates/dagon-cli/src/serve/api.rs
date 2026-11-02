//! The `/v1` API. Each endpoint runs a fixed CLI function; paths come from
//! dagon.toml only, never from the browser.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dagon_format::header::{image_kind, slot_binding, ImageHeader, HEADER_SIZE};
use dagon_sign::to_hex;
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
fn plain(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !s.starts_with('.')
}

fn config(server: &Server) -> Result<Config, String> {
    let p = server
        .config
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    Config::load(p.as_deref())
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
    let r = match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["hello"]) => Ok(hello(server)),
        ("GET", ["targets"]) => targets(server),
        ("GET", ["topology"]) => topology_get(server),
        ("PUT", ["topology"]) => topology_put(server, req),
        ("GET", ["nodes", id, "build"]) => build_get(server, id),
        ("PUT", ["nodes", id, "build"]) => build_put(server, id, req),
        ("POST", ["build"]) => start(server, req, "build", job_build),
        ("POST", ["sign"]) => start(server, req, "sign", job_sign),
        ("POST", ["release"]) => start(server, req, "release", job_release),
        ("GET", ["release", id]) => release_get(server, id),
        ("POST", ["conform"]) => start(server, req, "conform", job_conform),
        ("GET", ["conform", node]) => conform_get(server, node),
        ("POST", ["ai", "topology"]) => start(server, req, "ai_topology", job_ai_topology),
        ("POST", ["ai", "port"]) => Ok(Response::error(501, "porting is not available here")),
        ("GET", ["jobs", id]) => job_get(server, id),
        ("POST", ["jobs", id, "answer"]) => answer(server, id, req),
        ("POST", ["jobs", id, "cancel"]) => cancel(server, id),
        ("POST", ["device"]) => device(server, req),
        _ => Ok(Response::error(404, "no such endpoint")),
    };
    r.unwrap_or_else(|e| e)
}

type Res = Result<Response, Response>;
type Work = fn(&mut Job, Config, Value) -> Result<Value, String>;

fn start(server: &Arc<Server>, req: &Request, kind: &str, work: Work) -> Res {
    let v = body_json(req)?;
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    Ok(accepted(server.spawn(kind, move |job| work(job, cfg, v))))
}

/// Versions, work area, abilities; whether keys exist (never the keys).
fn hello(server: &Server) -> Response {
    let cfg = config(server).ok();
    let workspace = cfg
        .as_ref()
        .and_then(|c| std::fs::canonicalize(&c.base).ok())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let key = load_key(None)
        .ok()
        .map(|k| to_hex(&dagon_format::key_id(&k.pk)));
    json_ok(json!({
        "cli": env!("CARGO_PKG_VERSION"),
        "workspace": workspace,
        "features": ["topology_save", "ai_topology", "build", "sign", "release", "conform"],
        "api_key": crate::ai::api_key_present(),
        "key": key.is_some(),
        "key_id": key,
    }))
}

fn json_ok(v: Value) -> Response {
    Response::json(200, &v)
}

/// Firmware packages under targets/ with their features, and the board table.
fn targets(server: &Server) -> Res {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    let mut out = Vec::new();
    if let Ok(dirs) = std::fs::read_dir(cfg.base.join("targets")) {
        let mut dirs: Vec<PathBuf> = dirs.filter_map(|d| d.ok().map(|d| d.path())).collect();
        dirs.sort();
        for d in dirs {
            let Ok(text) = std::fs::read_to_string(d.join("Cargo.toml")) else {
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
                "dir": format!("targets/{}", d.file_name().unwrap_or_default().to_string_lossy()),
                "name": v.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()),
                "features": features,
            }));
        }
    }
    let boards: Vec<Value> = dagon_topology::boards::BOARDS
        .iter()
        .map(|b| json!({"id": b.id, "product": b.product, "mcu": b.mcu}))
        .collect();
    Ok(json_ok(json!({ "targets": out, "boards": boards })))
}

fn topology_path(server: &Server) -> Result<PathBuf, Response> {
    let cfg = config(server).map_err(|e| Response::error(500, &e))?;
    cfg.topology
        .ok_or_else(|| Response::error(409, "dagon.toml names no topology"))
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
    let report = dagon_topology::check_str(yaml);
    let report_json: Value = serde_json::from_str(&report.to_json()).unwrap_or(Value::Null);
    if !report.is_pushable() {
        return Err(Response::json(422, &json!({ "report": report_json })));
    }
    write_file(&p, yaml.as_bytes()).map_err(|e| Response::error(500, &e))?;
    Ok(json_ok(json!({ "report": report_json })))
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
        .config
        .clone()
        .ok_or_else(|| Response::error(409, "no dagon.toml in the work area"))?;
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
    write_file(&path, doc.to_string().as_bytes()).map_err(|e| Response::error(500, &e))?;
    Ok(json_ok(serde_json::to_value(&b).unwrap_or(Value::Null)))
}

fn job_build(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let nodes = nodes_of(&v).map_err(|_| "nodes must be node ids".to_string())?;
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

/// Signs the built applications of `nodes` with the CLI's key.
fn job_sign(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let nodes = nodes_of(&v).map_err(|_| "nodes must be node ids".to_string())?;
    let t = read_topology(&cfg.topology_path(None)?)?;
    let key = load_key(None)?;
    let mut out = Vec::new();
    for node in &nodes {
        let b = cfg
            .build
            .get(node)
            .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
        let ver = b
            .version
            .as_deref()
            .ok_or_else(|| format!("[build.{node}] has no version"))?;
        let opts = SignOpts {
            version: version(ver)?,
            release_seq: 1,
            min_bootcore: Default::default(),
            proto: 1,
            bits: None,
        };
        for slot in ['a', 'b'] {
            let elf = nodebuild::elf_path(&cfg, node, slot);
            if !elf.exists() {
                continue;
            }
            let binding = if slot == 'a' {
                slot_binding::A
            } else {
                slot_binding::B
            };
            let img = image_for_node(
                &t,
                node,
                image_kind::APP,
                binding,
                &read_file(&elf)?,
                Some(&opts),
                &key,
            )?;
            write_file(&sign_path(&cfg, node, slot), &img)?;
            job.log(&format!(
                "{node}: slot {slot} signed, {} bytes, {ver}",
                img.len()
            ));
            out.push(
                json!({"node": node, "slot": slot.to_string(), "bytes": img.len(), "version": ver}),
            );
        }
    }
    Ok(json!({ "images": out }))
}

/// Builds a release from every signed image, all of `version`.
fn job_release(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let name = v["name"].as_str().filter(|n| plain(n) && n.len() <= 16);
    let name = name.ok_or("name must be up to 16 plain characters")?;
    let want = version(v["version"].as_str().ok_or("version is required")?)?;
    let t = read_topology(&cfg.topology_path(None)?)?;
    let topo_bin = dagon_topology::compile(&t).map_err(|e| e.to_string())?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(cfg.base.join(SIGN_DIR))
        .map_err(|_| "nothing signed yet".to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "img"))
        .collect();
    files.sort();
    let mut images = Vec::new();
    for f in &files {
        let img = read_file(f)?;
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(|| format!("{}: not an image", f.display()))?;
        let h = ImageHeader::parse(raw).map_err(|e| format!("{}: {e:?}", f.display()))?;
        if h.version != want {
            return Err(format!("{} is {}, not {want}", f.display(), h.version));
        }
        images.push(img);
    }
    let key = load_key(None)?;
    let r = dagon_sign::release::build(&t, &topo_bin, &images, name, &key)
        .map_err(|e| e.to_string())?;
    let out = cfg.base.join(RELEASE_DIR).join(format!("{name}.dgb"));
    write_file(&out, &r.bundle)?;
    let sha = to_hex(&dagon_format::sha256(&topo_bin));
    job.log(&format!(
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
    let p = cfg.base.join(RELEASE_DIR).join(format!("{id}.dgb"));
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no such release"))?;
    Ok(Response::bytes("application/octet-stream", bytes))
}

/// The 12 conformance tests on `node` through the hub, after the page
/// hands over the port.
fn job_conform(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    let node = v["node"]
        .as_str()
        .filter(|n| plain(n))
        .ok_or("node is required")?;
    let t = read_topology(&cfg.topology_path(None)?)?;
    let to = crate::conform::node_target(&t, node)?;
    let dir = cfg.base.join(CONFORM_DIR).join(node);
    let key = load_key(None)?;
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
    let bytes = std::fs::read(p).map_err(|_| Response::error(404, "no result yet"))?;
    Ok(Response::bytes("application/json", bytes))
}

/// Topology from a description; the model's questions go to the page.
fn job_ai_topology(job: &mut Job, _cfg: Config, v: Value) -> Result<Value, String> {
    let description = v["description"]
        .as_str()
        .ok_or("description is required")?
        .to_string();
    let mut ask = |qs: &[String]| -> Result<Vec<String>, String> {
        qs.iter().enumerate().map(|(i, q)| job.ask(i, q)).collect()
    };
    let (yaml, report) = crate::ai::topology_for(&description, &mut ask)?;
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

/// Static file under `web` for `path` (the built page), or None.
pub fn static_file(web: &Path, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = path.split(['?', '#']).next()?.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let safe = rel
        .split('/')
        .all(|c| !c.is_empty() && c != ".." && c != "." && !c.contains('\\') && !c.contains(':'));
    if !safe {
        return None;
    }
    let bytes = std::fs::read(web.join(rel)).ok()?;
    let ext = rel.rsplit('.').next().unwrap_or("");
    let ct = match ext {
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
    };
    Some((bytes, ct))
}

#[cfg(test)]
mod tests {
    use super::*;

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
