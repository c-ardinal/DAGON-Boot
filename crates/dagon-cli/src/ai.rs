//! `dagon ai topology`: a natural-language board description becomes topology
//! YAML through the linked agent (see [`crate::agents`]). Each draft goes
//! through the same checks as `topo check`; errors go back to the agent
//! until none are left. Values the agent cannot derive are asked of the
//! user instead of guessed.

use std::io::{BufRead, Read, Write};
use std::time::Duration;

use dagon_topology::defs::{BoardDef, I2cPair, McuDef, Registry, Writer};
use dagon_topology::schema::{ControlKind, LinkKind, RomEntry, UartIface};
use serde_json::{json, Value};

use crate::agents::{Api, Prompt};

/// Agent calls per run, counting question rounds and fix rounds.
const MAX_ROUNDS: usize = 8;
/// One call of a topology draft waits at most this long.
pub(crate) const DRAFT_TIMEOUT: Duration = Duration::from_secs(600);
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
- The conversation holds your earlier replies and what DAGON sent back: either the user's \
answers or the checker report (JSON) of your YAML. Fix every reported error and reply with the \
whole file again.
- The example shows the format only; its pins and addresses belong to that setup alone.";

pub(crate) const SCHEMA_RS: &str = include_str!("../../dagon-topology/src/schema.rs");
pub(crate) const CHECK_RS: &str = include_str!("../../dagon-topology/src/check.rs");
const EXAMPLE_YAML: &str = include_str!("../../../topology/desk-demo.yaml");

struct Args {
    input: Option<String>,
    out: String,
    model: Option<String>,
}

/// `dagon ai topology [--in <file>] [--out <yaml>] [--model <id>]`
pub fn topology(args: &[&str]) -> Result<(), String> {
    let a = parse_args(args)?;
    let home = crate::consent::home()?;
    crate::consent::require_linked(&home)?;
    crate::agents::ready(&home)?;
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
    let mut api = |p: &Prompt| crate::agents::call(&home, p, DRAFT_TIMEOUT);
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
    let done = generate(&description, a.model, &reg, &mut api, &mut ask)?;
    std::fs::write(&a.out, &done.yaml).map_err(|e| format!("cannot write {}: {e}", a.out))?;
    for i in done.report.warnings() {
        println!("warning {} {}: {}", i.rule, i.path, i.message);
    }
    println!(
        "{}: 0 error(s), {} warning(s) after {} agent call(s)",
        a.out,
        done.report.warnings().count(),
        done.rounds
    );
    for (q, ans) in &done.answers {
        println!("you supplied: {q} -> {ans}");
    }
    Ok(())
}

/// For `dagon serve`: the topology YAML for `description` and its check
/// report (JSON), with the agent's questions answered through `ask` and
/// every agent call made through `post`.
pub(crate) fn topology_for(
    description: &str,
    reg: &Registry,
    ask: &mut Ask,
    cancel: &std::sync::atomic::AtomicBool,
    post: &mut Api,
) -> Result<(String, String), String> {
    if description.trim().is_empty() {
        return Err("the board description is empty".into());
    }
    let mut api = |p: &Prompt| unless_cancelled(cancel, post, p);
    let done = generate(description, None, reg, &mut api, ask)?;
    Ok((done.yaml, done.report.to_json()))
}

/// One agent call, refused once `cancel` is set (a cancelled job stops at
/// its next call).
fn unless_cancelled(
    cancel: &std::sync::atomic::AtomicBool,
    api: &mut Api,
    p: &Prompt,
) -> Result<String, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    api(p)
}

fn parse_args(args: &[&str]) -> Result<Args, String> {
    let mut a = Args {
        input: None,
        out: DEFAULT_OUT.into(),
        model: None,
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
            "--model" => a.model = Some(value()?),
            other => {
                return Err(format!(
                    "unknown argument {other}; usage: dagon ai topology [--in <file>] [--out <yaml>] [--model <id>]"
                ))
            }
        }
    }
    Ok(a)
}

#[derive(Debug)]
struct Outcome {
    yaml: String,
    report: dagon_topology::Report,
    /// (question, answer) pairs the user supplied.
    answers: Vec<(String, String)>,
    rounds: usize,
}

