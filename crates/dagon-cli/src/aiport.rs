//! `dagon ai port`: an agent ports DAGON to a board on its own, in the
//! DAGON clone the project builds with.
//!
//! dagon starts the program in the clone's `tools/agent-sdk` (Node.js, the
//! Claude Agent SDK). It loads the clone's `plugin/` and runs its porting
//! skill until the conformance tests pass or the skill's limits are
//! reached. The request goes in on stdin and progress comes back on
//! stdout, one JSON object per line (`tools/agent-sdk/protocol.mjs`).
//!
//! Trust boundary: unlike every other AI feature, this agent acts. It
//! reads and edits files and runs commands in the clone with the user's
//! permissions, and what it reads there goes to the agent's provider. So
//! the user is asked before every run, the linked way must be the Claude
//! API with its key, and the consent is held for the whole run. The key is
//! set in that program's environment only and is never printed.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::{json, Value};

use crate::agents::command::{stream, Streamed};
use crate::keystore::Keys;

/// The program's folder in a DAGON clone, its script and what it needs
/// installed.
pub(crate) const FOLDER: &str = "tools/agent-sdk";
const SCRIPT: &str = "port.mjs";
const INSTALLED: &str = "node_modules/@anthropic-ai/claude-agent-sdk";
/// The plugin's folder in a DAGON clone.
const PLUGIN: &str = "plugin";
/// The environment variable the Agent SDK reads its key from.
const KEY_ENV: &str = crate::agents::CLAUDE_KEY_ENV;
/// What the program inherits besides the base allow-list: where the Rust
/// tools it builds with are.
const PASS_ENV: [&str; 2] = ["CARGO_HOME", "RUSTUP_HOME"];
/// The skill's own limits when none is given (hours), and what a run may
/// take beyond its limit before dagon stops it.
const HOURS: u32 = 6;
const RERUN_HOURS: u32 = 3;
const MARGIN: Duration = Duration::from_secs(15 * 60);
pub(crate) const MAX_HOURS: u32 = 48;
pub(crate) const MAX_ITERATIONS: u32 = 200;
/// Largest board description sent.
pub(crate) const MAX_DESCRIPTION: usize = 32 * 1024;
const KEY_MARK: &str = "[API key]";

/// What the user is told before a run, and agrees to.
pub(crate) const NOTICE: &str = "\
The agent ports DAGON to the board on its own. Until it ends it
- reads and edits files and runs commands in the DAGON clone, with your
  permissions and without asking;
- sends what it reads there to the Claude API, with your API key (usage is
  billed to that key);
- works in a branch and worktree of its own and never pushes.
";

/// One port: the MCU, the board, the port kind and the skill's options.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub mcu: String,
    pub board: String,
    /// `S` or `V`.
    pub kind: String,
    pub description: Option<String>,
    pub max_hours: Option<u32>,
    pub max_iterations: Option<u32>,
    pub rerun: bool,
    pub model: Option<String>,
}

impl Request {
    /// Err: why the request cannot be sent.
    pub fn check(&self) -> Result<(), String> {
        for (name, v) in [("the MCU", &self.mcu), ("the board", &self.board)] {
            if !crate::serve::api::plain(v) {
                return Err(format!(
                    "{name} must be an id (letters, digits, '-', '_' and '.', up to 32)"
                ));
            }
        }
        if !matches!(self.kind.as_str(), "S" | "V") {
            return Err("the port kind must be S or V".into());
        }
        let within = |v: Option<u32>, max| v.is_none_or(|n| (1..=max).contains(&n));
        if !within(self.max_hours, MAX_HOURS) {
            return Err(format!("the hours must be from 1 to {MAX_HOURS}"));
        }
        if !within(self.max_iterations, MAX_ITERATIONS) {
            return Err(format!("the iterations must be from 1 to {MAX_ITERATIONS}"));
        }
        if self.description.as_ref().is_some_and(|d| d.len() > MAX_DESCRIPTION) {
            return Err("the board description is longer than 32 KiB".into());
        }
        Ok(())
    }

    /// How long the run may take before dagon stops it.
    fn timeout(&self) -> Duration {
        let hours = self
            .max_hours
            .unwrap_or(if self.rerun { RERUN_HOURS } else { HOURS });
        Duration::from_secs(u64::from(hours) * 3600) + MARGIN
    }

