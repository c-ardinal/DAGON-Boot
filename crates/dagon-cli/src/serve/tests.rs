//! `dagon serve` over real sockets on 127.0.0.1 with a temporary work area.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use super::jobs::Server;
use super::ws;

const YAML: &str = include_str!("../../../../topology/sim-pair.yaml");

struct Area {
    dir: PathBuf,
    server: Arc<Server>,
}

impl Drop for Area {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(name: &str) -> Area {
    let dir = std::env::temp_dir().join(format!("dagon-serve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("web")).unwrap();
    std::fs::create_dir_all(dir.join("topology")).unwrap();
    std::fs::write(dir.join("topology/t.yaml"), YAML).unwrap();
    std::fs::write(
        dir.join("dagon.toml"),
        "# work area\ntopology = \"topology/t.yaml\"\n\n[build.seg]\npackage = \"targets/f401-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("web/index.html"),
        "<!doctype html><head></head><p>page",
    )
    .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = Server::new(
        "0123456789abcdef0123456789abcdef".into(),
        port,
        Some(dir.join("dagon.toml")),
        dir.join("web"),
    );
    server.release_key.set([7; 32]).unwrap();
    let s = server.clone();
    std::thread::spawn(move || super::serve(listener, s));
    Area { dir, server }
}

/// Raw HTTP exchange; returns (status, body).
fn http(a: &Area, method: &str, path: &str, extra: &[(&str, &str)], body: &[u8]) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", a.server.port)).unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n",
        body.len()
    );
    let has_host = extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("host"));
    if !has_host {
        head.push_str(&format!("Host: 127.0.0.1:{}\r\n", a.server.port));
    }
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out[9..12].parse().unwrap();
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (status, body)
}

fn auth(a: &Area) -> String {
    format!("Bearer {}", a.server.pass)
}