pub(crate) type Ask<'a> = dyn FnMut(&[String]) -> Result<Vec<String>, String> + 'a;

/// The reply every draft round asks for.
fn reply_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "yaml": { "type": "string", "description": "The whole topology YAML file (may be empty while questions are open)." },
            "questions": { "type": "array", "items": { "type": "string" }, "description": "One question per unknown value; empty when the YAML is complete." }
        },
        "required": ["yaml", "questions"]
    })
}

/// The system text of every draft round: the same for every agent and way.
fn system_text(reg: &Registry) -> String {
    format!(
        "{INSTRUCTIONS}\n\n<schema file=\"schema.rs\">\n{SCHEMA_RS}</schema>\n\n\
         <board_table format=\"json\">\n{}\n</board_table>\n\n\
         <checks file=\"check.rs\">\n{CHECK_RS}</checks>\n\n\
         <example file=\"desk-demo.yaml\">\n{EXAMPLE_YAML}</example>\n\n{}",
        board_table(reg),
        crate::agents::reply_rule(&reply_schema())
    )
}

/// (yaml, questions) of a reply.
fn draft_of(text: &str) -> Result<(Value, String, Vec<String>), String> {
    let v = crate::agents::reply_json(text)?;
    let yaml = v["yaml"].as_str().unwrap_or("").to_string();
    let questions = v["questions"]
        .as_array()
        .map(|qs| {
            qs.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((v, yaml, questions))
}

/// Agent loop: draft, ask for unknowns, check, feed errors back, until the
/// check reports 0 errors or `MAX_ROUNDS` calls are spent.
fn generate(
    description: &str,
    model: Option<String>,
    reg: &Registry,
    api: &mut Api,
    ask: &mut Ask,
) -> Result<Outcome, String> {
    let system = system_text(reg);
    let mut turns = vec![format!("Board description:\n\n{description}")];
    let mut answers = Vec::new();
    let mut last_errors = String::new();
    for round in 1..=MAX_ROUNDS {
        let p = Prompt {
            system: system.clone(),
            user: crate::agents::conversation(&turns),
            model: model.clone(),
        };
        let (reply, yaml, questions) = draft_of(&api(&p)?)?;
        turns.push(reply.to_string());
        let feedback = if !questions.is_empty() {
            let got = ask(&questions)?;
            let text = questions
                .iter()
                .zip(&got)
                .map(|(q, a)| format!("Q: {q}\nA: {a}"))
                .collect::<Vec<_>>()
                .join("\n");
            answers.extend(questions.into_iter().zip(got));
            format!("User answers:\n{text}")
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
            format!("Checker report:\n{}", report.to_json())
        };
        turns.push(feedback);
    }
    Err(format!(
        "no error-free topology after {MAX_ROUNDS} agent calls; last errors:\n{last_errors}"
    ))
}

/// What the model needs of every MCU and board in use, as JSON built field
/// by field from an allow-list: ids, names, pins and their labels, flash
/// sizes, link kinds, the writer kind. Nothing else a definition holds
/// (doctor commands and hints, file names, comments) is ever sent.
/// A project's own words were made safe when its definitions loaded
/// (`page_safe`, else `project-text`; identifiers must be plain).
pub(crate) fn board_table(reg: &Registry) -> String {
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

fn prompt(qs: &[String]) -> Result<Vec<String>, String> {
    eprintln!("DAGON needs these values (it does not guess them):");
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
        "DAGON needs values it cannot derive; add them to the description and run again \
         with --in <file>:{list}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn a_cancelled_job_makes_no_model_call() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut calls = 0;
        let mut fake = |_: &Prompt| {
            calls += 1;
            Ok("{}".to_string())
        };
        let mut api = |p: &Prompt| unless_cancelled(&flag, &mut fake, p);
        let mut ask = |_: &[String]| Ok(Vec::new());
        let r = generate("a hub", None, Registry::builtin(), &mut api, &mut ask);
        assert_eq!(r.unwrap_err(), "cancelled");
        assert_eq!(calls, 0);
    }

    fn reply(yaml: &str, questions: &[&str]) -> String {
        json!({ "yaml": yaml, "questions": questions }).to_string()
    }

    #[test]
    fn asks_then_fixes_until_check_passes() {
        let mut replies = VecDeque::from([
            reply("", &["I2C address of seg?"]),
            // Words or a code fence around the object are tolerated.
            format!("```json\n{}\n```", reply("dagon: 1\nname: broken\n", &[])),
            reply(EXAMPLE_YAML, &[]),
        ]);
        let mut sent: Vec<Prompt> = Vec::new();
        let mut api = |p: &Prompt| {
            sent.push(p.clone());
            Ok(replies.pop_front().unwrap())
        };
        let mut asked = Vec::new();
        let mut ask = |qs: &[String]| {
            asked.extend_from_slice(qs);
            Ok(vec!["0x42".to_string()])
        };
        let out = generate(
            "hub and seg",
            Some("m-test".into()),
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
        assert_eq!(sent[0].model.as_deref(), Some("m-test"));
        assert_eq!(sent[0].user, "Board description:\n\nhub and seg");
        // Every round sends the same system text; the conversation grows.
        assert!(sent.iter().all(|p| p.system == sent[0].system));
        assert!(sent[0]
            .system
            .ends_with(&crate::agents::reply_rule(&reply_schema())));
        assert!(sent[1].user.contains("<turn from=\"you\">") && sent[1].user.contains("A: 0x42"));
        assert!(sent[2].user.contains("Checker report:") && sent[2].user.contains("\"ok\":false"));
    }

    #[test]
    fn gives_up_after_max_rounds() {
        let mut calls = 0;
        let mut api = |_: &Prompt| {
            calls += 1;
            Ok(reply("not: [valid", &[]))
        };
        let mut ask = |_: &[String]| -> Result<Vec<String>, String> { unreachable!() };
        let e = generate("x", None, Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert_eq!(calls, MAX_ROUNDS);
        assert!(e.contains("no error-free topology"), "{e}");
    }

    #[test]
    fn non_interactive_questions_are_printed_as_error() {
        let mut api = |_: &Prompt| Ok(reply("", &["baud rate of watch?"]));
        let mut ask = |qs: &[String]| Err(unanswered(qs));
        let e = generate("x", None, Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert!(e.contains("baud rate of watch?"), "{e}");
        let mut api = |_: &Prompt| Ok("I cannot help with that.".to_string());
        let mut ask = |_: &[String]| -> Result<Vec<String>, String> { unreachable!() };
        let e = generate("x", None, Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert_eq!(e, crate::agents::UNEXPECTED);
    }

    #[test]
    fn a_draft_stops_at_the_call_after_an_unlink() {
        let home = std::env::temp_dir().join(format!("dagon-ai-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        crate::consent::link(&home, "claude", "api").unwrap();
        let mut calls = 0;
        let mut api = |p: &Prompt| {
            let r = {
                let _held = crate::agents::before_call(&home, p)?;
                calls += 1;
                Ok(reply("not: [valid", &[]))
            };
            crate::consent::unlink(&home).unwrap();
            r
        };
        let mut ask = |_: &[String]| -> Result<Vec<String>, String> { unreachable!() };
        let e = generate("x", None, Registry::builtin(), &mut api, &mut ask).unwrap_err();
        assert_eq!((e.as_str(), calls), (crate::consent::NOT_LINKED, 1));
        // The fixed part of every draft fits well within the cap.
        let size = system_text(Registry::builtin()).len();
        assert!(size < crate::agents::MAX_REQUEST / 2, "{size}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn args_and_model_override() {
        let a = parse_args(&[]).unwrap();
        assert_eq!((a.out.as_str(), a.model.as_deref()), (DEFAULT_OUT, None));
        assert!(a.input.is_none());
        let a = parse_args(&["--in", "d.txt", "--model", "m2"]).unwrap();
        assert_eq!(
            (a.input.as_deref(), a.model.as_deref()),
            (Some("d.txt"), Some("m2"))
        );
        assert!(parse_args(&["--out"]).is_err());
        assert!(parse_args(&["--bogus"]).is_err());
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

        let system = system_text(&reg);
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
}
