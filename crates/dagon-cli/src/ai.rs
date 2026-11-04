//! `dagon ai topology`: a natural-language board description becomes topology
//! YAML through the Claude Messages API. Each draft goes through the same
//! checks as `topo check`; errors go back to the model until none are left.
//! Values the model cannot derive are asked of the user instead of guessed.

use std::io::{BufRead, Read, Write};
use std::time::Duration;

use dagon_topology::defs::{BoardDef, I2cPair, McuDef, Registry, Writer};
use dagon_topology::schema::{ControlKind, LinkKind, RomEntry, UartIface};
use serde_json::{json, Value};

/// The API key is read from here only; it is never printed, logged or written.
pub(crate) const KEY_ENV: &str = "ANTHROPIC_API_KEY";
const MODEL_ENV: &str = "DAGON_AI_MODEL";
const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 16_000;
/// Model calls per run, counting question rounds and fix rounds.
const MAX_ROUNDS: usize = 8;
const HTTP_TIMEOUT: Duration = Duration::from_secs(600);
const TOOL: &str = "submit_topology";
const DEFAULT_OUT: &str = "topology.yaml";

const INSTRUCTIONS: &str = "You write DAGON Boot topology YAML files from a natural-language \
description of a board setup (hub, nodes, buses, links, addresses, pins).

Rules:
- Use only the keys and values defined by the schema below (Rust serde types; unknown keys are \
rejected). Do not invent keys. MCU, board, port and flash map names come from the board table.
- Never guess. Any value that cannot be derived with certainty from the description or the board \
table (pin numbers, I2C addresses, UART interface, baud rate, reset/boot pins, pull-ups) must be \
asked: put one plain question per missing value in `questions`. Never fill in a \
plausible-looking value. When you ask questions, `yaml` may be an empty string or a partial draft.
- When nothing is missing, return the complete YAML in `yaml` and an empty `questions` list.
- Every answer is sent back to you via `submit_topology`: either the user's answers or the \
checker report (JSON) of your YAML. Fix every reported error and submit the whole file again.
- The example shows the format only; its pins and addresses belong to that setup alone.

Always reply by calling `submit_topology`.";

const SCHEMA_RS: &str = include_str!("../../dagon-topology/src/schema.rs");
const CHECK_RS: &str = include_str!("../../dagon-topology/src/check.rs");
const EXAMPLE_YAML: &str = include_str!("../../../topology/desk-demo.yaml");

struct Args {
    input: Option<String>,
    out: String,
    model: String,
}

/// `dagon ai topology [--in <file>] [--out <yaml>] [--model <id>]`
// Writes the topology file the user names with --out.
#[allow(clippy::disallowed_methods)]
pub fn topology(args: &[&str]) -> Result<(), String> {
    let a = parse_args(args, std::env::var(MODEL_ENV).ok())?;
    let key = api_key(std::env::var(KEY_ENV))?;
    let description = match &a.input {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
        None => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| format!("cannot read the description from stdin: {e}"))?;
            s
        }
    };
    if description.trim().is_empty() {
        return Err("the board description is empty".into());
    }
    let interactive = a.input.is_some();
    let mut api = |body: &Value| post_messages(API_URL, &key, body);
    let mut ask = |qs: &[String]| {
        if interactive {
            prompt(qs)
        } else {
            Err(unanswered(qs))
        }
    };
    // The dagon.toml here, if any (else the builtin set); definitions that
    // do not load stop here with the reason.
    let reg = crate::defs::registry(&crate::config::Config::load(None)?)?;
    let done = generate(&description, &a.model, &reg, &mut api, &mut ask)?;
    std::fs::write(&a.out, &done.yaml).map_err(|e| format!("cannot write {}: {e}", a.out))?;
    for i in done.report.warnings() {
        println!("warning {} {}: {}", i.rule, i.path, i.message);
    }
    println!(
        "{}: 0 error(s), {} warning(s) after {} model call(s)",
        a.out,
        done.report.warnings().count(),
        done.rounds
    );
    for (q, ans) in &done.answers {
        println!("you supplied: {q} -> {ans}");
    }
    Ok(())
}

/// Whether a Claude API key is set (its value is never shown).
pub(crate) fn api_key_present() -> bool {
    api_key(std::env::var(KEY_ENV)).is_ok()
}

