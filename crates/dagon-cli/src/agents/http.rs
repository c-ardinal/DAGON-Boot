//! Transport kind `messages-http`: a Messages API (Anthropic's protocol)
//! over HTTPS, with the way's key from the OS credential store
//! ([`crate::keystore`]). The key goes only into the request header; it is
//! never printed, logged, written or put in an error.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::{Cleared, Prompt, TIMED_OUT};

/// Tokens an answer may take.
const MAX_TOKENS: u32 = 16_000;
/// Characters an error from the agent's side is cut to: a summary for one
/// notice or log line, not the remote body.
const MAX_ERROR: usize = 300;
/// What stands where the agent's answer or error held the key.
const KEY_MARK: &str = "[API key]";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagesHttp {
    /// The messages endpoint, https only.
    pub url: String,
    /// The `anthropic-version` header.
    pub version: String,
    pub model: String,
    /// An environment variable that overrides `model`, if set.
    #[serde(default)]
    pub model_env: Option<String>,
}

fn env_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

impl MessagesHttp {
    pub(super) fn check(&self) -> Result<(), String> {
        if !self.url.starts_with("https://") {
            return Err("url must start with https://".into());
        }
        if !self.model_env.as_deref().is_none_or(env_name) {
            return Err("model_env must be an environment variable name".into());
        }
        if self.model.trim().is_empty() || self.version.trim().is_empty() {
            return Err("model and version are required".into());
        }
        Ok(())
    }

    fn model(&self, prompt: &Prompt) -> String {
        prompt
            .model
            .clone()
            .or_else(|| self.model_env.as_ref().and_then(|e| std::env::var(e).ok()))
            .unwrap_or_else(|| self.model.clone())
    }
}

/// The request body: the prompt's two texts as they are.
fn body(h: &MessagesHttp, prompt: &Prompt) -> Value {
    json!({
        "model": h.model(prompt),
        "max_tokens": MAX_TOKENS,
        // The large constant system text is cached across one run's calls.
        "system": [{ "type": "text", "text": prompt.system, "cache_control": { "type": "ephemeral" } }],
        "messages": [{ "role": "user", "content": [{ "type": "text", "text": prompt.user }] }],
    })
}

/// The answer's text.
fn text_of(resp: &Value) -> Result<String, String> {
    if resp["stop_reason"] == "max_tokens" {
        return Err("DAGON's answer was cut off; ask a narrower question".into());
    }
    let text: String = resp["content"]
        .as_array()
        .ok_or(super::UNEXPECTED)?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Ok(text)
}

/// One call.
pub(crate) fn post(
    _cleared: &Cleared,
    h: &MessagesHttp,
    key: &str,
    prompt: &Prompt,
    timeout: Duration,
) -> Result<String, String> {
    answer(h, key, prompt, timeout)
}

/// The call's answer or error, as every caller gets it. Both come from the
/// remote side, which may reflect the request's key header: every
/// occurrence of the key is replaced before either leaves this module, and
/// an error is cut to a short summary (after the replacement, so no part of
/// the key survives a cut).
fn answer(
    h: &MessagesHttp,
    key: &str,
    prompt: &Prompt,
    timeout: Duration,
) -> Result<String, String> {
    let clean = |s: String| {
        if key.is_empty() {
            s
        } else {
            s.replace(key, KEY_MARK)
        }
    };
    send(&h.url, &h.version, key, &body(h, prompt), timeout)
        .and_then(|v| text_of(&v))
        .map(clean)
        .map_err(|e| clean(e).chars().take(MAX_ERROR).collect())
}

fn agent_error(e: ureq::Error, what: &str) -> String {
    match e {
        ureq::Error::Timeout(_) => TIMED_OUT.into(),
        e => format!("{what}: {e}"),
    }
}

