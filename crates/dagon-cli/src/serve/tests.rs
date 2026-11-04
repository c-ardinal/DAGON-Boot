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
    server.set_release_key([7; 32]);
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
fn definitions_are_served_behind_the_same_guards_with_the_projects_boards() {
    let a = start("defs");
    let toml = std::fs::read_to_string(a.dir.join("dagon.toml")).unwrap();
    std::fs::write(
        a.dir.join("dagon.toml"),
        format!(
            "{toml}\n[board.desk-pico]\nproduct = \"Desk Pico\"\nmcu = \"rp2040\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\
             default_link = {{ kind = \"i2c\", addr = 0x44, control = \"full\" }}\n"
        ),
    )
    .unwrap();
    let bearer = auth(&a);
    assert_eq!(http(&a, "GET", "/v1/definitions", &[], b"").0, 401);
    let rebind = [("Host", "attacker.example:80"), ("Authorization", &bearer)];
    assert_eq!(http(&a, "GET", "/v1/definitions", &rebind, b"").0, 403);
    let cross = [
        ("Origin", "https://example.com"),
        ("Authorization", &bearer),
    ];
    assert_eq!(http(&a, "GET", "/v1/definitions", &cross, b"").0, 403);
    let (st, body) = http(
        &a,
        "GET",
        "/v1/definitions",
        &[("Authorization", &bearer)],
        b"",
    );
    assert_eq!(st, 200, "{body}");
    let reg = dagon_topology::defs::Registry::from_json(&body).unwrap();
    assert_eq!(reg.board("desk-pico").unwrap().product, "Desk Pico");
    assert!(reg.mcu("esp32s3").is_some());
    // Nothing local leaves: no project or DAGON source path, no command.
    let plain = body.replace("\\\\", "\\");
    let src = std::path::Path::new(crate::dagonsrc::BUILT_FROM);
    for local in [
        a.dir.display().to_string(),
        src.display().to_string(),
        src.canonicalize().unwrap().display().to_string(),
        "dagon.toml".into(),
        "targets/".into(),
        "rustup".into(),
    ] {
        assert!(!plain.contains(&local), "{local} in {body}");
    }

    // A broken project definition is a 500 without its path.
    std::fs::write(
        a.dir.join("dagon.toml"),
        format!("{toml}\n[board.bad]\nmcu = \"nope\"\n"),
    )
    .unwrap();
    let (st, body) = http(
        &a,
        "GET",
        "/v1/definitions",
        &[("Authorization", &bearer)],
        b"",
    );
    assert_eq!(st, 500);
    assert!(!body.contains(&*a.dir.to_string_lossy()), "{body}");
    assert!(body.contains(super::api::DEFS_ERROR), "{body}");
    // hello still answers, but offers nothing to flash: the builtin set
    // never stands in for a project's definitions.
    let (st, hello) = http(&a, "GET", "/v1/hello", &[("Authorization", &bearer)], b"");
    assert_eq!(st, 200);
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(v["flash_mcus"], serde_json::json!([]));
    // A 構成 is not checked against the builtin set instead either.
    let yaml = std::fs::read(a.dir.join("topology/t.yaml")).unwrap();
    let (st, body) = http(
        &a,
        "PUT",
        "/v1/topology",
        &[("Authorization", &bearer)],
        &yaml,
    );
    assert_eq!(st, 500);
    assert!(body.contains(super::api::DEFS_ERROR), "{body}");
    // Every job of the project stops before it starts, with the same words.
    for (path, body) in [
        ("/v1/sign", r#"{"nodes":["seg"]}"#),
        ("/v1/release", r#"{"version":"1.0.0"}"#),
        ("/v1/flash", r#"{"node":"seg"}"#),
        ("/v1/build", r#"{}"#),
        ("/v1/gen", r#"{}"#),
    ] {
        let h = [
            ("Authorization", bearer.as_str()),
            ("Content-Type", "application/json"),
        ];
        let (st, started) = http(&a, "POST", path, &h, body.as_bytes());
        assert_eq!(st, 202, "{path}: {started}");
        let v: serde_json::Value = serde_json::from_str(&started).unwrap();
        let job_path = format!("/v1/jobs/{}", v["job"]);
        let t = std::time::Instant::now();
        let job = loop {
            let (_, j) = http(&a, "GET", &job_path, &[("Authorization", &bearer)], b"");
            if j.contains(r#""failed""#) || t.elapsed() > std::time::Duration::from_secs(5) {
                break j;
            }
            std::thread::yield_now();
        };
        assert!(
            job.contains(r#""failed""#) && job.contains(super::api::DEFS_ERROR),
            "{path}: {job}"
        );
        assert!(
            !job.contains("nope"),
            "{path}: the reason stays in the terminal: {job}"
        );
    }
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
fn without_a_release_key_the_page_carries_none() {
    let dir = std::env::temp_dir().join(format!("dagon-serve-nokey-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<head></head>").unwrap();
    let server = Server::new("p".repeat(32), 1, None, dir.clone());
    let (page, _) = super::api::page_file(&server, "/").unwrap();
    assert!(!String::from_utf8_lossy(&page).contains("dagon-release-key"));
    server.set_release_key([1; 32]);
    let (page, _) = super::api::page_file(&server, "/").unwrap();
    assert!(String::from_utf8_lossy(&page).contains(&"01".repeat(32)));
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
fn a_projects_own_words_reach_the_page_only_as_plain_names() {
    let a = start("plain");
    let toml = std::fs::read_to_string(a.dir.join("dagon.toml")).unwrap();
    let secret = "C:/Users/me/sk-ant-FAKE:TOKEN";
    std::fs::write(
        a.dir.join("dagon.toml"),
        format!(
            "{toml}\n[board.desk-pico]\nproduct = \"{secret}\"\nmcu = \"rp2040\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\
             header = [{{ pin = \"GPIO4\", label = \"{secret}\" }}]\n\
             default_link = {{ kind = \"i2c\", addr = 0x44, control = \"full\" }}\n\
             [board.desk-two]\nproduct = \"Desk Two\"\nmcu = \"rp2040\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\
             default_link = {{ kind = \"i2c\", addr = 0x45, control = \"full\" }}\n"
        ),
    )
    .unwrap();
    let ok = [("Authorization", auth(&a))];
    let ok: Vec<(&str, &str)> = ok.iter().map(|(k, v)| (*k, v.as_str())).collect();
    for route in ["/v1/targets", "/v1/definitions", "/v1/hello"] {
        let (st, body) = http(&a, "GET", route, &ok, b"");
        assert_eq!(st, 200, "{route}: {body}");
        assert!(
            !body.contains("sk-ant") && !body.contains("Users"),
            "{route}: {body}"
        );
    }
    let (_, body) = http(&a, "GET", "/v1/targets", &ok, b"");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let product = |id: &str| {
        v["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == id)
            .map(|b| b["product"].clone())
    };
    assert_eq!(product("desk-pico"), Some("project-text".into()));
    assert_eq!(product("desk-two"), Some("Desk Two".into()));
    // Builtin boards keep their own product names.
    assert_eq!(
        product("waveshare-rp2040-touch-lcd-1.28"),
        Some("Waveshare RP2040-Touch-LCD-1.28".into())
    );
}

#[test]
fn the_page_built_into_dagon_is_served_with_the_release_key() {
    assert!(
        super::api::page_embedded(),
        "crates/dagon-cli/page is built into dagon"
    );
    let server = Server::new("p".repeat(32), 1, None, std::path::PathBuf::new());
    server.set_release_key([9; 32]);
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

/// sim-pair with the F401 as a second 親機: only "not updatable yet".
fn two_hubs() -> String {
    YAML.replacen("role: node", "role: coordinator", 1)
}

#[test]
fn a_shape_not_updatable_yet_is_saved_and_its_updatable_part_compiled() {
    let a = start("v23");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    let y = two_hubs();
    let report = dagon_topology::check_str(&y);
    assert_eq!(report.errors().count(), 0, "{}", report.to_json());
    assert!(report.issues.iter().any(|i| i.rule == "V-23"));
    let (st, body) = http(&a, "PUT", "/v1/topology", &h, y.as_bytes());
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["updatable"], true);
    assert_eq!(v["skipped"], serde_json::json!({ "seg": "second-hub" }));
    assert_eq!(
        std::fs::read_to_string(a.dir.join("topology/t.yaml")).unwrap(),
        y
    );
    // The second 親機 is skipped: the compiled topology is the 親機 alone.
    let t = dagon_topology::parse_str(&y).unwrap();
    let (sub, _) = dagon_topology::updatable(&t);
    assert_eq!(
        dagon_topology::compile(&t).unwrap(),
        dagon_topology::compile(&sub).unwrap()
    );
    assert_eq!(v["reach"], serde_json::json!(["hub"]));
    assert_eq!(compiled_tags(&y), reach_tags(&t, &v));

    // Reached whole: reach is every node, in the compiled table's order.
    let (st, body) = http(&a, "PUT", "/v1/topology", &h, YAML.as_bytes());
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["reach"], serde_json::json!(["hub", "seg"]));
    assert_eq!(v["skipped"], serde_json::json!({}));
    let t = dagon_topology::parse_str(YAML).unwrap();
    assert_eq!(compiled_tags(YAML), reach_tags(&t, &v));
}

/// The target tags of the compiled topology's nodes, in its order.
fn compiled_tags(yaml: &str) -> Vec<[u8; 16]> {
    let bin = dagon_topology::compile_str(yaml).unwrap();
    let view = dagon_format::topology::TopologyView::parse(&bin).unwrap();
    (0..view.node_count())
        .map(|i| view.node(i).target_tag)
        .collect()
}

/// The target tags of the nodes PUT /v1/topology answered as `reach`, in its order.
fn reach_tags(t: &dagon_topology::Topology, v: &serde_json::Value) -> Vec<[u8; 16]> {
    v["reach"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let n = t.node(id.as_str().unwrap()).unwrap();
            dagon_format::header::target_tag(&n.tag)
        })
        .collect()
}

/// A server whose home is a fresh folder, with a key folder in it.
fn with_home(name: &str) -> (Area, PathBuf) {
    let a = start(name);
    let home = a.dir.join("home");
    std::fs::create_dir_all(home.join(".dagon").join("keys")).unwrap();
    std::fs::create_dir_all(home.join("proj")).unwrap();
    project_with_key(&home, "proj");
    std::fs::create_dir_all(home.join("proj/topology")).unwrap();
    std::fs::write(home.join("proj/topology/t.yaml"), YAML).unwrap();
    a.server.home.set(home.clone()).unwrap();
    (a, home)
}

/// home/<name>/dagon.toml with its own key pair in home/.dagon/keys/<name>;
/// returns the public key.
fn project_with_key(home: &std::path::Path, name: &str) -> [u8; 32] {
    let k = dagon_sign::generate_to_dir(&home.join(".dagon/keys").join(name)).unwrap();
    let pubkey = k.public_path.to_string_lossy().replace(char::from(92), "/");
    std::fs::create_dir_all(home.join(name)).unwrap();
    std::fs::write(
        home.join(name).join("dagon.toml"),
        format!("topology = \"topology/t.yaml\"\npubkey = {pubkey:?}\n"),
    )
    .unwrap();
    k.public_key
}

fn link(link: &std::path::Path, to: &std::path::Path) {
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(to)
        .output()
        .is_ok_and(|o| o.status.success());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(to, link).is_ok();
    assert!(made, "could not create the test link");
}

fn post(a: &Area, path: &str, body: serde_json::Value) -> (u16, String) {
    let b = auth(a);
    let h = [("Authorization", b.as_str())];
    http(a, "POST", path, &h, body.to_string().as_bytes())
}

fn get(a: &Area, path: &str) -> (u16, String) {
    let b = auth(a);
    http(a, "GET", path, &[("Authorization", b.as_str())], b"")
}

#[test]
fn projects_open_and_list_under_home_only() {
    let (a, home) = with_home("proj-open");
    let (st, body) = get(&a, "/v1/fs");
    assert_eq!(st, 200, "{body}");
    assert!(
        body.contains("\"proj\"") && !body.contains(".dagon"),
        "{body}"
    );
    let (st, body) = post(
        &a,
        "/v1/project/open",
        serde_json::json!({ "path": "proj" }),
    );
    assert_eq!(st, 200, "{body}");
    assert!(body.contains("\"name\":\"proj\""), "{body}");
    assert_eq!(get(&a, "/v1/topology"), (200, YAML.to_string()));
    let (st, body) = get(&a, "/v1/project");
    assert!(st == 200 && body.contains("\"open\":true"), "{body}");
    // A folder without dagon.toml.
    std::fs::create_dir_all(home.join("empty")).unwrap();
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "empty" })
        )
        .0,
        404
    );
}

#[test]
fn project_paths_outside_home_are_refused() {
    let (a, home) = with_home("proj-refuse");
    let outside = a.dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::copy(a.dir.join("dagon.toml"), outside.join("dagon.toml")).unwrap();
    link(&home.join("escape"), &outside);
    let abs = outside.to_string_lossy().into_owned();
    for path in [
        "..",
        "../outside",
        abs.as_str(),
        "escape",
        "proj/../../outside",
    ] {
        let (st, _) = post(&a, "/v1/project/open", serde_json::json!({ "path": path }));
        assert_eq!(st, 403, "open {path}");
        let (st, _) = post(
            &a,
            "/v1/project/new",
            serde_json::json!({ "path": format!("{path}/n") }),
        );
        assert_eq!(st, 403, "new {path}");
        let q = path.replace(char::from(92), "%5C").replace(' ', "%20");
        assert_eq!(get(&a, &format!("/v1/fs?path={q}")).0, 403, "fs {path}");
    }
    // The junction out of home is not listed either.
    let (_, body) = get(&a, "/v1/fs");
    assert!(!body.contains("escape"), "{body}");
    assert!(!outside.join("n").exists());
}

#[test]
fn device_names_and_key_folders_are_refused() {
    let (a, _home) = with_home("proj-dev");
    for path in ["NUL", "proj/COM1", "con.txt", "LPT9/x"] {
        let (st, _) = post(&a, "/v1/project/new", serde_json::json!({ "path": path }));
        assert_eq!(st, 403, "new {path}");
        assert_eq!(get(&a, &format!("/v1/fs?path={path}")).0, 403, "fs {path}");
    }
    for path in [
        ".dagon",
        ".dagon/keys",
        ".dagon/keys/x",
        "proj/../.dagon/keys",
    ] {
        let (st, _) = post(&a, "/v1/project/open", serde_json::json!({ "path": path }));
        assert_eq!(st, 403, "open {path}");
        assert_eq!(get(&a, &format!("/v1/fs?path={path}")).0, 403, "fs {path}");
        let (st, _) = post(&a, "/v1/project/new", serde_json::json!({ "path": path }));
        assert_eq!(st, 403, "new {path}");
    }
}

#[test]
fn a_new_project_is_created_under_home_and_opened() {
    let (a, home) = with_home("proj-new");
    let (st, body) = post(
        &a,
        "/v1/project/new",
        serde_json::json!({ "path": "fresh" }),
    );
    assert_eq!(st, 200, "{body}");
    assert!(home.join("fresh/dagon.toml").is_file());
    assert!(home.join(".dagon/keys/fresh").is_dir());
    let (st, yaml) = get(&a, "/v1/topology");
    assert!(st == 200 && yaml.contains("nodes: []"), "{yaml}");
    let (st, _) = post(
        &a,
        "/v1/project/new",
        serde_json::json!({ "path": "proj", "from_sample": true }),
    );
    assert_eq!(st, 409, "an existing folder is not overwritten");
    let (st, _) = post(
        &a,
        "/v1/project/new",
        serde_json::json!({ "path": "sample", "from_sample": true }),
    );
    assert_eq!(st, 200);
    assert!(std::fs::read_to_string(home.join("sample/topology.yaml"))
        .unwrap()
        .contains("desk-demo"));
}

#[test]
fn gen_is_a_job_and_listed_in_hello() {
    let (a, _home) = with_home("gen");
    let (_, hello) = get(&a, "/v1/hello");
    assert!(hello.contains("\"gen\""), "{hello}");
    assert!(
        hello.contains("\"flash\"") && hello.contains("\"doctor\""),
        "{hello}"
    );
    // The page offers 書き込む only for these.
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(
        v["flash_mcus"],
        serde_json::json!(["rp2040", "stm32f401re", "esp32s3"])
    );
    assert_eq!(
        post(&a, "/v1/project/new", serde_json::json!({ "path": "g" })).0,
        200
    );
    let (st, body) = post(&a, "/v1/gen", serde_json::json!({}));
    assert_eq!(st, 202, "{body}");
    assert_eq!(
        post(&a, "/v1/gen", serde_json::json!({ "nodes": ["../x"] })).0,
        400
    );
    // Jobs now; a flash without a node fails in its job, writing nothing.
    assert_eq!(post(&a, "/v1/flash", serde_json::json!({})).0, 202);
    assert_eq!(post(&a, "/v1/doctor", serde_json::json!({})).0, 202);
}

#[cfg(windows)]
#[test]
fn key_folders_are_refused_in_any_case() {
    let (a, home) = with_home("proj-case");
    std::fs::remove_dir_all(home.join(".dagon")).unwrap();
    let (st, _) = post(
        &a,
        "/v1/project/new",
        serde_json::json!({ "path": ".DAGON/keys/x" }),
    );
    assert_eq!(st, 403);
    assert!(!home.join(".dagon").exists());
}

fn hex(k: &[u8; 32]) -> String {
    dagon_sign::to_hex(k)
}

#[test]
fn keys_follow_the_opened_project() {
    let (a, home) = with_home("proj-keys");
    let other = project_with_key(&home, "other");
    std::fs::create_dir_all(home.join("other/topology")).unwrap();
    std::fs::write(home.join("other/topology/t.yaml"), YAML).unwrap();
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "other" })
        )
        .0,
        200
    );
    let (_, hello) = get(&a, "/v1/hello");
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(v["release_key"], hex(&other));
    assert_eq!(v["key"], true, "the secret beside the pubkey signs");
    assert_eq!(
        v["key_id"],
        dagon_sign::to_hex(&dagon_format::key_id(&other))
    );
    assert!(!hello.contains(".dagon"), "no key paths: {hello}");
    let (_, page) = get(&a, "/");
    assert!(page.contains(&hex(&other)), "{page}");
    // A new project gets its own pair, and the page its public half.
    assert_eq!(
        post(
            &a,
            "/v1/project/new",
            serde_json::json!({ "path": "fresh" })
        )
        .0,
        200
    );
    let fresh =
        dagon_sign::load_public_key(&home.join(".dagon/keys/fresh").join(dagon_sign::PUBLIC_FILE))
            .unwrap();
    let (_, hello) = get(&a, "/v1/hello");
    assert!(
        hello.contains(&hex(&fresh)) && !hello.contains(&hex(&other)),
        "{hello}"
    );
}