/// For `dagon serve`: the topology YAML for `description` and its check
/// report (JSON), with the model's questions answered through `ask`.
pub(crate) fn topology_for(
    description: &str,
    reg: &Registry,
    ask: &mut Ask,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(String, String), String> {
    if description.trim().is_empty() {
        return Err("the board description is empty".into());
    }
    let key = api_key(std::env::var(KEY_ENV))?;
    let model = std::env::var(MODEL_ENV).unwrap_or_else(|_| DEFAULT_MODEL.into());
    let mut post = |body: &Value| post_messages(API_URL, &key, body);
    let mut api = |body: &Value| unless_cancelled(cancel, &mut post, body);
    let done = generate(description, &model, reg, &mut api, ask)?;
    Ok((done.yaml, done.report.to_json()))
}

/// One model call, refused once `cancel` is set (a cancelled job stops at
/// its next call).
fn unless_cancelled(
    cancel: &std::sync::atomic::AtomicBool,
    api: &mut Api,
    body: &Value,
) -> Result<Value, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    api(body)
}

fn parse_args(args: &[&str], env_model: Option<String>) -> Result<Args, String> {
    let mut a = Args {
        input: None,
        out: DEFAULT_OUT.into(),
        model: env_model.unwrap_or_else(|| DEFAULT_MODEL.into()),
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .map(|v| v.to_string())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match *flag {
            "--in" => a.input = Some(value()?),
            "--out" => a.out = value()?,
            "--model" => a.model = value()?,
            other => {
                return Err(format!(
                    "unknown argument {other}; usage: dagon ai topology [--in <file>] [--out <yaml>] [--model <id>]"
                ))
            }
        }
    }
    Ok(a)
}

fn api_key(v: Result<String, std::env::VarError>) -> Result<String, String> {
    match v {
        Ok(k) if !k.trim().is_empty() => Ok(k.trim().to_string()),
        _ => Err(format!(
            "{KEY_ENV} is not set; export your Claude API key in that environment variable"
        )),
    }
}

#[derive(Debug)]
struct Outcome {
    yaml: String,
    report: dagon_topology::Report,
    /// (question, answer) pairs the user supplied.
    answers: Vec<(String, String)>,
    rounds: usize,
}

type Api<'a> = dyn FnMut(&Value) -> Result<Value, String> + 'a;
pub(crate) type Ask<'a> = dyn FnMut(&[String]) -> Result<Vec<String>, String> + 'a;

/// Model loop: draft, ask for unknowns, check, feed errors back, until the
/// check reports 0 errors or `MAX_ROUNDS` calls are spent.
fn generate(
    description: &str,
    model: &str,
    reg: &Registry,
    api: &mut Api,
    ask: &mut Ask,
) -> Result<Outcome, String> {
    let mut messages = vec![json!({
        "role": "user",
        "content": [{ "type": "text", "text": format!("Board description:\n\n{description}") }],
    })];
    let mut answers = Vec::new();
    let mut last_errors = String::new();
    for round in 1..=MAX_ROUNDS {
        let resp = api(&request(model, reg, &messages))?;
        let (id, yaml, questions) = tool_call(&resp)?;
        messages.push(json!({ "role": "assistant", "content": resp["content"] }));
        let (feedback, is_error) = if !questions.is_empty() {
            let got = ask(&questions)?;
            let text = questions
                .iter()
                .zip(&got)
                .map(|(q, a)| format!("Q: {q}\nA: {a}"))
                .collect::<Vec<_>>()
                .join("\n");
            answers.extend(questions.into_iter().zip(got));
            (format!("User answers:\n{text}"), false)
        } else {
            let report = dagon_topology::check_str_with(reg, &yaml);
            if report.is_pushable() {
                return Ok(Outcome {
                    yaml,
                    report,
                    answers,
                    rounds: round,
                });
            }
            last_errors = report
                .errors()
                .map(|i| format!("  {} {}: {}", i.rule, i.path, i.message))
                .collect::<Vec<_>>()
                .join("\n");
            (format!("Checker report:\n{}", report.to_json()), true)
        };
        messages.push(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": id,
                "content": feedback,
                "is_error": is_error,
            }],
        }));
    }
    Err(format!(
        "no error-free topology after {MAX_ROUNDS} model calls; last errors:\n{last_errors}"
    ))
}

