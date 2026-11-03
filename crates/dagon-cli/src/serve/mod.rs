//! `dagon serve`: serves the browser page and its API on 127.0.0.1 only.
//!
//! - A 128-bit passphrase is made per start and handed to the page in the
//!   URL fragment (never sent to the server in a URL, so never logged). API
//!   requests carry it as `Authorization: Bearer`; the event WebSocket
//!   sends it as its first message. Anything else gets 401.
//! - Host must be `127.0.0.1:<port>` or `localhost:<port>` (DNS
//!   rebinding), and an Origin, when present, must be this server's own; the
//!   event stream requires it. No CORS headers are ever sent.
//! - Endpoints run fixed CLI functions; file paths come from dagon.toml.
//! - The signing key and the Claude API key stay in this process: the API
//!   reports only whether they exist and the key id.

mod api;
mod devjobs;
mod http;
mod jobs;
mod project;
mod recent;
mod sim;

pub use sim::SimDevice;
mod ws;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::config::{Config, Opts, CONFIG_FILE};
use http::{Request, Response};
use jobs::Server;

const PASS_BYTES: usize = 16;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// The page must send the passphrase this soon after opening the stream.
const WS_AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const WS_POLL: Duration = Duration::from_millis(500);
/// A page that takes no event for this long is dropped (it reconnects and
/// reads state again), so a stalled tab never holds an event thread.
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_WEB: &str = "web/dist";
/// Connections handled at once; more get 503 straight away.
pub const MAX_CONNECTIONS: usize = 32;
/// Pages on the event stream at once.
pub const MAX_PAGES: usize = 8;
/// How long the browser-launch file stays before it is deleted.
const LAUNCHER_LIFETIME: Duration = Duration::from_secs(60);

/// A fresh passphrase: 128 random bits as hex.
fn passphrase() -> Result<String, String> {
    let mut b = [0u8; PASS_BYTES];
    getrandom::fill(&mut b).map_err(|e| format!("no random source: {e}"))?;
    Ok(dagon_sign::to_hex(&b))
}

/// `dagon serve [--port <n>] [--web <dir>] [--no-open] [--config <file>]`
/// The key the page verifies with at start, through the same checks as an
/// opened project: --pubkey, pubkey in dagon.toml (also only from the
/// project or a key folder), else the public half of the signing key
/// (--key or DAGON_SIGNING_KEY). None when none of them is given: serve
/// starts on the start screen and the key comes with the project.
fn start_key(
    o: &Opts<'_>,
    server: &Server,
    config: Option<&Path>,
) -> Result<Option<[u8; 32]>, String> {
    let c = api::config_at(server, config)?;
    let explicit_key = o.get("key").is_some() || std::env::var_os(crate::config::KEY_ENV).is_some();
    match (o.get("pubkey"), &c.pubkey) {
        (Some(p), _) => crate::config::checked_pubkey(&c, Path::new(p)).map(Some),
        (None, Some(_)) => api::release_key_of(&c).map(Some),
        (None, None) if explicit_key => Ok(Some(*crate::config::load_key(o.get("key"))?.pk)),
        (None, None) => Ok(None),
    }
}

pub fn command(args: &[&str]) -> Result<(), String> {
    run(args, None)
}