#[test]
fn a_running_job_blocks_a_project_switch() {
    let (a, home) = with_home("proj-busy");
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    a.server
        .spawn("test", move |_| {
            let _ = rx.recv();
            Ok(serde_json::Value::Null)
        })
        .unwrap();
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "proj" })
        )
        .0,
        409
    );
    assert_eq!(
        post(&a, "/v1/project/new", serde_json::json!({ "path": "n" })).0,
        409
    );
    assert!(!home.join("n").exists());
    assert_eq!(a.server.release_key(), Some([7; 32]));
    tx.send(()).unwrap();
    let t = std::time::Instant::now();
    while a.server.busy() && t.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::yield_now();
    }
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "proj" })
        )
        .0,
        200
    );
}

#[test]
fn a_signing_key_that_does_not_match_the_pubkey_is_not_used() {
    let (a, home) = with_home("proj-mismatch");
    let stray = dagon_sign::generate_to_dir(&home.join(".dagon/keys/stray")).unwrap();
    let toml = std::fs::read_to_string(home.join("proj/dagon.toml")).unwrap();
    let sk = stray
        .secret_path
        .to_string_lossy()
        .replace(char::from(92), "/");
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "proj" })
        )
        .0,
        200
    );
    std::fs::write(
        home.join("proj/dagon.toml"),
        format!("{toml}signing_key = {sk:?}\n"),
    )
    .unwrap();
    let (st, _) = post(
        &a,
        "/v1/project/open",
        serde_json::json!({ "path": "proj" }),
    );
    assert_eq!(st, 422, "not opened with a mismatched pair");
    let (_, hello) = get(&a, "/v1/hello");
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(v["key"], false, "{hello}");
    let (st, body) = post(&a, "/v1/sign", serde_json::json!({ "nodes": ["seg"] }));
    assert_eq!(st, 202, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["job"].clone();
    let t = std::time::Instant::now();
    let job = loop {
        let (_, j) = get(&a, &format!("/v1/jobs/{id}"));
        if j.contains("\"failed\"") || t.elapsed() > std::time::Duration::from_secs(5) {
            break j;
        }
        std::thread::yield_now();
    };
    assert!(
        job.contains("\"failed\"") && job.contains("signing key is not available"),
        "{job}"
    );
    assert!(!job.contains("stray"), "no key paths to the page: {job}");
}