fn send(
    url: &str,
    version: &str,
    key: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        // A redirect would carry the key to another host: never follow one.
        .max_redirects(0)
        .https_only(url.starts_with("https://"))
        .build()
        .into();
    let mut resp = agent
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", version)
        .header("content-type", "application/json")
        .send(body.to_string())
        .map_err(|e| agent_error(e, "DAGON could not reach its agent"))?;
    let status = resp.status();
    if status.is_redirection() {
        return Err(format!(
            "the agent answered with a redirect (HTTP {status}); not followed"
        ));
    }
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| agent_error(e, "cannot read the agent's answer"))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("the agent answered with non-JSON (HTTP {status}): {e}"))?;
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("no message");
        return Err(format!(
            "the agent refused the request (HTTP {status}): {msg}"
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    const T: Duration = Duration::from_secs(30);

    /// One-shot local HTTP server standing in for the API (no network).
    fn mock_server(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/messages", l.local_addr().unwrap());
        let resp = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            s.write_all(resp.as_bytes()).unwrap();
            head + "\r\n" + &String::from_utf8(body).unwrap()
        });
        (url, h)
    }

    fn way() -> MessagesHttp {
        MessagesHttp {
            url: "https://example.invalid/v1/messages".into(),
            version: "2023-06-01".into(),
            model: "m-def".into(),
            model_env: None,
        }
    }

    #[test]
    fn http_request_shape() {
        let ok = json!({ "content": [{ "type": "text", "text": "{\"a\":1}" }], "stop_reason": "end_turn" });
        let (url, h) = mock_server("200 OK", &ok.to_string());
        let p = Prompt {
            system: "sys".into(),
            user: "usr".into(),
            model: None,
        };
        let v = send(&url, "2023-06-01", "sk-test", &body(&way(), &p), T).unwrap();
        assert_eq!(text_of(&v).unwrap(), "{\"a\":1}");
        let req = h.join().unwrap();
        let low = req.to_ascii_lowercase();
        assert!(low.starts_with("post /v1/messages "), "{req}");
        assert!(low.contains("x-api-key: sk-test"));
        assert!(low.contains("anthropic-version: 2023-06-01"));
        let sent: Value = serde_json::from_str(req.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(sent["model"], "m-def");
        assert_eq!(sent["system"][0]["text"], "sys");
        assert_eq!(sent["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(sent["messages"][0]["content"][0]["text"], "usr");
        let p = Prompt {
            model: Some("m2".into()),
            ..p
        };
        assert_eq!(body(&way(), &p)["model"], "m2");
    }

    #[test]
    fn http_redirect_is_not_followed() {
        let never = TcpListener::bind("127.0.0.1:0").unwrap();
        never.set_nonblocking(true).unwrap();
        let target = format!("http://{}/v1/messages", never.local_addr().unwrap());
        let (url, h) = mock_server(&format!("302 Found\r\nlocation: {target}"), "");
        let e = send(&url, "v", "sk-secret", &json!({}), T).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("302") && e.contains("not followed"), "{e}");
        assert!(!e.contains("sk-secret"));
        assert!(never.accept().is_err(), "redirect target was contacted");
    }

    #[test]
    fn http_error_message_without_key() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let (url, h) = mock_server("401 Unauthorized", body);
        let e = send(&url, "v", "sk-secret", &json!({}), T).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("401") && e.contains("invalid x-api-key"), "{e}");
        assert!(!e.contains("sk-secret"));
    }

    #[test]
    fn a_cut_off_answer_and_a_bad_definition_are_clear_errors() {
        assert!(
            text_of(&json!({ "stop_reason": "max_tokens", "content": [] }))
                .unwrap_err()
                .contains("cut off")
        );
        let mut bad = way();
        bad.url = "http://example.invalid".into();
        assert!(bad.check().is_err());
        bad = way();
        bad.model_env = Some("lower".into());
        assert!(bad.check().is_err());
    }

    const KEY: &str = "sk-reflected-0123456789abcdef";

    fn ask(url: String) -> Result<String, String> {
        let h = MessagesHttp { url, ..way() };
        let p = Prompt {
            system: "s".into(),
            user: "u".into(),
            model: None,
        };
        answer(&h, KEY, &p, T)
    }

    /// An endpoint that reflects the x-api-key header: in the answer, in
    /// an error message, and in a body that is no JSON. Nothing returned
    /// carries the key.
    #[test]
    fn a_reflected_key_never_comes_back() {
        let ok = json!({ "content": [{ "type": "text", "text": format!("your key is {KEY}.") }] });
        let (url, h) = mock_server("200 OK", &ok.to_string());
        let text = ask(url).unwrap();
        h.join().unwrap();
        assert!(
            !text.contains(KEY) && text.contains("your key is"),
            "{text}"
        );

        let msg = format!("invalid x-api-key {KEY} {}", "x".repeat(4000));
        let err =
            json!({ "type": "error", "error": { "type": "authentication_error", "message": msg } });
        let (url, h) = mock_server("401 Unauthorized", &err.to_string());
        let e = ask(url).unwrap_err();
        h.join().unwrap();
        assert!(
            !e.contains(KEY) && e.contains("401") && e.contains("invalid x-api-key"),
            "{e}"
        );
        assert!(
            e.chars().count() <= MAX_ERROR,
            "an error is a bounded summary: {}",
            e.len()
        );

        let (url, h) = mock_server("500 Internal Server Error", &format!("{{oops {KEY}"));
        let e = ask(url).unwrap_err();
        h.join().unwrap();
        assert!(!e.contains(KEY) && e.contains("500"), "{e}");
    }
}