/// What the model needs of every MCU and board in use, as JSON built field
/// by field from an allow-list: ids, names, pins and their labels, flash
/// sizes, link kinds, the writer kind. Nothing else a definition holds
/// (doctor commands and hints, file names, comments) is ever sent.
/// A project's own words were made safe when its definitions loaded
/// (`page_safe`, else `project-text`; identifiers must be plain).
fn board_table(reg: &Registry) -> String {
    let mcus: Vec<Value> = reg.mcus().iter().map(mcu_facts).collect();
    let boards: Vec<Value> = reg.boards().iter().map(board_facts).collect();
    serde_json::to_string_pretty(&json!({ "mcus": mcus, "boards": boards }))
        .expect("facts serialise")
}

fn names(kinds: &[LinkKind]) -> Vec<&'static str> {
    kinds.iter().map(|k| k.name()).collect()
}

fn pairs(p: &[I2cPair]) -> Vec<Value> {
    p.iter()
        .map(|p| json!({ "controller": p.controller, "sda": p.sda, "scl": p.scl }))
        .collect()
}

fn mcu_facts(m: &McuDef) -> Value {
    let writer = match m.writer {
        Writer::Uf2Drive { .. } => "uf2-drive",
        Writer::ProbeRs { .. } => "probe-rs",
        Writer::Espflash { .. } => "espflash",
    };
    json!({
        "id": m.id,
        "name": m.name,
        "port": m.port.as_str(),
        "gpio_count": m.gpio_count,
        "flash_maps": m.flash_maps.iter().map(|f| json!({ "name": f.name, "bytes": f.bytes })).collect::<Vec<_>>(),
        "writer": writer,
        "links": { "up": names(&m.links.up), "down": names(&m.links.down) },
        "coordinator": m.coordinator.as_ref().map(|c| json!({
            "pio_state_machines": c.pio_state_machines,
            "pio_uart_state_machines": c.pio_uart_state_machines,
            "uart0_tx": c.uart0_tx,
            "uart0_rx": c.uart0_rx,
        })),
        "rom_i2c": m.rom_i2c.as_ref().map(|r| json!({
            "addr": r.addr,
            "max_hz": r.max_hz,
            "pairs": pairs(&r.pairs),
        })),
    })
}

fn board_facts(b: &BoardDef) -> Value {
    let link = b.default_link.map(|l| {
        json!({
            "kind": l.kind.name(),
            "iface": l.iface.map(|i| match i {
                UartIface::HwUart0 => "hw-uart0",
                UartIface::PioUart => "pio-uart",
            }),
            "addr": l.addr,
            "baud": l.baud,
            "control": match l.control {
                ControlKind::Full => "full",
                ControlKind::Cooperative => "cooperative",
            },
            "rom_entry": l.rom_entry.map(|r| match r {
                RomEntry::ForceDownloadBoot => "force-download-boot",
                RomEntry::M5pm1 => "m5pm1",
                RomEntry::Stm32I2cRom => "stm32-i2c-rom",
            }),
        })
    });
    json!({
        "id": b.id,
        "product": b.product,
        "mcu": b.mcu,
        "flash_bytes": b.flash_bytes,
        "i2c_target": b.i2c_target,
        "header": b.header.iter().map(|h| json!({ "pin": h.pin, "label": h.label })).collect::<Vec<_>>(),
        "i2c": pairs(&b.i2c_pairs),
        "default_link": link,
    })
}

fn request(model: &str, reg: &Registry, messages: &[Value]) -> Value {
    // A cache breakpoint on the newest block bills only the delta next round.
    let mut messages = messages.to_vec();
    if let Some(block) = messages
        .last_mut()
        .and_then(|m| m["content"].as_array_mut())
        .and_then(|c| c.last_mut())
    {
        block["cache_control"] = json!({ "type": "ephemeral" });
    }
    let system = format!(
        "{INSTRUCTIONS}\n\n<schema file=\"schema.rs\">\n{SCHEMA_RS}</schema>\n\n\
         <board_table format=\"json\">\n{}\n</board_table>\n\n\
         <checks file=\"check.rs\">\n{CHECK_RS}</checks>\n\n\
         <example file=\"desk-demo.yaml\">\n{EXAMPLE_YAML}</example>",
        board_table(reg)
    );
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        // The large constant prefix is cached across the rounds of one run.
        "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
        "tools": [{
            "name": TOOL,
            "description": "Submit the topology YAML draft and any questions for values that cannot be derived.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "yaml": { "type": "string", "description": "The whole topology YAML file (may be empty while questions are open)." },
                    "questions": { "type": "array", "items": { "type": "string" }, "description": "One question per unknown value; empty when the YAML is complete." }
                },
                "required": ["yaml", "questions"]
            }
        }],
        "tool_choice": { "type": "tool", "name": TOOL },
        "messages": messages,
    })
}

