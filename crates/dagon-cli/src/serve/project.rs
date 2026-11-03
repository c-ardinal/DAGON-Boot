//! Choosing the project from the page: the open project, opening or
//! creating one, and a folder listing to pick it from.
//!
//! These are the only endpoints that take a path from the browser. A path
//! must lie under the user's home folder after resolving symlinks and
//! junctions (paths::contained), with no Windows device names, and never in
//! a key folder (`~/.dagon`, the folder of DAGON_SIGNING_KEY): those are
//! neither listed nor opened.
//!
//! Opening or creating a project switches the release key the page
//! verifies with to that project's; the page reads /v1/hello again after
//! a switch. Switching is refused while a job runs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use super::http::{Request, Response};
use super::jobs::Server;
use crate::config::{CONFIG_FILE, KEY_ENV};
use crate::paths::contained;

/// Entries a folder listing returns at most.
const MAX_ENTRIES: usize = 500;

/// The user's home folder (or the server's override), resolved.
pub(super) fn home(server: &Server) -> Result<PathBuf, Response> {
    let h = match server.home.get() {
        Some(h) => h.clone().into_os_string(),
        None => std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| Response::error(500, "no home folder"))?,
    };
    std::fs::canonicalize(h).map_err(|_| Response::error(500, "home folder not found"))
}

/// Folders that hold keys: never listed, opened or written.
pub(super) fn key_dirs(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![home.join(".dagon")];
    if let Some(k) = std::env::var_os(KEY_ENV) {
        let k = PathBuf::from(k);
        if let Some(d) = std::fs::canonicalize(&k)
            .ok()
            .and_then(|f| f.parent().map(Path::to_path_buf))
        {
            out.push(d);
        }
    }
    out.into_iter()
        .map(|d| std::fs::canonicalize(&d).unwrap_or(d))
        .collect()
}

/// Whether `p` is `dir` or inside it; without regard to case on Windows,
/// where `.DAGON` is `.dagon` even before it exists.
pub(super) fn in_dir(p: &Path, dir: &Path) -> bool {
    if cfg!(windows) {
        let low = |x: &Path| PathBuf::from(x.to_string_lossy().to_lowercase());
        low(p).starts_with(low(dir))
    } else {
        p.starts_with(dir)
    }
}

/// `path` from the browser, resolved under `home`; refused in a key folder.
fn under_home(home: &Path, path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Ok(home.to_path_buf());
    }
    let p = contained(home, Path::new(path.trim())).map_err(|e| {
        if e.contains("leaves") {
            "only folders under your home folder can be used".to_string()
        } else {
            e
        }
    })?;
    if key_dirs(home).iter().any(|k| in_dir(&p, k)) {
        return Err("key folders cannot be opened".into());
    }
    Ok(p)
}

fn shown(p: &Path) -> String {
    crate::config::forward_path(p)
}

fn project_json(server: &Server) -> Value {
    match server.config_path() {
        Some(c) => {
            let dir = std::fs::canonicalize(c.parent().unwrap_or(Path::new(".")))
                .unwrap_or_else(|_| c.parent().unwrap_or(Path::new(".")).to_path_buf());
            json!({
                "open": true,
                "path": shown(&dir),
                "name": dir.file_name().map(|n| n.to_string_lossy().into_owned()),
            })
        }
        None => json!({ "open": false }),
    }
}

/// The 構成 files (*.yaml, *.yml) directly in `dir`, by name.
fn yaml_files(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_yaml(n))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.truncate(MAX_ENTRIES);
    out
}

fn is_yaml(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    (low.ends_with(".yaml") || low.ends_with(".yml"))
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !crate::paths::device_name(name)
}

fn path_of(req: &Request) -> Result<String, Response> {
    let v: Value =
        serde_json::from_slice(&req.body).map_err(|_| Response::error(400, "body must be JSON"))?;
    v["path"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| Response::error(400, "path is required"))
}

/// GET /v1/project: the open project and the recent ones.
pub fn get(server: &Server) -> Response {
    let mut v = project_json(server);
    if let Ok(h) = home(server) {
        v["recent"] = super::recent::json(&h);
    }
    Response::json(200, &v)
}

fn busy() -> Response {
    Response::error(409, "a job is running; switch projects when it ends")
}

/// Opens the project of `cfg` with its keys: the page's release key
/// becomes the project's (the secret key stays in this process, unprinted).
fn switch(server: &Server, cfg: PathBuf) -> Result<Response, Response> {
    let c = super::api::config_at(server, Some(&cfg))
        .map_err(|_| Response::error(422, "its dagon.toml does not load"))?;
    let key = super::api::release_key_of(&c).map_err(|e| {
        eprintln!("dagon serve: project key: {e}");
        Response::error(
            422,
            "the project has no usable public key; see the dagon serve terminal",
        )
    })?;
    let dir = c.base.clone();
    if !server.switch_project(cfg, key) {
        return Err(busy());
    }
    if let Ok(h) = home(server) {
        super::recent::add(&h, &dir);
    }
    Ok(Response::json(200, &project_json(server)))
}

/// POST /v1/project/open {path}: a folder with a dagon.toml.
pub fn open(server: &Arc<Server>, req: &Request) -> Response {
    let r = (|| {
        let home = home(server)?;
        let dir = under_home(&home, &path_of(req)?).map_err(|e| Response::error(403, &e))?;
        let cfg = dir.join(CONFIG_FILE);
        if !cfg.is_file() {
            let yaml = yaml_files(&dir);
            if yaml.is_empty() {
                return Err(Response::error(404, "no dagon.toml in that folder"));
            }
            // Only a 構成: the page asks whether to make dagon.toml here
            // (POST /v1/project/init).
            return Err(Response::json(
                428,
                &json!({ "error": "the folder has a topology but no dagon.toml", "yaml": yaml }),
            ));
        }
        switch(server, cfg)
    })();
    r.unwrap_or_else(|e| e)
}