/// `dagon serve`, with the simulated desk of `dagon demo` when `sim` is
/// given (the page then reaches it instead of a real hub).
pub fn run(args: &[&str], sim: Option<Arc<SimDevice>>) -> Result<(), String> {
    // serve's output is a log for a person (a terminal, or a file it is
    // sent to): the banner opens it. Machine-readable output never has it.
    println!("{}\n", crate::banner_text());
    let no_open = args.contains(&"--no-open");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "--no-open").collect();
    let o = Opts::parse(&args, &["port", "web", "config", "pubkey", "key"])?;
    if !o.pos.is_empty() {
        return Err("serve takes options only".into());
    }
    let config = match o.get("config") {
        Some(p) => Some(PathBuf::from(p)),
        None => Path::new(CONFIG_FILE)
            .exists()
            .then(|| PathBuf::from(CONFIG_FILE)),
    };
    let cfg = Config::load(o.get("config"))?;
    // The page verifies releases with the developer's public key, given
    // now (not baked into the page): --pubkey, pubkey in dagon.toml, or the
    // public half of the signing key. The test key needs
    // DAGON_ALLOW_TEST_KEY=1.
    let web = match o.get("web") {
        Some(w) => PathBuf::from(w),
        None if api::page_embedded() => PathBuf::new(),
        None => cfg.base.join(DEFAULT_WEB),
    };
    let port: u16 = o
        .get("port")
        .map_or(Ok(0), |p| p.parse().map_err(|_| "--port must be a number"))?;
    // Loopback only: never 0.0.0.0.
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let server = Server::new(passphrase()?, port, config.clone(), web.clone());
    let key = start_key(&o, &server, config.as_deref()).map_err(|e| {
        format!(
            "dagon serve needs the developer's public key: pass --pubkey <file> \
             (from dagon key gen) or set pubkey in dagon.toml ({e})"
        )
    })?;
    if let Some(k) = key {
        server.set_release_key(k);
    }
    if let Some(s) = sim {
        let _ = server.sim.set(s);
    }
    if web.as_os_str().is_empty() {
        println!("DAGON Console: built into this executable");
    } else if !web.join("index.html").exists() {
        eprintln!(
            "note: {} has no index.html; build DAGON Console (npm run build in web/) or pass --web",
            web.display()
        );
    }
    match key {
        Some(k) => println!("release key: {}", crate::config::key_id_text(&k)),
        None => println!("no project yet: open or create one on the page"),
    }
    let url = format!("http://127.0.0.1:{port}/#{}", server.pass);
    println!("dagon serve on http://127.0.0.1:{port}/ (Ctrl+C stops it)");
    println!("open DAGON Console: {url}");
    if !no_open {
        open_browser(&url);
    }
    serve(listener, server);
    Ok(())
}

/// A local page that forwards to `url`. Opening it keeps the passphrase
/// out of the browser's command line (visible in the process list).
fn launcher_page(url: &str) -> String {
    let js = serde_json::to_string(url).unwrap_or_default();
    format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\">\
         <title>dagon serve</title><script>location.replace({js});</script>"
    )
}

/// The program and arguments that open `file` in the default browser.
fn open_command(file: &Path) -> (&'static str, Vec<std::ffi::OsString>) {
    let f = file.as_os_str().to_os_string();
    if cfg!(windows) {
        ("cmd", vec!["/C".into(), "start".into(), "".into(), f])
    } else if cfg!(target_os = "macos") {
        ("open", vec![f])
    } else {
        ("xdg-open", vec![f])
    }
}

/// Creates `path` (never over an existing file or link), readable by the
/// owner only on Unix; the per-user temporary folder covers Windows.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(bytes)
}

fn open_browser(url: &str) {
    let name = passphrase().map(|r| format!("dagon-open-{}.html", &r[..12]));
    let file = name.map(|n| std::env::temp_dir().join(n));
    let opened = file.as_ref().is_ok_and(|f| {
        write_private(f, launcher_page(url).as_bytes()).is_ok() && {
            let (prog, args) = open_command(f);
            std::process::Command::new(prog).args(args).spawn().is_ok()
        }
    });
    if let Ok(f) = file {
        std::thread::spawn(move || {
            std::thread::sleep(LAUNCHER_LIFETIME);
            let _ = std::fs::remove_file(f);
        });
    }
    if !opened {
        eprintln!("could not open a browser; open the URL above");
    }
}