/// The `submit_topology` call in a Messages API response: (id, yaml, questions).
fn tool_call(resp: &Value) -> Result<(String, String, Vec<String>), String> {
    let block = resp["content"]
        .as_array()
        .and_then(|c| {
            c.iter()
                .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)
        })
        .ok_or_else(|| {
            format!(
                "the model did not call {TOOL} (stop_reason {})",
                resp["stop_reason"]
            )
        })?;
    if resp["stop_reason"] == "max_tokens" {
        return Err("the model reply was cut off at max_tokens".into());
    }
    let id = block["id"]
        .as_str()
        .ok_or("tool call without id")?
        .to_string();
    let yaml = block["input"]["yaml"].as_str().unwrap_or("").to_string();
    let questions = block["input"]["questions"]
        .as_array()
        .map(|qs| {
            qs.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((id, yaml, questions))
}

fn post_messages(base: &str, key: &str, body: &Value) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        // A redirect would carry x-api-key to another host: never follow one.
        .max_redirects(0)
        .https_only(base.starts_with("https://"))
        .build()
        .into();
    let mut resp = agent
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("Claude API request failed: {e}"))?;
    let status = resp.status();
    if status.is_redirection() {
        return Err(format!(
            "Claude API answered with a redirect (HTTP {status}); not followed"
        ));
    }
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("cannot read the Claude API response: {e}"))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Claude API returned non-JSON (HTTP {status}): {e}"))?;
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("no message");
        return Err(format!("Claude API error (HTTP {status}): {msg}"));
    }
    Ok(v)
}

fn prompt(qs: &[String]) -> Result<Vec<String>, String> {
    eprintln!("The model needs these values (it does not guess them):");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    qs.iter()
        .map(|q| {
            eprint!("? {q}\n> ");
            let _ = std::io::stderr().flush();
            match lines.next() {
                Some(Ok(l)) => Ok(l.trim().to_string()),
                _ => Err(unanswered(qs)),
            }
        })
        .collect()
}

