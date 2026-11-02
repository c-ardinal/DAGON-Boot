//! Minimal HTTP/1.1 for `dagon serve`: one request per connection, bodies
//! sized by Content-Length, and the origin checks every request passes.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

/// Largest request head and body the API takes.
pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads one request from `stream`.
pub fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut r = BufReader::new(stream);
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        // Never reads past MAX_HEAD, even inside one endless line.
        let room = (MAX_HEAD + 1).saturating_sub(head.len()) as u64;
        let n = (&mut r)
            .take(room)
            .read_until(b'\n', &mut head)
            .map_err(|e| e.to_string())?;
        if n == 0 || head.len() > MAX_HEAD {
            return Err("bad request head".into());
        }
    }
    let mut hs = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut hs);
    match req.parse(&head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err("bad request head".into()),
    }
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).trim().to_string(),
            )
        })
        .collect();
    let mut out = Request {
        method: req.method.unwrap_or("").to_string(),
        path: req.path.unwrap_or("").to_string(),
        headers,
        body: Vec::new(),
    };
    let len: usize = out
        .header("content-length")
        .map(|v| v.parse().map_err(|_| "bad content-length".to_string()))
        .transpose()?
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    out.body = vec![0; len];
    r.read_exact(&mut out.body).map_err(|e| e.to_string())?;
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, v: &serde_json::Value) -> Self {
        Response {
            status,
            content_type: "application/json",
            body: v.to_string().into_bytes(),
        }
    }

    pub fn error(status: u16, msg: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": msg }))
    }

    pub fn bytes(content_type: &'static str, body: Vec<u8>) -> Self {
        Response {
            status: 200,
            content_type,
            body,
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        501 => "Not Implemented",
        _ => "Error",
    }
}

/// Writes `r` and closes the exchange. No CORS headers: the page is served
/// from the same origin.
pub fn write_response(stream: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        r.status,
        reason(r.status),
        r.content_type,
        r.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

/// Host must name this server on the loopback (blocks DNS rebinding).
pub fn host_ok(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"))
}

/// An Origin, when sent, must be this server's own (blocks other sites).
pub fn origin_ok(origin: Option<&str>, port: u16) -> bool {
    origin.is_none_or(|o| {
        o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
    })
}

/// Constant-time comparison of the passphrase.
pub fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// `Authorization: Bearer <passphrase>`.
pub fn authorized(req: &Request, pass: &str) -> bool {
    req.header("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| same_secret(t.trim().as_bytes(), pass.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_origin_must_be_this_server() {
        assert!(host_ok(Some("127.0.0.1:8123"), 8123));
        assert!(host_ok(Some("localhost:8123"), 8123));
        assert!(!host_ok(Some("evil.example:8123"), 8123));
        assert!(!host_ok(Some("127.0.0.1:9"), 8123));
        assert!(!host_ok(None, 8123));
        assert!(origin_ok(None, 8123));
        assert!(origin_ok(Some("http://localhost:8123"), 8123));
        assert!(!origin_ok(Some("https://example.com"), 8123));
        assert!(!origin_ok(Some("null"), 8123));
    }

    #[test]
    fn bearer_passphrase() {
        let mut r = Request {
            method: "GET".into(),
            path: "/v1/hello".into(),
            headers: vec![("Authorization".into(), "Bearer abc".into())],
            body: vec![],
        };
        assert!(authorized(&r, "abc"));
        assert!(!authorized(&r, "abd"));
        r.headers.clear();
        assert!(!authorized(&r, "abc"));
        assert!(!same_secret(b"abc", b"abcd"));
    }
}