#[test]
fn a_project_without_any_key_is_not_opened() {
    if std::env::var_os(crate::config::KEY_ENV).is_some() {
        return;
    }
    let (a, home) = with_home("proj-nokey");
    std::fs::create_dir_all(home.join("bare")).unwrap();
    std::fs::write(home.join("bare/dagon.toml"), "topology = \"t.yaml\"\n").unwrap();
    let (st, body) = post(
        &a,
        "/v1/project/open",
        serde_json::json!({ "path": "bare" }),
    );
    assert_eq!(st, 422, "{body}");
    assert_eq!(a.server.release_key(), Some([7; 32]));
}

/// home/<name>/dagon.toml whose pubkey is `pubkey`.
fn project_with_pubkey(home: &std::path::Path, name: &str, pubkey: &std::path::Path) {
    let pk = pubkey.to_string_lossy().replace(char::from(92), "/");
    std::fs::create_dir_all(home.join(name)).unwrap();
    std::fs::write(
        home.join(name).join("dagon.toml"),
        format!("topology = \"t.yaml\"\npubkey = {pk:?}\n"),
    )
    .unwrap();
}

#[test]
fn a_pubkey_naming_a_secret_key_is_refused_and_nothing_is_published() {
    let (a, home) = with_home("proj-secret");
    let victim = dagon_sign::generate_to_dir(&home.join(".dagon/keys/victim")).unwrap();
    let seed = std::fs::read_to_string(&victim.secret_path).unwrap();
    let seed = seed.trim().to_string();
    // The secret file itself; a .pub copy of it beside it; a .pub outside
    // the project and the key folders.
    let copy = home.join(".dagon/keys/victim/copy.pub");
    std::fs::write(&copy, &seed).unwrap();
    let outside = a.dir.join("elsewhere.pub");
    std::fs::copy(&victim.public_path, &outside).unwrap();
    for (name, pk) in [
        ("p1", victim.secret_path.clone()),
        ("p2", copy.clone()),
        ("p3", outside.clone()),
    ] {
        project_with_pubkey(&home, name, &pk);
        let (st, body) = post(&a, "/v1/project/open", serde_json::json!({ "path": name }));
        assert_eq!(st, 422, "{name}: {body}");
        assert!(!body.contains(&seed) && !body.contains("victim"), "{body}");
        assert_eq!(a.server.release_key(), Some([7; 32]), "{name}");
    }
    let (_, hello) = get(&a, "/v1/hello");
    assert!(!hello
        .to_ascii_lowercase()
        .contains(&seed.to_ascii_lowercase()));
    let (_, page) = get(&a, "/");
    assert!(!page
        .to_ascii_lowercase()
        .contains(&seed.to_ascii_lowercase()));
}