/// Accepts connections forever, one thread each.
pub fn serve(listener: TcpListener, server: Arc<Server>) {
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for mut stream in listener.incoming().flatten() {
        if active.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::Relaxed);
            let _ = http::write_response(&mut stream, &Response::error(503, "busy"));
            continue;
        }
        let server = server.clone();
        let active = active.clone();
        std::thread::spawn(move || {
            connection(stream, &server);
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

fn connection(mut stream: TcpStream, server: &Arc<Server>) {
    let _ = stream.set_read_timeout(Some(HTTP_TIMEOUT));
    let req = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(e) => {
            let _ = http::write_response(&mut stream, &Response::error(400, &e));
            return;
        }
    };
    if let Some(resp) = guard(server, &req) {
        let _ = http::write_response(&mut stream, &resp);
        return;
    }
    let path = req.path.split('?').next().unwrap_or("");
    if path == "/v1/events" {
        events(stream, server, &req);
        return;
    }
    let resp = if path.starts_with("/v1/") {
        if http::authorized(&req, &server.pass) {
            api::handle(server, &req)
        } else {
            Response::error(401, "passphrase required")
        }
    } else if req.method == "GET" {
        match api::page_file(server, path) {
            Some((body, ct)) => Response::bytes(ct, body),
            None => Response::error(404, "not found"),
        }
    } else {
        Response::error(405, "method not allowed")
    };
    let _ = http::write_response(&mut stream, &resp);
}

/// Host and Origin checks every request passes first.
fn guard(server: &Server, req: &Request) -> Option<Response> {
    if !http::host_ok(req.header("host"), server.port) {
        return Some(Response::error(403, "bad Host"));
    }
    if !http::origin_ok(req.header("origin"), server.port) {
        return Some(Response::error(403, "bad Origin"));
    }
    None
}

/// The event stream: upgrade, passphrase as the first message, then every
/// event as a text frame until the page goes away.
fn events(mut stream: TcpStream, server: &Arc<Server>, req: &Request) {
    let upgrade = req
        .header("upgrade")
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    let key = req.header("sec-websocket-key").map(str::to_string);
    let (true, Some(key), true) = (upgrade, key, req.header("origin").is_some()) else {
        let _ = http::write_response(
            &mut stream,
            &Response::error(400, "WebSocket with Origin expected"),
        );
        return;
    };
    if stream
        .write_all(ws::handshake_response(&key).as_bytes())
        .is_err()
    {
        return;
    }
    let _ = stream.set_read_timeout(Some(WS_AUTH_TIMEOUT));
    let authed = matches!(ws::read_frame(&mut stream),
        Ok((ws::op::TEXT, p)) if http::same_secret(p.trim_ascii(), server.pass.as_bytes()));
    if !authed {
        let _ = ws::write_frame(&mut stream, ws::op::CLOSE, &4401u16.to_be_bytes());
        return;
    }
    if server.pages.fetch_add(1, Ordering::Relaxed) >= MAX_PAGES {
        server.pages.fetch_sub(1, Ordering::Relaxed);
        let _ = ws::write_frame(&mut stream, ws::op::CLOSE, &4429u16.to_be_bytes());
        return;
    }
    let Ok(reader) = stream.try_clone() else {
        server.pages.fetch_sub(1, Ordering::Relaxed);
        return;
    };
    let _ = reader.set_read_timeout(None);
    let _ = stream.set_write_timeout(Some(WS_WRITE_TIMEOUT));
    let writer = Arc::new(Mutex::new(stream));
    let closed = Arc::new(AtomicBool::new(false));
    spawn_reader(reader, writer.clone(), closed.clone());
    let rx = server.subscribe();
    let _ = send(&writer, r#"{"type":"ready"}"#);
    while !closed.load(Ordering::Relaxed) {
        match rx.recv_timeout(WS_POLL) {
            Ok(text) => {
                if send(&writer, &text).is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // Ends the reader thread too.
    if let Ok(s) = writer.lock() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    server.pages.fetch_sub(1, Ordering::Relaxed);
}

fn send(w: &Mutex<TcpStream>, text: &str) -> std::io::Result<()> {
    let mut s = w.lock().map_err(|_| std::io::Error::other("lock"))?;
    ws::write_frame(&mut *s, ws::op::TEXT, text.as_bytes())
}

/// Answers pings and notices close; the page sends nothing else.
fn spawn_reader(mut r: TcpStream, w: Arc<Mutex<TcpStream>>, closed: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            match ws::read_frame(&mut r) {
                Ok((ws::op::PING, p)) => {
                    if let Ok(mut s) = w.lock() {
                        let _ = ws::write_frame(&mut *s, ws::op::PONG, &p);
                    }
                }
                Ok((ws::op::CLOSE, _)) | Err(_) => break,
                Ok(_) => {}
            }
        }
        closed.store(true, Ordering::Relaxed);
        if let Ok(mut s) = w.lock() {
            let _ = ws::write_frame(&mut *s, ws::op::CLOSE, &[]);
        }
    });
}

#[cfg(test)]
mod tests;
