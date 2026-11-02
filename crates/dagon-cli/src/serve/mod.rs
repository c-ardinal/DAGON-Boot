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
mod http;
mod jobs;
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
const DEFAULT_WEB: &str = "web/dist";

/// A fresh passphrase: 128 random bits as hex.
fn passphrase() -> Result<String, String> {
    let mut b = [0u8; PASS_BYTES];
    getrandom::fill(&mut b).map_err(|e| format!("no random source: {e}"))?;
    Ok(dagon_sign::to_hex(&b))
}

/// `dagon serve [--port <n>] [--web <dir>] [--no-open] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let no_open = args.contains(&"--no-open");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "--no-open").collect();
    let o = Opts::parse(&args, &["port", "web", "config"])?;
    if !o.pos.is_empty() {
        return Err("serve takes options only".into());
    }
    let config = match o.get("config") {
        Some(p) => Some(PathBuf::from(p)),
        None => Path::new(CONFIG_FILE)
            .exists()
            .then(|| PathBuf::from(CONFIG_FILE)),
    };
    let base = Config::load(o.get("config"))?.base;
    let web = o
        .get("web")
        .map_or_else(|| base.join(DEFAULT_WEB), PathBuf::from);
    let port: u16 = o
        .get("port")
        .map_or(Ok(0), |p| p.parse().map_err(|_| "--port must be a number"))?;
    // Loopback only: never 0.0.0.0.
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let server = Server::new(passphrase()?, port, config, web.clone());
    if !web.join("index.html").exists() {
        eprintln!(
            "note: {} has no index.html; build the page (npm run build in web/) or pass --web",
            web.display()
        );
    }
    let url = format!("http://127.0.0.1:{port}/#{}", server.pass);
    println!("dagon serve on http://127.0.0.1:{port}/ (Ctrl+C stops it)");
    println!("open: {url}");
    if !no_open {
        open_browser(&url);
    }
    serve(listener, server);
    Ok(())
}

fn open_browser(url: &str) {
    let r = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if r.is_err() {
        eprintln!("could not open a browser; open the URL above");
    }
}

/// Accepts connections forever, one thread each.
pub fn serve(listener: TcpListener, server: Arc<Server>) {
    for stream in listener.incoming().flatten() {
        let server = server.clone();
        std::thread::spawn(move || connection(stream, &server));
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
        match api::static_file(&server.web, path) {
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
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let _ = reader.set_read_timeout(None);
    let writer = Arc::new(Mutex::new(stream));
    let closed = Arc::new(AtomicBool::new(false));
    spawn_reader(reader, writer.clone(), closed.clone());
    let rx = server.subscribe();
    server.pages.fetch_add(1, Ordering::Relaxed);
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