/// POST /v1/project/init {path, yaml?}: dagon.toml in a folder that has a
/// 構成 but none, for that YAML (`yaml`, or the folder's only one); key
/// pair in ~/.dagon/keys/<folder> when none. Then opens it.
pub fn init(server: &Arc<Server>, req: &Request) -> Response {
    let r = (|| {
        let v: Value = serde_json::from_slice(&req.body)
            .map_err(|_| Response::error(400, "body must be JSON"))?;
        let home = home(server)?;
        let raw = v["path"]
            .as_str()
            .ok_or_else(|| Response::error(400, "path is required"))?;
        let dir = under_home(&home, raw).map_err(|e| Response::error(403, &e))?;
        if dir == home {
            return Err(Response::error(
                403,
                "choose a folder inside the home folder",
            ));
        }
        let found = yaml_files(&dir);
        let yaml = match v["yaml"].as_str() {
            Some(y) if found.iter().any(|f| f == y) => y.to_string(),
            Some(_) => return Err(Response::error(404, "no such topology in that folder")),
            None if found.len() == 1 => found[0].clone(),
            None if found.is_empty() => {
                return Err(Response::error(404, "no topology in that folder"))
            }
            None => {
                return Err(Response::error(
                    400,
                    "the folder has several; name one in yaml",
                ))
            }
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Response::error(400, "the folder needs a name"))?;
        if server.busy() {
            return Err(busy());
        }
        let keys = home.join(".dagon").join("keys").join(&name);
        crate::scaffold::init_project(&dir, &name, &yaml, &keys).map_err(|e| {
            let shown = e.replace(&*keys.to_string_lossy(), "<key folder>");
            eprintln!("dagon serve: init project: {shown}");
            Response::error(
                409,
                "dagon.toml could not be created; see the dagon serve terminal",
            )
        })?;
        switch(server, dir.join(CONFIG_FILE))
    })();
    r.unwrap_or_else(|e| e)
}

/// POST /v1/project/new {path, from_sample?}: `dagon new` there (empty
/// 構成 unless from_sample), key pair in ~/.dagon/keys/<name> when none.
pub fn new(server: &Arc<Server>, req: &Request) -> Response {
    let r = (|| {
        let v: Value = serde_json::from_slice(&req.body)
            .map_err(|_| Response::error(400, "body must be JSON"))?;
        let home = home(server)?;
        let raw = v["path"]
            .as_str()
            .ok_or_else(|| Response::error(400, "path is required"))?;
        let dir = under_home(&home, raw).map_err(|e| Response::error(403, &e))?;
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Response::error(400, "the folder needs a name"))?;
        let yaml = if v["from_sample"].as_bool().unwrap_or(false) {
            crate::scaffold::SAMPLE.to_string()
        } else {
            crate::scaffold::empty_topology(&name)
        };
        if server.busy() {
            return Err(busy());
        }
        let keys = home.join(".dagon").join("keys").join(&name);
        crate::scaffold::new_project(&dir, &name, &yaml, &keys).map_err(|e| {
            let shown = e.replace(&*keys.to_string_lossy(), "<key folder>");
            eprintln!("dagon serve: new project: {shown}");
            Response::error(
                409,
                "the project could not be created; see the dagon serve terminal",
            )
        })?;
        switch(server, dir.join(CONFIG_FILE))
    })();
    r.unwrap_or_else(|e| e)
}

/// `%xx` and `+` decoding of one query value.
fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// GET /v1/fs?path=: the folders in a folder under home (default: home),
/// each marked when it holds a dagon.toml. Key folders are left out.
pub fn list(server: &Server, req: &Request) -> Response {
    let r = (|| {
        let query = req.path.split_once('?').map(|q| q.1).unwrap_or("");
        let raw = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("path="))
            .map(decode)
            .unwrap_or(Some(String::new()))
            .ok_or_else(|| Response::error(400, "bad path encoding"))?;
        let home = home(server)?;
        let dir = under_home(&home, &raw).map_err(|e| Response::error(403, &e))?;
        let keys = key_dirs(&home);
        let mut entries: Vec<(String, bool, bool)> = std::fs::read_dir(&dir)
            .map_err(|_| Response::error(404, "not a folder"))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| {
                let p = std::fs::canonicalize(e.path()).ok()?;
                // Links out of home and key folders are not shown.
                (p.starts_with(&home) && !keys.iter().any(|k| in_dir(&p, k))).then(|| {
                    let project = p.join(CONFIG_FILE).is_file();
                    let yaml = !project && !yaml_files(&p).is_empty();
                    (e.file_name().to_string_lossy().into_owned(), project, yaml)
                })
            })
            .collect();
        entries.sort();
        entries.truncate(MAX_ENTRIES);
        let list: Vec<Value> = entries
            .into_iter()
            .map(|(name, project, yaml)| json!({ "name": name, "project": project, "yaml": yaml }))
            .collect();
        let parent = (dir != home).then(|| dir.parent().map(shown)).flatten();
        Ok(Response::json(
            200,
            &json!({ "path": shown(&dir), "parent": parent, "folders": list }),
        ))
    })();
    r.unwrap_or_else(|e| e)
}