/// home/proj with `pubkey` and `signing_key` as given (paths relative to
/// the project, or absolute).
fn project_keys(home: &std::path::Path, name: &str, pubkey: &str, signing: &std::path::Path) {
    let sk = signing.to_string_lossy().replace(char::from(92), "/");
    std::fs::create_dir_all(home.join(name)).unwrap();
    std::fs::write(
        home.join(name).join("dagon.toml"),
        format!("topology = \"t.yaml\"\npubkey = {pubkey:?}\nsigning_key = {sk:?}\n"),
    )
    .unwrap();
}

#[test]
fn a_secret_key_under_any_name_is_never_a_pubkey() {
    let (a, home) = with_home("proj-leak");
    let mine = dagon_sign::generate_to_dir(&home.join(".dagon/keys/mine")).unwrap();
    let seed = std::fs::read_to_string(&mine.secret_path)
        .unwrap()
        .trim()
        .to_string();
    // A renamed copy of the signing key inside the project.
    std::fs::create_dir_all(home.join("p1")).unwrap();
    std::fs::write(home.join("p1/leak.pub"), &seed).unwrap();
    project_keys(&home, "p1", "leak.pub", &mine.secret_path);
    // A copy in another allowed folder.
    let other = home.join(".dagon/keys/other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("x.pub"), &seed).unwrap();
    let x = other
        .join("x.pub")
        .to_string_lossy()
        .replace(char::from(92), "/");
    project_keys(&home, "p2", &x, &mine.secret_path);
    // Some other valid public key while the signing key is known.
    let stranger = dagon_sign::generate_to_dir(&home.join(".dagon/keys/stranger")).unwrap();
    let sp = stranger
        .public_path
        .to_string_lossy()
        .replace(char::from(92), "/");
    project_keys(&home, "p3", &sp, &mine.secret_path);
    for name in ["p1", "p2", "p3"] {
        let (st, body) = post(&a, "/v1/project/open", serde_json::json!({ "path": name }));
        assert_eq!(st, 422, "{name}: {body}");
        assert_eq!(a.server.release_key(), Some([7; 32]), "{name}");
    }
    let (_, hello) = get(&a, "/v1/hello");
    let (_, page) = get(&a, "/");
    for text in [hello, page] {
        assert!(!text
            .to_ascii_lowercase()
            .contains(&seed.to_ascii_lowercase()));
    }
    // The matching pair is accepted.
    let mp = mine
        .public_path
        .to_string_lossy()
        .replace(char::from(92), "/");
    project_keys(&home, "p4", &mp, &mine.secret_path);
    let (st, body) = post(&a, "/v1/project/open", serde_json::json!({ "path": "p4" }));
    assert_eq!(st, 200, "{body}");
    assert_eq!(a.server.release_key(), Some(mine.public_key));
}

#[test]
fn every_start_pubkey_source_goes_through_the_same_checks() {
    let (a, home) = with_home("start-key");
    let mine = dagon_sign::generate_to_dir(&home.join(".dagon/keys/mine")).unwrap();
    let seed = std::fs::read_to_string(&mine.secret_path).unwrap();
    std::fs::create_dir_all(home.join("p")).unwrap();
    std::fs::write(home.join("p/leak.pub"), seed.trim()).unwrap();
    let leak = home.join("p/leak.pub").to_string_lossy().into_owned();
    let good = mine.public_path.to_string_lossy().into_owned();
    project_keys(
        &home,
        "p",
        &leak.replace(char::from(92), "/"),
        &mine.secret_path,
    );
    let config = home.join("p/dagon.toml");
    let start = |args: &[&str]| {
        let o = crate::config::Opts::parse(args, &["pubkey", "key"]).unwrap();
        super::start_key(&o, &a.server, Some(&config))
    };
    // --pubkey naming a renamed secret, and the same in dagon.toml.
    assert!(start(&["--pubkey", &leak]).is_err());
    assert!(start(&[]).is_err());
    // --pubkey with the matching public key.
    assert_eq!(start(&["--pubkey", &good]).unwrap(), Some(mine.public_key));
}

/// serve started with no project and no key (a fresh folder): home is the
/// area's own folder.
fn bare(name: &str) -> (Area, PathBuf) {
    let dir = std::env::temp_dir().join(format!("dagon-serve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(dir.join("web")).unwrap();
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
        None,
        dir.join("web"),
    );
    server.home.set(home.clone()).unwrap();
    let s = server.clone();
    std::thread::spawn(move || super::serve(listener, s));
    (Area { dir, server }, home)
}

#[test]
fn serve_without_a_project_starts_keyless_and_takes_the_new_projects_key() {
    let (a, home) = bare("bare");
    let (st, hello) = get(&a, "/v1/hello");
    assert_eq!(st, 200);
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert!(
        v["project"].is_null() && v["release_key"].is_null(),
        "{hello}"
    );
    let (st, page) = http(&a, "GET", "/", &[], b"");
    assert!(st == 200 && !page.contains("dagon-release-key"), "{page}");
    let (st, p) = get(&a, "/v1/project");
    assert!(st == 200 && p.contains("\"open\":false"), "{p}");
    assert_eq!(
        post(
            &a,
            "/v1/project/new",
            serde_json::json!({ "path": "fresh" })
        )
        .0,
        200
    );
    let fresh =
        dagon_sign::load_public_key(&home.join(".dagon/keys/fresh").join(dagon_sign::PUBLIC_FILE))
            .unwrap();
    let (_, hello) = get(&a, "/v1/hello");
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(v["release_key"], hex(&fresh));
    assert_eq!(v["project"], "fresh");
    let (_, page) = http(&a, "GET", "/", &[], b"");
    assert!(page.contains(&hex(&fresh)));
}

#[test]
fn start_key_is_none_without_any_source() {
    if std::env::var_os(crate::config::KEY_ENV).is_some() {
        return;
    }
    let (a, _home) = bare("bare-key");
    let o = crate::config::Opts::parse(&[], &["pubkey", "key"]).unwrap();
    let none = super::start_key(&o, &a.server, None);
    // Without dagon.toml in the current folder (the test's) there is none.
    if !std::path::Path::new(crate::config::CONFIG_FILE).exists() {
        assert_eq!(none.unwrap(), None);
    }
}

#[test]
fn recent_projects_are_kept_newest_first_and_only_under_home() {
    let (a, home) = bare("recent");
    for i in 0..12 {
        let (st, _) = post(
            &a,
            "/v1/project/new",
            serde_json::json!({ "path": format!("p{i}") }),
        );
        assert_eq!(st, 200);
    }
    assert_eq!(
        post(&a, "/v1/project/open", serde_json::json!({ "path": "p3" })).0,
        200
    );
    let (_, body) = get(&a, "/v1/project");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = v["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), super::recent::MAX_RECENT);
    assert_eq!(&names[..3], ["p3", "p11", "p10"]);
    // Entries outside home, in a key folder or without dagon.toml are not shown.
    let outside = a.dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("dagon.toml"), "").unwrap();
    std::fs::write(home.join(".dagon/keys/p1/dagon.toml"), "").unwrap();
    let list = serde_json::json!([
        outside.to_string_lossy(),
        home.join(".dagon/keys/p1").to_string_lossy(),
        home.join("nothing").to_string_lossy(),
        home.join("p5").to_string_lossy(),
    ]);
    std::fs::write(home.join(".dagon/recent.json"), list.to_string()).unwrap();
    let (_, body) = get(&a, "/v1/project");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let recent = v["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 1, "{body}");
    assert_eq!(recent[0]["name"], "p5");
    assert!(!body.contains(".dagon"));
    // An oversized recent.json is ignored, not read whole.
    let mut big = String::from("[");
    big.push_str(&format!("{:?}", home.join("p5").to_string_lossy()));
    big.push_str(&" ".repeat(300 * 1024));
    big.push(']');
    std::fs::write(home.join(".dagon/recent.json"), big).unwrap();
    let (_, body) = get(&a, "/v1/project");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["recent"], serde_json::json!([]), "{body}");
}

#[test]
fn a_folder_with_only_a_topology_is_offered_dagon_toml() {
    let (a, home) = bare("yamlonly");
    std::fs::create_dir_all(home.join("y")).unwrap();
    std::fs::write(home.join("y/desk.yaml"), YAML).unwrap();
    let (_, fs) = get(&a, "/v1/fs");
    assert!(
        fs.contains("\"name\":\"y\",\"project\":false,\"yaml\":true"),
        "{fs}"
    );
    let (st, body) = post(&a, "/v1/project/open", serde_json::json!({ "path": "y" }));
    assert_eq!(st, 428, "{body}");
    assert!(body.contains("desk.yaml"), "{body}");
    // Refused: outside home, the home itself, a key folder, another file.
    assert_eq!(
        post(&a, "/v1/project/init", serde_json::json!({ "path": ".." })).0,
        403
    );
    assert_eq!(
        post(&a, "/v1/project/init", serde_json::json!({ "path": "" })).0,
        403
    );
    assert_eq!(
        post(
            &a,
            "/v1/project/init",
            serde_json::json!({ "path": ".dagon" })
        )
        .0,
        403
    );
    assert_eq!(
        post(
            &a,
            "/v1/project/init",
            serde_json::json!({ "path": "y", "yaml": "../t.yaml" })
        )
        .0,
        404
    );
    assert!(!home.join("y/dagon.toml").exists());
    let (st, body) = post(&a, "/v1/project/init", serde_json::json!({ "path": "y" }));
    assert_eq!(st, 200, "{body}");
    assert!(std::fs::read_to_string(home.join("y/dagon.toml"))
        .unwrap()
        .contains("topology = \"desk.yaml\""));
    assert_eq!(get(&a, "/v1/topology"), (200, YAML.to_string()));
    let (_, hello) = get(&a, "/v1/hello");
    assert!(!serde_json::from_str::<serde_json::Value>(&hello).unwrap()["release_key"].is_null());
    // Once there, init is refused and open works.
    assert_eq!(
        post(&a, "/v1/project/init", serde_json::json!({ "path": "y" })).0,
        409
    );
    assert_eq!(
        post(&a, "/v1/project/open", serde_json::json!({ "path": "y" })).0,
        200
    );
    // Nothing at all: 404.
    std::fs::create_dir_all(home.join("empty")).unwrap();
    assert_eq!(
        post(
            &a,
            "/v1/project/open",
            serde_json::json!({ "path": "empty" })
        )
        .0,
        404
    );
}

#[test]
fn the_layout_is_kept_beside_the_topology() {
    let a = start("layout");
    let b = auth(&a);
    let h = [("Authorization", b.as_str())];
    assert_eq!(http(&a, "GET", "/v1/layout", &h, b"").0, 404);
    let layout = serde_json::json!({ "names": { "seg": "7セグ" }, "positions": { "seg": { "x": 1, "y": 2 } } });
    let (st, _) = http(&a, "PUT", "/v1/layout", &h, layout.to_string().as_bytes());
    assert_eq!(st, 200);
    let (st, body) = http(&a, "GET", "/v1/layout", &h, b"");
    assert_eq!(st, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        layout
    );
    assert!(a.dir.join("topology/t.layout.json").is_file());
    assert_eq!(http(&a, "PUT", "/v1/layout", &h, b"[1]").0, 400);
    assert_eq!(http(&a, "PUT", "/v1/layout", &h, b"not json").0, 400);
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(300 * 1024));
    assert_eq!(http(&a, "PUT", "/v1/layout", &h, big.as_bytes()).0, 413);
    // A layout file grown too large on disk is not read whole or served.
    std::fs::write(a.dir.join("topology/t.layout.json"), &big).unwrap();
    assert_eq!(http(&a, "GET", "/v1/layout", &h, b"").0, 413);
    // A topology path out of the project: the layout beside it is refused.
    std::fs::write(a.dir.join("dagon.toml"), "topology = \"../outside.yaml\"\n").unwrap();
    assert_eq!(http(&a, "GET", "/v1/layout", &h, b"").0, 403);
    assert_eq!(http(&a, "PUT", "/v1/layout", &h, b"{}").0, 403);
}

#[test]
fn a_page_that_stops_reading_events_is_dropped() {
    use std::sync::atomic::Ordering;
    let a = start("stall");
    // Signed in, then never reads: the socket buffers fill up.
    let s = events(&a, &a.server.pass.clone());
    let t = std::time::Instant::now();
    while a.server.pages.load(Ordering::Relaxed) == 0 && t.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(a.server.pages.load(Ordering::Relaxed), 1);
    let big = "x".repeat(60 * 1024);
    let t = std::time::Instant::now();
    while a.server.pages.load(Ordering::Relaxed) == 1 && t.elapsed().as_secs() < 30 {
        for _ in 0..64 {
            a.server
                .emit(serde_json::json!({ "type": "job.log", "lines": [big] }));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        a.server.pages.load(Ordering::Relaxed),
        0,
        "the event thread let go of the stalled page"
    );
    drop(s);
}

/// A serve with dagon demo's simulated desk.
fn demo_area(name: &str) -> Area {
    let a = start(name);
    let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([5; 32]));
    let hub = crate::demo::desk(&key, None, 1).unwrap();
    let _ = a
        .server
        .sim
        .set(Arc::new(super::SimDevice::new(hub, 10.0, None)));
    a
}

#[test]
fn the_sim_link_exists_only_in_a_demo_and_keeps_the_guards() {
    let a = start("nosim");
    let b = auth(&a);
    let ok = [("Authorization", b.as_str())];
    assert_eq!(http(&a, "GET", "/v1/sim/read?wait=1", &ok, b"").0, 404);
    assert_eq!(http(&a, "POST", "/v1/sim/write", &ok, b"x").0, 404);

    let d = demo_area("sim-guard");
    let b = auth(&d);
    let ok = [("Authorization", b.as_str())];
    assert_eq!(http(&d, "GET", "/v1/sim/read?wait=1", &ok, b"").0, 200);
    assert_eq!(http(&d, "GET", "/v1/sim/read?wait=1", &[], b"").0, 401);
    assert_eq!(http(&d, "POST", "/v1/sim/write", &[], b"x").0, 401);
    let rebind = [
        ("Host", "attacker.example:80"),
        ("Authorization", b.as_str()),
    ];
    assert_eq!(http(&d, "POST", "/v1/sim/write", &rebind, b"x").0, 403);
    let cross = [
        ("Origin", "https://example.com"),
        ("Authorization", b.as_str()),
    ];
    assert_eq!(http(&d, "GET", "/v1/sim/read?wait=1", &cross, b"").0, 403);
}

#[test]
fn a_demo_refuses_other_projects_and_real_hardware() {
    let d = demo_area("demo-locks");
    let b = auth(&d);
    let ok = [("Authorization", b.as_str())];
    for (m, path) in [
        ("POST", "/v1/project/open"),
        ("POST", "/v1/project/new"),
        ("POST", "/v1/project/init"),
        ("GET", "/v1/fs"),
        ("GET", "/v1/fs?path=x"),
        ("POST", "/v1/flash"),
        ("POST", "/v1/doctor"),
        ("POST", "/v1/conform"),
        ("GET", "/v1/conform/seg"),
        ("POST", "/v1/device"),
    ] {
        let (st, _) = http(&d, m, path, &ok, br#"{"path":"x","node":"seg"}"#);
        assert_eq!(st, 403, "{m} {path}");
    }
    let (_, hello) = http(&d, "GET", "/v1/hello", &ok, b"");
    let v: serde_json::Value = serde_json::from_str(&hello).unwrap();
    let features: Vec<&str> = v["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    for hidden in ["project", "flash", "doctor", "conform"] {
        assert!(!features.contains(&hidden), "{hidden} in {features:?}");
    }
    assert!(
        features.contains(&"sim") && features.contains(&"build"),
        "{features:?}"
    );
    // The demo's own project stays readable.
    assert_eq!(http(&d, "GET", "/v1/project", &ok, b"").0, 200);
    assert_eq!(http(&d, "GET", "/v1/topology", &ok, b"").0, 200);
}

#[test]
fn the_demo_uplink_follows_the_saved_topology() {
    let d = demo_area("demo-uplink");
    let b = auth(&d);
    let h = [("Authorization", b.as_str())];
    let sim = |d: &Area| {
        let (_, hello) = http(d, "GET", "/v1/hello", &h, b"");
        serde_json::from_str::<serde_json::Value>(&hello).unwrap()["sim"].clone()
    };
    assert_eq!(sim(&d)["uplink"], "usb-cdc");
    for (kind, max, reconnect) in [("uart", 1024, "stays"), ("wifi-tcp", 1088, "reopen")] {
        let y = YAML.replacen(
            "    tag: hub-rp2040-lcd
",
            &format!(
                "    tag: hub-rp2040-lcd
    uplink: {{ kind: {kind}, host: browser }}
"
            ),
            1,
        );
        assert_eq!(
            http(&d, "PUT", "/v1/topology", &h, y.as_bytes()).0,
            200,
            "{kind}"
        );
        let s = sim(&d);
        assert_eq!(
            (
                s["uplink"].as_str(),
                s["max_payload"].as_u64(),
                s["reconnect"].as_str()
            ),
            (Some(kind), Some(max), Some(reconnect))
        );
    }
    // An uplink the demo cannot simulate (a kind other than USB, UART or
    // Wi-Fi, or a custom device driving the 親機) is refused: the saved
    // 構成 and the simulated medium both stay on Wi-Fi.
    for (uplink, kind, host) in [
        ("{ kind: ble, host: browser }", "ble", "browser"),
        ("{ kind: uart, host: embedded }", "uart", "embedded"),
    ] {
        let y = YAML.replacen(
            "    tag: hub-rp2040-lcd
",
            &format!(
                "    tag: hub-rp2040-lcd
    uplink: {uplink}
"
            ),
            1,
        );
        let (status, body) = http(&d, "PUT", "/v1/topology", &h, y.as_bytes());
        assert_eq!(status, 422, "{uplink}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["demo_uplink"]["kind"], kind, "{body}");
        assert_eq!(v["demo_uplink"]["host"], host, "{body}");
        assert_eq!(sim(&d)["uplink"], "wifi-tcp");
        let (_, saved) = http(&d, "GET", "/v1/topology", &h, b"");
        assert!(saved.contains("kind: wifi-tcp"), "{saved}");
    }
    // Without an uplink the 親機 is on USB again.
    assert_eq!(http(&d, "PUT", "/v1/topology", &h, YAML.as_bytes()).0, 200);
    assert_eq!(sim(&d)["uplink"], "usb-cdc");
}

/// What the page receives of job `started` (its log lines and its error),
/// decoded from the events: every string as the page reads it.
fn job_text(rx: &std::sync::mpsc::Receiver<String>, started: &str) -> String {
    let id = serde_json::from_str::<serde_json::Value>(started).unwrap()["job"].clone();
    let mut text = Vec::new();
    loop {
        let ev = rx
            .recv_timeout(std::time::Duration::from_secs(300))
            .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&ev).unwrap();
        if ev["job"] != id {
            continue;
        }
        if ev["type"] == "job.log" {
            for l in ev["lines"].as_array().unwrap() {
                text.push(l.as_str().unwrap().to_string());
            }
        }
        if ev["type"] == "job.result" {
            if let Some(e) = ev["data"]["error"].as_str() {
                text.push(e.to_string());
            }
            break;
        }
    }
    text.join("\n")
}

/// Every spelling of the work area's folder.
fn local_dirs(a: &Area) -> Vec<String> {
    let canon = std::fs::canonicalize(&a.dir).unwrap();
    vec![
        a.dir.display().to_string(),
        crate::config::plain_path(canon).display().to_string(),
    ]
}

/// A job's first step can fail (here: dagon.toml does not parse); the
/// error already hides the project's folder, for every kind of job.
#[test]
fn a_job_hides_the_project_folder_before_anything_can_fail() {
    let a = start("pagebad");
    std::fs::write(a.dir.join("dagon.toml"), "this is = not [toml\n").unwrap();
    let rx = a.server.subscribe();
    for (path, body) in [
        ("/v1/build", serde_json::json!({ "nodes": ["seg"] })),
        ("/v1/gen", serde_json::json!({})),
    ] {
        let (st, started) = post(&a, path, body);
        assert_eq!(st, 202, "{path}: {started}");
        let text = job_text(&rx, &started);
        assert!(text.contains("<project>"), "{path}: {text}");
        for local in local_dirs(&a) {
            assert!(
                !text.contains(&local),
                "{path}: {local} reached the page: {text}"
            );
        }
    }
}

/// /v1/build runs cargo: its lines and error name the app by its package,
/// never a folder of this machine.
#[test]
fn build_output_reaches_the_page_without_local_folders() {
    let a = start("pagebuild");
    let app = a.dir.join("private/token/app");
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"fake-app\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    )
    .unwrap();
    // Does not compile: cargo names the package folder in its lines.
    std::fs::write(
        app.join("src/main.rs"),
        "fn main() { let x: u8 = \"no\"; }\n",
    )
    .unwrap();
    std::fs::write(
        a.dir.join("dagon.toml"),
        "topology = \"topology/t.yaml\"\n[build.seg]\npackage = \"private/token/app\"\n",
    )
    .unwrap();
    let rx = a.server.subscribe();
    let (st, started) = post(&a, "/v1/build", serde_json::json!({ "nodes": ["seg"] }));
    assert_eq!(st, 202, "{started}");
    let all = job_text(&rx, &started);
    assert!(all.contains("Compiling fake-app"), "{all}");
    assert!(all.contains("fake-app (the seg app)"), "{all}");
    assert!(
        all.contains("cargo build failed in fake-app (the seg app)"),
        "{all}"
    );
    for local in ["private".to_string(), "token".to_string()]
        .into_iter()
        .chain(local_dirs(&a))
    {
        assert!(!all.contains(&local), "{local} reached the page: {all}");
    }
}

#[test]
fn dagon_links_with_consent_once_and_ai_routes_wait_for_it() {
    let (a, home) = with_home("agent-link");
    let b = auth(&a);
    let ok = [("Authorization", b.as_str())];
    let agent =
        |a: &Area| -> serde_json::Value { serde_json::from_str(&get(a, "/v1/agent").1).unwrap() };
    let v = agent(&a);
    assert_eq!(v["linked"], false);
    assert_eq!(v["consent"], serde_json::Value::Null);
    assert_eq!(v["provider"]["id"], "anthropic");
    assert!(v["available"].is_boolean());
    assert_eq!(v["sends"].as_array().unwrap().len(), 3);
    assert_eq!(v["never_sends"].as_array().unwrap().len(), 3);

    for path in ["/v1/ai/topology", "/v1/ai/port"] {
        let (st, body) = post(&a, path, serde_json::json!({ "description": "a hub" }));
        assert_eq!(st, 409, "{path}");
        assert!(body.contains("\"code\":\"AGENT_NOT_LINKED\""), "{body}");
    }
    // Anything but an explicit yes for the one provider records nothing.
    for body in [
        serde_json::json!({ "provider": "anthropic" }),
        serde_json::json!({ "provider": "anthropic", "consent": "true" }),
        serde_json::json!({ "provider": "other", "consent": true }),
    ] {
        assert_eq!(post(&a, "/v1/agent/link", body).0, 400);
    }
    assert!(!home.join(".dagon/agent.json").exists());
    let yes = br#"{"provider":"anthropic","consent":true}"#;
    assert_eq!(http(&a, "POST", "/v1/agent/link", &[], yes).0, 401);
    let cross = [
        ("Origin", "https://example.com"),
        ("Authorization", b.as_str()),
    ];
    assert_eq!(http(&a, "POST", "/v1/agent/link", &cross, yes).0, 403);

    let (st, body) = http(&a, "POST", "/v1/agent/link", &ok, yes);
    assert_eq!(st, 200, "{body}");
    let v = agent(&a);
    assert_eq!(v["linked"], true);
    assert_eq!(v["consent"]["provider"], "anthropic");
    assert!(v["consent"]["at"].as_str().unwrap().ends_with('Z'));
    let shown = home.to_string_lossy().replace('\\', "/");
    assert!(!body.replace("\\\\", "/").contains(&shown) && !body.contains("agent.json"));
    // Linked: the AI route starts its job (no description: it stops before
    // any model call, so no request leaves even with a key set).
    let (st, _) = post(&a, "/v1/ai/topology", serde_json::json!({}));
    assert_eq!(st, 202);
    assert_eq!(post(&a, "/v1/ai/port", serde_json::json!({})).0, 501);

    assert_eq!(http(&a, "DELETE", "/v1/agent/link", &ok, b"").0, 200);
    assert_eq!(agent(&a)["linked"], false);
    assert!(!home.join(".dagon/agent.json").exists());
    assert_eq!(post(&a, "/v1/ai/topology", serde_json::json!({})).0, 409);
}

#[test]
fn a_demo_does_not_link_or_unlink() {
    let d = demo_area("demo-agent");
    let b = auth(&d);
    let ok = [("Authorization", b.as_str())];
    let yes = br#"{"provider":"anthropic","consent":true}"#;
    assert_eq!(http(&d, "POST", "/v1/agent/link", &ok, yes).0, 403);
    assert_eq!(http(&d, "DELETE", "/v1/agent/link", &ok, b"").0, 403);
}
