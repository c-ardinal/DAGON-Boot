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
    std::fs::write(dir.join("web/index.html"), "<!doctype html><p>page").unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = Server::new(
        "0123456789abcdef0123456789abcdef".into(),
        port,
        Some(dir.join("dagon.toml")),
        dir.join("web"),
    );
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
    assert_eq!((st, body.as_str()), (200, "<!doctype html><p>page"));
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
    assert_eq!(srv.job_json(id).unwrap()["state"], "succeeded");
}