#[test]
fn host_origin_and_passphrase_are_enforced() {
    let a = start("guard");
    let bearer = auth(&a);
    let ok = [("Authorization", bearer.as_str())];
    assert_eq!(http(&a, "GET", "/v1/hello", &ok, b"").0, 200);
    assert_eq!(http(&a, "GET", "/v1/hello", &[], b"").0, 401);
    let wrong = [("Authorization", "Bearer 00000000000000000000000000000000")];
    assert_eq!(http(&a, "GET", "/v1/hello", &wrong, b"").0, 401);
    let rebind = [
        ("Host", "attacker.example:80"),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(http(&a, "GET", "/v1/hello", &rebind, b"").0, 403);
    let cross = [
        ("Origin", "https://example.com"),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(http(&a, "GET", "/v1/hello", &cross, b"").0, 403);
    let same = format!("http://localhost:{}", a.server.port);
    let own = [
        ("Origin", same.as_str()),
        ("Authorization", bearer.as_str()),
    ];
    assert_eq!(http(&a, "GET", "/v1/hello", &own, b"").0, 200);
    // The page itself needs no passphrase; nothing outside it is served.
    let (st, body) = http(&a, "GET", "/", &[], b"");
    let meta = format!(
        "<meta name=\"dagon-release-key\" content=\"{}\">",
        "07".repeat(32)
    );
    assert_eq!(
        (st, body),
        (200, format!("<!doctype html><head>{meta}</head><p>page"))
    );
    assert_eq!(http(&a, "GET", "/../dagon.toml", &[], b"").0, 404);
}

#[test]
fn hello_reports_keys_without_their_values() {
    let a = start("hello");
    let (st, body) = http(&a, "GET", "/v1/hello", &[("Authorization", &auth(&a))], b"");
    assert_eq!(st, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "build"));
    assert!(v["api_key"].is_boolean() && v["key"].is_boolean());
    let lower = body.to_ascii_lowercase();
    assert!(!lower.contains("sk-") && !lower.contains("secret"));
}

#[test]
fn topology_is_checked_before_it_is_written() {
    let a = start("topo");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    let (st, yaml) = http(&a, "GET", "/v1/topology", &h, b"");
    assert_eq!((st, yaml.as_str()), (200, YAML));
    let (st, _) = http(&a, "PUT", "/v1/topology", &h, b"dagon: 1\nnodes: []\n");
    assert_eq!(st, 422);
    assert_eq!(
        std::fs::read_to_string(a.dir.join("topology/t.yaml")).unwrap(),
        YAML
    );
    let edited = YAML.replace("name: sim-pair", "name: edited");
    let (st, _) = http(&a, "PUT", "/v1/topology", &h, edited.as_bytes());
    assert_eq!(st, 200);
    assert_eq!(
        std::fs::read_to_string(a.dir.join("topology/t.yaml")).unwrap(),
        edited
    );
}

#[test]
fn build_settings_round_trip_and_keep_the_file() {
    let a = start("buildcfg");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    let (st, body) = http(&a, "GET", "/v1/nodes/seg/build", &h, b"");
    assert_eq!(st, 200);
    assert!(body.contains("targets/f401-app"));
    let new = br#"{"package":"targets/f401-app","slot_b_feature":"slot-b","version":"1.1.0"}"#;
    assert_eq!(http(&a, "PUT", "/v1/nodes/hub/build", &h, new).0, 200);
    let bad = br#"{"package":"../../outside"}"#;
    assert_eq!(http(&a, "PUT", "/v1/nodes/x/build", &h, bad).0, 400);
    let text = std::fs::read_to_string(a.dir.join("dagon.toml")).unwrap();
    assert!(text.starts_with("# work area"), "comments kept:\n{text}");
    assert!(text.contains("[build.hub]") && text.contains("slot_b_feature = \"slot-b\""));
    assert!(!text.contains("outside"));
}

#[test]
fn release_and_conform_reads_reject_paths() {
    let a = start("paths");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    assert_eq!(http(&a, "GET", "/v1/release/..%2Fdagon", &h, b"").0, 400);
    assert_eq!(http(&a, "GET", "/v1/release/none", &h, b"").0, 404);
    assert_eq!(http(&a, "GET", "/v1/conform/seg", &h, b"").0, 404);
    assert_eq!(http(&a, "POST", "/v1/ai/port", &h, b"{}").0, 501);
}

/// Opens the event stream and sends `first` as the first message.
fn events(a: &Area, first: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", a.server.port)).unwrap();
    let req = format!(
        "GET /v1/events HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nOrigin: http://127.0.0.1:{p}\r\n\
         Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        p = a.server.port
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("HTTP/1.1 101") && head.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    // Client frames are masked.
    let mask = [1u8, 2, 3, 4];
    let data: Vec<u8> = first
        .bytes()
        .enumerate()
        .map(|(i, c)| c ^ mask[i % 4])
        .collect();
    let mut f = vec![0x81, 0x80 | data.len() as u8];
    f.extend_from_slice(&mask);
    f.extend_from_slice(&data);
    s.write_all(&f).unwrap();
    s
}

/// Next server frame's text (server frames are unmasked).
fn next_text(s: &mut TcpStream) -> (u8, String) {
    let mut h = [0u8; 2];
    s.read_exact(&mut h).unwrap();
    let len = match h[1] {
        126 => {
            let mut b = [0u8; 2];
            s.read_exact(&mut b).unwrap();
            u16::from_be_bytes(b) as usize
        }
        n => n as usize,
    };
    let mut p = vec![0u8; len];
    s.read_exact(&mut p).unwrap();
    (h[0] & 0x0F, String::from_utf8_lossy(&p).into_owned())
}

#[test]
fn event_stream_needs_the_passphrase_and_carries_job_events() {
    let a = start("events");
    let mut bad = events(&a, "wrong");
    assert_eq!(next_text(&mut bad).0, ws::op::CLOSE);

    let mut s = events(&a, &a.server.pass.clone());
    assert_eq!(next_text(&mut s).1, r#"{"type":"ready"}"#);
    // A build of a node without settings fails; its events still flow.
    let b = auth(&a);
    let (st, body) = http(
        &a,
        "POST",
        "/v1/build",
        &[("Authorization", &b)],
        br#"{"nodes":["nope"]}"#,
    );
    assert_eq!(st, 202, "{body}");
    let mut seen = Vec::new();
    while !seen.iter().any(|t: &String| t.contains("\"failed\"")) {
        seen.push(next_text(&mut s).1);
    }
    assert!(seen
        .iter()
        .any(|t| t.contains("job.state") && t.contains("queued")));
    assert!(seen
        .iter()
        .any(|t| t.contains("job.result") && t.contains("no [build.nope]")));
}

#[test]
fn device_jobs_wait_for_the_page_to_release_the_port() {
    let a = start("device");
    let mut s = events(&a, &a.server.pass.clone());
    next_text(&mut s);
    let srv = a.server.clone();
    let id = srv.spawn("probe", |job| {
        job.with_device(|_| Ok(serde_json::json!("ran")))
    });
    let mut seen = Vec::new();
    while !seen.iter().any(|t: &String| t.contains("device.need")) {
        seen.push(next_text(&mut s).1);
    }
    assert!(
        !seen.iter().any(|t| t.contains("\"ran\"")),
        "waits for released"
    );
    let b = auth(&a);
    let (st, _) = http(
        &a,
        "POST",
        "/v1/device",
        &[("Authorization", &b)],
        br#"{"state":"released"}"#,
    );
    assert_eq!(st, 200);
    while !seen.iter().any(|t: &String| t.contains("device.returned")) {
        seen.push(next_text(&mut s).1);
    }
    while !seen.iter().any(|t: &String| t.contains("succeeded")) {
        seen.push(next_text(&mut s).1);
    }
    assert!(seen.iter().any(|t| t.contains("\"ran\"")));
    assert_eq!(srv.job_json(id.unwrap()).unwrap()["state"], "succeeded");
}

#[test]
fn saves_replace_existing_files() {
    // The atomic save renames over an existing file (on Windows too).
    let a = start("replace");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    for name in ["one", "two", "three"] {
        let y = YAML.replace("name: sim-pair", &format!("name: {name}"));
        assert_eq!(http(&a, "PUT", "/v1/topology", &h, y.as_bytes()).0, 200);
        assert_eq!(
            std::fs::read_to_string(a.dir.join("topology/t.yaml")).unwrap(),
            y
        );
        let body = format!(r#"{{"package":"targets/f401-app","version":"1.0.0","bin":"{name}"}}"#);
        assert_eq!(
            http(&a, "PUT", "/v1/nodes/seg/build", &h, body.as_bytes()).0,
            200
        );
        let toml = std::fs::read_to_string(a.dir.join("dagon.toml")).unwrap();
        assert!(toml.contains(&format!("bin = \"{name}\"")), "{toml}");
    }
}

#[test]
fn the_launcher_file_is_new_and_private() {
    let f = std::env::temp_dir().join(format!("dagon-open-t-{}.html", std::process::id()));
    let _ = std::fs::remove_file(&f);
    super::write_private(&f, b"x").unwrap();
    assert!(
        super::write_private(&f, b"y").is_err(),
        "never over an existing file"
    );
    assert_eq!(std::fs::read(&f).unwrap(), b"x");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&f).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let _ = std::fs::remove_file(&f);
}

#[test]
fn the_browser_is_opened_without_the_passphrase_on_its_command_line() {
    let url = "http://127.0.0.1:5/#0123456789abcdef0123456789abcdef";
    let page = super::launcher_page(url);
    assert!(page.contains(url));
    let file = std::env::temp_dir().join("dagon-open-test.html");
    let (prog, args) = super::open_command(&file);
    let line = format!("{prog} {:?}", args);
    assert!(!line.contains("0123456789abcdef"), "{line}");
}

#[test]
fn an_endless_request_line_is_cut_at_the_head_limit() {
    let a = start("headlimit");
    let mut s = TcpStream::connect(("127.0.0.1", a.server.port)).unwrap();
    let line = vec![b'a'; super::http::MAX_HEAD + 4096];
    let _ = s.write_all(b"GET /");
    let _ = s.write_all(&line);
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(
        out.starts_with("HTTP/1.1 400"),
        "{}",
        &out[..out.len().min(40)]
    );
}

#[test]
fn connections_beyond_the_limit_get_503() {
    let a = start("connlimit");
    let idle: Vec<TcpStream> = (0..super::MAX_CONNECTIONS)
        .map(|_| TcpStream::connect(("127.0.0.1", a.server.port)).unwrap())
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(200));
    // The server answers 503 at once, before reading any request.
    let mut extra = TcpStream::connect(("127.0.0.1", a.server.port)).unwrap();
    let mut out = String::new();
    let _ = extra.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 503"), "{out}");
    drop(idle);
}

#[test]
fn configured_paths_outside_the_work_area_are_refused() {
    let a = start("escape");
    std::fs::write(a.dir.join("dagon.toml"), "topology = \"../outside.yaml\"\n").unwrap();
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    assert_eq!(http(&a, "GET", "/v1/topology", &h, b"").0, 403);
    assert_eq!(http(&a, "PUT", "/v1/topology", &h, YAML.as_bytes()).0, 403);
    assert!(!a.dir.parent().unwrap().join("outside.yaml").exists());
}

#[test]
fn concurrent_build_setting_writes_all_land() {
    let a = Arc::new(start("concurrent"));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let a = a.clone();
            std::thread::spawn(move || {
                let b = auth(&a);
                let body = format!(r#"{{"package":"targets/n{i}","version":"1.0.{i}"}}"#);
                let path = format!("/v1/nodes/n{i}/build");
                http(&a, "PUT", &path, &[("Authorization", &b)], body.as_bytes()).0
            })
        })
        .collect();
    assert!(threads.into_iter().all(|t| t.join().unwrap() == 200));
    let text = std::fs::read_to_string(a.dir.join("dagon.toml")).unwrap();
    for i in 0..8 {
        assert!(
            text.contains(&format!("[build.n{i}]")),
            "n{i} lost:\n{text}"
        );
    }
    assert!(text.starts_with("# work area"));
}

#[test]
fn the_page_carries_the_release_key_given_at_start() {
    let a = start("relkey");
    let (_, body) = http(&a, "GET", "/v1/hello", &[("Authorization", &auth(&a))], b"");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["release_key"], "07".repeat(32));
    // Non-HTML files are served as they are.
    std::fs::write(a.dir.join("web/app.js"), "x()").unwrap();
    assert_eq!(http(&a, "GET", "/app.js", &[], b"").1, "x()");
}