    fn json(&self, clone: &Path) -> Value {
        json!({
            "mcu": self.mcu,
            "board": self.board,
            "kind": self.kind,
            "plugin": clone.join(PLUGIN),
            "cwd": clone,
            "description": self.description,
            "max_hours": self.max_hours,
            "max_iterations": self.max_iterations,
            "rerun": self.rerun,
            "model": self.model,
        })
    }
}

/// What the agent reports while it works.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// What it says.
    Text(String),
    /// A tool it calls, and the command, file or pattern it names.
    Tool { name: String, detail: String },
}

impl Step {
    /// One line for a terminal or a job log.
    pub fn line(&self) -> String {
        match self {
            Step::Text(t) => t.clone(),
            Step::Tool { name, detail } if detail.is_empty() => format!("[{name}]"),
            Step::Tool { name, detail } => format!("[{name}] {detail}"),
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The agent finished its run (whether the port passed is in `text`).
    pub ok: bool,
    /// The agent's final report.
    pub text: String,
    pub turns: u64,
    pub cost_usd: Option<f64>,
}

impl Outcome {
    pub fn json(&self) -> Value {
        json!({ "ok": self.ok, "text": self.text, "turns": self.turns, "cost_usd": self.cost_usd })
    }
}

/// The script of the clone's program; Err names what is missing and how
/// to get it.
pub(crate) fn script(clone: &Path) -> Result<PathBuf, String> {
    let dir = clone.join(FOLDER);
    let script = dir.join(SCRIPT);
    if !script.is_file() || !clone.join(PLUGIN).is_dir() {
        return Err(format!(
            "the DAGON clone has no {FOLDER}/{SCRIPT} or no {PLUGIN}/: porting needs a clone \
             that has them ([dagon] path in dagon.toml, or DAGON_HOME)"
        ));
    }
    if !dir.join(INSTALLED).is_dir() {
        return Err(format!(
            "the packages of {FOLDER} are not installed: run npm ci in that folder of the \
             DAGON clone"
        ));
    }
    Ok(script)
}

/// The DAGON clone of the project, or why porting cannot run there.
pub(crate) fn clone_of(cfg: &crate::config::Config) -> Result<PathBuf, String> {
    let clone = crate::dagonsrc::clone_root(&cfg.dagon, &cfg.base).ok_or(
        "porting edits DAGON's source, so the project must build with a local clone: set \
         [dagon] path in dagon.toml (it names a release by git and tag now)",
    )?;
    std::fs::canonicalize(&clone).map_err(|_| "the DAGON clone is not there".to_string())
}

/// What the program printed, folded: the steps go to `on_step`, the end
/// is kept.
#[derive(Default)]
struct Report {
    outcome: Option<Outcome>,
    error: Option<String>,
}

impl Report {
    fn take(&mut self, line: &str, key: &str, on_step: &mut dyn FnMut(Step)) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let text = |k: &str| v[k].as_str().unwrap_or("").replace(key, KEY_MARK);
        match v["event"].as_str() {
            Some("text") => on_step(Step::Text(text("text"))),
            Some("tool") => on_step(Step::Tool {
                name: text("name"),
                detail: text("detail"),
            }),
            Some("result") => {
                self.outcome = Some(Outcome {
                    ok: v["ok"] == true,
                    text: text("text"),
                    turns: v["turns"].as_u64().unwrap_or(0),
                    cost_usd: v["cost_usd"].as_f64(),
                })
            }
            Some("error") => self.error = Some(text("text")),
            _ => {}
        }
    }

    fn end(self, exited_ok: bool) -> Result<Outcome, String> {
        match (self.error, self.outcome) {
            (Some(e), _) => Err(format!("the porting program failed: {e}")),
            (None, Some(o)) => Ok(o),
            (None, None) if exited_ok => Err("the porting program ended without a result".into()),
            (None, None) => Err("the porting program failed".into()),
        }
    }
}

/// What a run starts: Node.js, the clone's script, and the key it gets.
pub(crate) struct Program<'a> {
    pub node: &'a Path,
    pub script: &'a Path,
    pub clone: &'a Path,
    pub key: &'a str,
}