fn unanswered(qs: &[String]) -> String {
    let list: String = qs.iter().map(|q| format!("\n  - {q}")).collect();
    format!(
        "the model needs values it cannot derive; add them to the description and run again \
         with --in <file>:{list}"
    )
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_job_makes_no_model_call() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut calls = 0;
        let mut fake = |_: &Value| {
            calls += 1;
            Ok(json!({}))
        };
        let mut api = |b: &Value| unless_cancelled(&flag, &mut fake, b);
        let mut ask = |_: &[String]| Ok(Vec::new());
        let r = generate("a hub", "m", Registry::builtin(), &mut api, &mut ask);
        assert_eq!(r.unwrap_err(), "cancelled");
        assert_eq!(calls, 0);
    }
    use std::collections::VecDeque;
    use std::io::BufReader;
    use std::net::TcpListener;

    fn reply(id: &str, yaml: &str, questions: &[&str]) -> Value {
        json!({
            "content": [{ "type": "tool_use", "id": id, "name": TOOL,
                          "input": { "yaml": yaml, "questions": questions } }],
            "stop_reason": "tool_use",
        })
    }

    #[test]
    fn asks_then_fixes_until_check_passes() {
        let mut replies = VecDeque::from([
            reply("t1", "", &["I2C address of seg?"]),
            reply("t2", "dagon: 1\nname: broken\n", &[]),
            reply("t3", EXAMPLE_YAML, &[]),
        ]);
        let mut sent = Vec::new();
        let mut api = |b: &Value| {
            sent.push(b.clone());
            Ok(replies.pop_front().unwrap())
        };
        let mut asked = Vec::new();
        let mut ask = |qs: &[String]| {
            asked.extend_from_slice(qs);
            Ok(vec!["0x42".to_string()])
        };
        let out = generate(
            "hub and seg",
            "m-test",
            Registry::builtin(),
            &mut api,
            &mut ask,
        )
        .unwrap();
        assert_eq!(out.rounds, 3);
        assert_eq!(out.yaml, EXAMPLE_YAML);
        assert!(out.report.is_pushable());
        assert_eq!(asked, ["I2C address of seg?"]);
        assert_eq!(out.answers, [("I2C address of seg?".into(), "0x42".into())]);
        assert_eq!(sent[0]["model"], "m-test");
        assert_eq!(sent[0]["tool_choice"]["name"], TOOL);
        // Round 2 carries the answer, round 3 the checker errors, both as tool results.
        let r2 = &sent[1]["messages"][2]["content"][0];
        assert_eq!(r2["tool_use_id"], "t1");
        assert!(r2["content"].as_str().unwrap().contains("A: 0x42"));
        let r3 = &sent[2]["messages"][4]["content"][0];
        assert_eq!(r3["tool_use_id"], "t2");
        assert_eq!(r3["is_error"], true);
        assert!(r3["content"].as_str().unwrap().contains("\"ok\":false"));
    }

    #[test]
    fn gives_up_after_max_rounds() {
        let mut calls = 0;
        let mut api = |_: &Value| {
            calls += 1;
            Ok(reply("t", "not: [valid", &[]))
        };
        let mut ask = |_: &[String]| -> Result<Vec<String>, String> { unreachable!() };
        let e = generate("x", "m", Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert_eq!(calls, MAX_ROUNDS);
        assert!(e.contains("no error-free topology"), "{e}");
    }

    #[test]
    fn non_interactive_questions_are_printed_as_error() {
        let mut api = |_: &Value| Ok(reply("t", "", &["baud rate of watch?"]));
        let mut ask = |qs: &[String]| Err(unanswered(qs));
        let e = generate("x", "m", Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert!(e.contains("baud rate of watch?"), "{e}");
    }

    #[test]
    fn missing_key_is_a_clear_error() {
        let e = api_key(Err(std::env::VarError::NotPresent)).unwrap_err();
        assert!(e.contains(KEY_ENV));
        assert!(api_key(Ok("  ".into())).is_err());
        assert_eq!(api_key(Ok("k\n".into())).unwrap(), "k");
    }

    #[test]
    fn args_and_model_override() {
        let a = parse_args(&[], None).unwrap();
        assert_eq!(
            (a.out.as_str(), a.model.as_str()),
            (DEFAULT_OUT, DEFAULT_MODEL)
        );
        assert!(a.input.is_none());
        let a = parse_args(&["--in", "d.txt", "--model", "m2"], Some("m1".into())).unwrap();
        assert_eq!(
            (a.input.as_deref(), a.model.as_str()),
            (Some("d.txt"), "m2")
        );
        assert_eq!(parse_args(&[], Some("m1".into())).unwrap().model, "m1");
        assert!(parse_args(&["--out"], None).is_err());
        assert!(parse_args(&["--bogus"], None).is_err());
    }

    /// One-shot local HTTP server standing in for the API (no network).
    fn mock_server(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
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
        (base, h)
    }

    #[test]
    fn http_request_shape() {
        let ok = reply("t1", "y", &[]).to_string();
        let (base, h) = mock_server("200 OK", &ok);
        let v = post_messages(&base, "sk-test", &json!({ "model": "m" })).unwrap();
        assert_eq!(v["content"][0]["id"], "t1");
        let req = h.join().unwrap().to_ascii_lowercase();
        assert!(req.starts_with("post /v1/messages "), "{req}");
        assert!(req.contains("x-api-key: sk-test"));
        assert!(req.contains(&format!("anthropic-version: {API_VERSION}")));
        assert!(req.ends_with("{\"model\":\"m\"}"));
    }

    #[test]
    fn http_redirect_is_not_followed() {
        let never = TcpListener::bind("127.0.0.1:0").unwrap();
        never.set_nonblocking(true).unwrap();
        let target = format!("http://{}/v1/messages", never.local_addr().unwrap());
        let (base, h) = mock_server(&format!("302 Found\r\nlocation: {target}"), "");
        let e = post_messages(&base, "sk-secret", &json!({})).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("302") && e.contains("not followed"), "{e}");
        assert!(!e.contains("sk-secret"));
        assert!(never.accept().is_err(), "redirect target was contacted");
    }

    #[test]
    fn last_message_gets_cache_breakpoint_only_in_request() {
        let msg = |role: &str, t: &str| json!({ "role": role, "content": [{ "type": "text", "text": t }] });
        let msgs = vec![msg("user", "a"), msg("assistant", "b"), msg("user", "c")];
        let r = request("m", Registry::builtin(), &msgs);
        assert_eq!(
            r["messages"][2]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert!(r["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert!(msgs[2]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn the_board_table_sends_facts_only_never_a_projects_commands_or_paths() {
        let dir = std::env::temp_dir().join(format!("dagon-ai-defs-{}", std::process::id()));
        for sub in ["m/stage0", "m/bootcore"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let secret = "sk-ant-FAKE-TOKEN-4242";
        let tool = dir
            .join("bin/tool.exe")
            .display()
            .to_string()
            .replace('\\', "/");
        let mcu = include_str!("../../../targets/mcu/rp2040/dagon-mcu.toml")
            .replace(
                "id = \"rp2040\"",
                &format!("# {secret} at {tool}\nid = \"desk-m0\""),
            )
            .replace("wire_id = 0x0001", "wire_id = 0x00F2")
            .replace("name = \"rp2040-4m\"", "name = \"desk-m0-4m\"");
        let doctor = format!(
            "\n[[doctor]]\nwhat = \"desk tool\"\nprobe = [\"{tool}\", \"--token\", \"{secret}\"]\n\
             expect = \"runs\"\nfix = [\"{tool}\", \"install\"]\nhint = \"ask {secret}\"\n"
        );
        std::fs::write(dir.join("m/dagon-mcu.toml"), format!("{mcu}{doctor}")).unwrap();
        let toml = dir.join("dagon.toml");
        std::fs::write(
            &toml,
            "mcu_files = [\"m/dagon-mcu.toml\"]\n\
             [board.desk-pico]\nproduct = \"Desk Pico\"\nmcu = \"desk-m0\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\
             header = [{ pin = \"GPIO4\", label = \"D4\" }]\n\
             default_link = { kind = \"i2c\", addr = 0x44, control = \"full\" }\n\
             [board.desk-path]\nproduct = \"C:/Users/me/desk\"\nmcu = \"desk-m0\"\n\
             flash_bytes = 0x40_0000\ndrawing = \"pico\"\n\
             header = [{ pin = \"GPIO4\", label = \"see C:/Users/me/notes\" }]\n\
             default_link = { kind = \"i2c\", addr = 0x45, control = \"full\" }\n",
        )
        .unwrap();
        let reg = Registry::load(&[dagon_topology::defs::Source::Project(&toml)]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!reg.mcu("desk-m0").unwrap().doctor.is_empty());

        let system = request("m", &reg, &[])["system"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let table: Value = serde_json::from_str(&board_table(&reg)).unwrap();
        let ids = |k: &str| -> Vec<String> {
            table[k]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["id"].as_str().unwrap().into())
                .collect()
        };
        assert_eq!(
            ids("mcus"),
            reg.mcus().iter().map(|m| m.id.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            ids("boards"),
            reg.boards()
                .iter()
                .map(|b| b.id.clone())
                .collect::<Vec<_>>()
        );
        let board = |id: &str| -> Value {
            table["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["id"] == id)
                .unwrap()
                .clone()
        };
        let pico = &board("desk-pico");
        // A project's words that are not plain names go as project-text.
        let path = board("desk-path");
        assert_eq!(path["product"], dagon_topology::defs::PROJECT_TEXT);
        assert_eq!(
            path["header"][0],
            json!({ "pin": "GPIO4", "label": dagon_topology::defs::PROJECT_TEXT })
        );
        assert_eq!(pico["product"], "Desk Pico");
        // Builtin facts go as they are.
        assert_eq!(
            board("waveshare-rp2040-touch-lcd-1.28")["product"],
            "Waveshare RP2040-Touch-LCD-1.28"
        );
        assert_eq!(pico["header"][0], json!({ "pin": "GPIO4", "label": "D4" }));
        assert_eq!(pico["default_link"]["addr"], 0x44);
        let m0 = table["mcus"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "desk-m0")
            .unwrap();
        assert_eq!(m0["writer"], "uf2-drive");
        assert_eq!(m0["links"]["up"], json!(["usb-cdc"]));

        let plain = system.replace("\\\\", "\\");
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        for never in [
            secret,
            "tool.exe",
            "desk tool",
            "rustup",
            "INFO_UF2",
            "boards.rs",
            "Users",
            &dir.display().to_string(),
            &home,
        ] {
            assert!(
                never.is_empty() || !plain.contains(never),
                "{never} reached the prompt"
            );
        }
    }

    #[test]
    fn http_error_message_without_key() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let (base, h) = mock_server("401 Unauthorized", body);
        let e = post_messages(&base, "sk-secret", &json!({})).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("401") && e.contains("invalid x-api-key"), "{e}");
        assert!(!e.contains("sk-secret"));
    }
}