#[test]
fn without_a_release_key_no_page_is_served() {
    let dir = std::env::temp_dir().join(format!("dagon-serve-nokey-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<head></head>").unwrap();
    let server = Server::new("p".repeat(32), 1, None, dir.clone());
    assert!(super::api::page_file(&server, "/").is_none());
    server.release_key.set([1; 32]).unwrap();
    assert!(super::api::page_file(&server, "/").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn targets_lists_the_generated_node_crates() {
    let a = start("nodes");
    std::fs::create_dir_all(a.dir.join("nodes/seg")).unwrap();
    std::fs::write(
        a.dir.join("nodes/seg/Cargo.toml"),
        "[package]
name = \"seg\"
[features]
slot-b = []
",
    )
    .unwrap();
    let (st, body) = http(
        &a,
        "GET",
        "/v1/targets",
        &[("Authorization", &auth(&a))],
        b"",
    );
    assert_eq!(st, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let t = &v["targets"][0];
    assert_eq!(
        (t["dir"].as_str(), t["name"].as_str()),
        (Some("nodes/seg"), Some("seg"))
    );
    assert_eq!(t["features"][0], "slot-b");
}

#[test]
fn the_page_built_into_dagon_is_served_with_the_release_key() {
    assert!(
        super::api::page_embedded(),
        "crates/dagon-cli/page is built into dagon"
    );
    let server = Server::new("p".repeat(32), 1, None, std::path::PathBuf::new());
    server.release_key.set([9; 32]).unwrap();
    let (html, ct) = super::api::page_file(&server, "/").unwrap();
    assert!(ct.starts_with("text/html"));
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains(&format!("content=\"{}\"", "09".repeat(32))));
    assert!(super::api::page_file(&server, "/fonts/OFL-Doto.txt").is_some());
    assert!(
        super::api::page_file(&server, "/SOURCE").is_none(),
        "build stamp not served"
    );
}