/// One run of `p`, under the held consent.
pub(crate) fn run_with(
    cleared: &crate::agents::Cleared,
    p: &Program<'_>,
    req: &Request,
    cancel: &AtomicBool,
    on_step: &mut dyn FnMut(Step),
) -> Result<Outcome, String> {
    req.check()?;
    let Program {
        node,
        script,
        clone,
        key,
    } = *p;
    let args = [script.display().to_string()];
    let pass: Vec<String> = PASS_ENV.iter().map(|s| s.to_string()).collect();
    let mut report = Report::default();
    let exited_ok = stream(
        cleared,
        Streamed {
            exe: node,
            args: &args,
            dir: clone,
            pass_env: &pass,
            set_env: &[(KEY_ENV, key)],
            input: req.json(clone).to_string(),
            timeout: req.timeout(),
        },
        cancel,
        &mut |line| report.take(line, key, on_step),
    )?;
    report.end(exited_ok)
}

/// One run in `clone` with the linked way's key.
pub(crate) fn run(
    home: &Path,
    keys: &Keys,
    clone: &Path,
    req: &Request,
    cancel: &AtomicBool,
    on_step: &mut dyn FnMut(Step),
) -> Result<Outcome, String> {
    req.check()?;
    let script = script(clone)?;
    let node = crate::agents::command::on_path("node")
        .ok_or("Node.js (node) is not on PATH: porting needs it")?;
    let (cleared, key) = crate::agents::claude_api_key(home, keys)?;
    let p = Program {
        node: &node,
        script: &script,
        clone,
        key: &key,
    };
    run_with(&cleared, &p, req, cancel, on_step)
}

/// `dagon ai port`
// Reads the board description file the user names with --in.
#[allow(clippy::disallowed_methods)]
pub fn command(a: &crate::cli::AiPortArgs) -> Result<(), String> {
    let home = crate::consent::home()?;
    crate::consent::require_linked(&home)?;
    let description = match &a.input {
        Some(p) => Some(std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?),
        None => None,
    };
    let req = Request {
        mcu: a.mcu.clone(),
        board: a.board.clone(),
        kind: a.kind.clone(),
        description,
        max_hours: a.max_hours,
        max_iterations: a.max_iterations,
        rerun: a.rerun,
        model: a.model.clone(),
    };
    req.check()?;
    let cfg = crate::config::Config::load(a.config.as_deref())?;
    let clone = clone_of(&cfg)?;
    script(&clone)?;
    let listing = format!("{NOTICE}DAGON clone: {}\n", clone.display());
    if !a.yes && !crate::deploy::confirm(&listing, &mut std::io::stdin().lock()) {
        return Err("nothing was started".into());
    }
    let keys = Keys::system();
    let never = AtomicBool::new(false);
    let out = run(&home, &keys, &clone, &req, &never, &mut |s| {
        println!("{}", s.line())
    })?;
    println!("\n{}", out.text);
    match out.cost_usd {
        Some(c) => println!("{} turns, {c:.2} USD", out.turns),
        None => println!("{} turns", out.turns),
    }
    if out.ok {
        Ok(())
    } else {
        Err("the agent did not finish its run".into())
    }
}

#[cfg(test)]
// Tests set up their own scratch clone.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn req() -> Request {
        Request {
            mcu: "esp32s3".into(),
            board: "m5stopwatch".into(),
            kind: "V".into(),
            ..Request::default()
        }
    }

    /// A clone whose program is `body` (JavaScript), with the folders
    /// [`script`] looks for.
    fn clone_with(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dagon-aiport-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(FOLDER).join(INSTALLED)).unwrap();
        std::fs::create_dir_all(dir.join(PLUGIN)).unwrap();
        std::fs::write(dir.join(FOLDER).join(SCRIPT), body).unwrap();
        std::fs::canonicalize(dir).unwrap()
    }

    /// Runs the clone's program as dagon does, linked to the Claude API
    /// with key `sk-test`.
    fn run_in(clone: &Path, r: &Request, cancel: &AtomicBool) -> (Result<Outcome, String>, Vec<Step>) {
        let home = clone.join("home");
        std::fs::create_dir_all(&home).unwrap();
        crate::consent::link(&home, "claude", "api").unwrap();
        let nothing = crate::agents::Prompt {
            system: String::new(),
            user: String::new(),
            model: None,
        };
        let cleared = crate::agents::before_call(&home, &nothing).unwrap();
        let node = crate::agents::command::on_path("node").expect("the tests need node on PATH");
        let mut steps = Vec::new();
        let script = script(clone).unwrap();
        let p = Program {
            node: &node,
            script: &script,
            clone,
            key: "sk-test",
        };
        let out = run_with(&cleared, &p, r, cancel, &mut |s| steps.push(s));
        (out, steps)
    }

    const ECHO: &str = r#"
let text = "";
process.stdin.on("data", (d) => (text += d));
process.stdin.on("end", () => {
  const r = JSON.parse(text);
  const say = (o) => console.log(JSON.stringify(o));
  say({ event: "init", plugins: ["dagon"], skills: ["dagon:dagon-port"] });
  say({ event: "text", text: `port ${r.mcu} ${r.board} ${r.kind} in ${r.cwd === process.cwd()}` });
  console.log("not json");
  say({ event: "tool", name: "Bash", detail: `key ${process.env.ANTHROPIC_API_KEY}` });
  say({ event: "tool", name: "Read", detail: "" });
  say({ event: "text", text: `signing key ${process.env.DAGON_SIGNING_KEY ?? "unset"}` });
  say({ event: "result", ok: true, text: `12/12 ${r.plugin.endsWith("plugin")}`, turns: 7, cost_usd: 1.25 });
});
"#;

    #[test]
    fn a_run_sends_the_request_and_folds_the_programs_lines() {
        let clone = clone_with("echo", ECHO);
        let (out, steps) = run_in(&clone, &req(), &AtomicBool::new(false));
        assert_eq!(
            out.unwrap(),
            Outcome {
                ok: true,
                text: "12/12 true".into(),
                turns: 7,
                cost_usd: Some(1.25),
            }
        );
        let lines: Vec<String> = steps.iter().map(Step::line).collect();
        assert_eq!(
            lines,
            [
                "port esp32s3 m5stopwatch V in true",
                // The program has the key; what it prints of it is masked.
                "[Bash] key [API key]",
                "[Read]",
                "signing key unset",
            ]
        );
    }

    #[test]
    fn a_program_that_reports_an_error_or_no_result_fails_the_run() {
        let failing = r#"console.log(JSON.stringify({ event: "error", text: "no skill" })); process.exitCode = 1;"#;
        let clone = clone_with("error", failing);
        let (out, _) = run_in(&clone, &req(), &AtomicBool::new(false));
        assert_eq!(out.unwrap_err(), "the porting program failed: no skill");

        let silent = clone_with("silent", "");
        let (out, _) = run_in(&silent, &req(), &AtomicBool::new(false));
        assert_eq!(out.unwrap_err(), "the porting program ended without a result");

        let crashing = clone_with("crash", "process.exit(3);");
        let (out, _) = run_in(&crashing, &req(), &AtomicBool::new(false));
        assert_eq!(out.unwrap_err(), "the porting program failed");
    }

    #[test]
    fn a_cancelled_run_stops_the_program() {
        let clone = clone_with("cancel", "setInterval(() => {}, 1000);");
        let t0 = std::time::Instant::now();
        let (out, _) = run_in(&clone, &req(), &AtomicBool::new(true));
        assert_eq!(out.unwrap_err(), crate::agents::command::STOPPED);
        assert!(t0.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_request_that_is_not_plain_is_never_sent() {
        for bad in [
            Request { mcu: "esp32s3 --rerun".into(), ..req() },
            Request { board: "../x".into(), ..req() },
            Request { kind: "X".into(), ..req() },
            Request { max_hours: Some(0), ..req() },
            Request { max_hours: Some(MAX_HOURS + 1), ..req() },
            Request { max_iterations: Some(MAX_ITERATIONS + 1), ..req() },
            Request { description: Some("x".repeat(MAX_DESCRIPTION + 1)), ..req() },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
        assert!(req().check().is_ok());
        assert_eq!(req().timeout(), Duration::from_secs(6 * 3600 + 900));
        let rerun = Request { rerun: true, ..req() };
        assert_eq!(rerun.timeout(), Duration::from_secs(3 * 3600 + 900));
    }

    #[test]
    fn a_clone_without_the_program_or_its_packages_says_what_to_do() {
        let clone = clone_with("missing", "");
        std::fs::remove_dir_all(clone.join(FOLDER).join("node_modules")).unwrap();
        assert!(script(&clone).unwrap_err().contains("npm ci"));
        std::fs::remove_file(clone.join(FOLDER).join(SCRIPT)).unwrap();
        assert!(script(&clone).unwrap_err().contains("DAGON_HOME"));
    }
}
