//! The agents DAGON talks to, described by data: an agent (id, name,
//! provider) and the ways to reach it, each of a transport kind DAGON
//! implements (`command`, `messages-http`). The builtin set ships with
//! dagon (`agents/*.toml`); the user may add files in ~/.dagon/agents/
//! (never a project: a way may name a program to run). Every AI feature
//! talks through [`call`]: one prompt (system text, user text) in, the
//! agent's text out, behind the same guards whatever the agent.

pub(crate) mod command;
pub(crate) mod http;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

const BUILTIN: [(&str, &str); 1] = [("claude.toml", include_str!("../../agents/claude.toml"))];
/// ~/.dagon/agents/*.toml: the user's own agents and ways.
const USER_DIR: &str = "agents";
const MAX_DEF_FILE: usize = 64 * 1024;
const MAX_ID: usize = 32;
const MAX_NAME: usize = 64;

/// Largest prompt sent (system and user text together); a larger one is
/// not sent.
pub(crate) const MAX_REQUEST: usize = 192 * 1024;
pub(crate) const TOO_LARGE: &str = "the question and its context are too large to send to DAGON";
/// The error of a call that did not finish within its time.
pub(crate) const TIMED_OUT: &str = "DAGON did not answer in time";
pub(crate) const UNEXPECTED: &str = "DAGON's agent did not answer in the expected form";

/// The `code` of every agent error the page tells apart (the serve API
/// contract; docs/agent-api.md lists exactly these, a test checks it).
pub mod codes {
    /// Declares each code once: its constant, and its place in [`ALL`].
    macro_rules! codes {
        ($($name:ident = $code:literal),* $(,)?) => {
            $(pub const $name: &str = $code;)*
            /// Every code, in declaration order.
            #[cfg(test)]
            pub const ALL: &[&str] = &[$($code),*];
        };
    }
    codes! {
        NOT_LINKED = "AGENT_NOT_LINKED",
        CONSENT_VERSION = "AGENT_CONSENT_VERSION",
        UNAVAILABLE = "AGENT_UNAVAILABLE",
        BUSY = "AGENT_BUSY",
        CONTEXT_TOO_LARGE = "AGENT_CONTEXT_TOO_LARGE",
        TIMEOUT = "AGENT_TIMEOUT",
        FAILED = "AGENT_FAILED",
    }
}

/// The agent definition format this dagon reads (`dagon_agent = 1`);
/// another number is refused with a clear reason.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDef {
    /// The definition format, [`FORMAT`].
    pub dagon_agent: u32,
    pub id: String,
    pub name: String,
    pub provider: String,
    pub ways: Vec<WayDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WayDef {
    pub id: String,
    pub name: String,
    pub transport: Transport,
}

/// The closed set of transport kinds.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Transport {
    Command(command::Command),
    MessagesHttp(http::MessagesHttp),
}

/// The name of every transport kind, as definitions write it; a new kind
/// needs its name here before [`Transport::kind`] compiles.
pub const KINDS: [&str; 2] = ["command", "messages-http"];

impl Transport {
    pub fn kind(&self) -> &'static str {
        match self {
            Transport::Command(_) => KINDS[0],
            Transport::MessagesHttp(_) => KINDS[1],
        }
    }

    fn check(&self) -> Result<(), String> {
        match self {
            Transport::Command(c) => c.check(),
            Transport::MessagesHttp(h) => h.check(),
        }
    }
}

impl WayDef {
    /// Whether this way can be used here now: the program is on PATH, or
    /// the key's environment variable is set (never its value).
    pub fn available(&self) -> bool {
        match &self.transport {
            Transport::Command(c) => c.program_path().is_some(),
            Transport::MessagesHttp(h) => h.key().is_ok(),
        }
    }
}

impl AgentDef {
    /// "Claude (Anthropic)": the name the linking texts show.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.provider)
    }
}

/// Lower-case letters, digits and '-', up to 32.
pub fn plain_id(s: &str) -> bool {
    (1..=MAX_ID).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn plain_name(s: &str) -> bool {
    (1..=MAX_NAME).contains(&s.chars().count()) && !s.chars().any(char::is_control)
}

fn check(def: &AgentDef) -> Result<(), String> {
    if def.dagon_agent != FORMAT {
        return Err(format!("dagon_agent must be {FORMAT}"));
    }
    if !plain_id(&def.id) {
        return Err("id must be 1 to 32 of a-z 0-9 -".into());
    }
    if !plain_name(&def.name) || !plain_name(&def.provider) {
        return Err("name and provider must be 1 to 64 characters".into());
    }
    if def.ways.is_empty() {
        return Err("an agent needs at least one way".into());
    }
    for (i, w) in def.ways.iter().enumerate() {
        if !plain_id(&w.id) || def.ways[..i].iter().any(|o| o.id == w.id) {
            return Err(format!(
                "way {}: ids must be unique, 1 to 32 of a-z 0-9 -",
                i + 1
            ));
        }
        if !plain_name(&w.name) {
            return Err(format!("way {}: name must be 1 to 64 characters", w.id));
        }
        w.transport
            .check()
            .map_err(|e| format!("way {}: {e}", w.id))?;
    }
    Ok(())
}

fn parse(text: &str) -> Result<AgentDef, String> {
    // The number first, so a newer file says why instead of failing on a key.
    let n = toml::from_str::<toml::Table>(text)
        .map_err(|e| e.message().to_string())?
        .get("dagon_agent")
        .and_then(toml::Value::as_integer);
    if n != Some(i64::from(FORMAT)) {
        return Err(format!(
            "dagon_agent = {} is not a format this dagon reads (it reads {FORMAT})",
            n.map_or("(missing)".to_string(), |n| n.to_string())
        ));
    }
    let def: AgentDef = toml::from_str(text).map_err(|e| e.message().to_string())?;
    check(&def)?;
    Ok(def)
}

/// The agents in use: the builtin ones, then the user's.
#[derive(Debug, Clone)]
pub struct Agents {
    pub list: Vec<AgentDef>,
    /// "<file name>: <why>" of user files that did not load (no folders).
    pub errors: Vec<String>,
}

impl Agents {
    /// The builtin agents and, with `home`, those in ~/.dagon/agents/.
    pub fn load(home: Option<&Path>) -> Agents {
        let mut list: Vec<AgentDef> = BUILTIN
            .iter()
            .map(|(name, text)| parse(text).unwrap_or_else(|e| panic!("builtin {name}: {e}")))
            .collect();
        let mut errors = Vec::new();
        for (name, text) in home.map(user_files).unwrap_or_default() {
            let r = text.and_then(|t| parse(&t)).and_then(|d| {
                if list.iter().any(|o| o.id == d.id) {
                    Err(format!("agent {} is defined already", d.id))
                } else {
                    Ok(d)
                }
            });
            match r {
                Ok(d) => list.push(d),
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        Agents { list, errors }
    }

    pub fn way(&self, agent: &str, way: &str) -> Option<(&AgentDef, &WayDef)> {
        let a = self.list.iter().find(|a| a.id == agent)?;
        a.ways.iter().find(|w| w.id == way).map(|w| (a, w))
    }

    /// The first way of the first agent.
    pub fn default_way(&self) -> (&AgentDef, &WayDef) {
        let a = &self.list[0];
        (a, &a.ways[0])
    }
}

/// (file name, its text or why it cannot be read), sorted by name.
fn user_files(home: &Path) -> Vec<(String, Result<String, String>)> {
    let Ok(rd) = std::fs::read_dir(home.join(".dagon").join(USER_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Result<String, String>)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".toml").then(|| {
                let text = match crate::paths::read_capped(&e.path(), MAX_DEF_FILE) {
                    Ok(Some(b)) => String::from_utf8(b).map_err(|_| "not UTF-8".to_string()),
                    Ok(None) => Err("larger than 64 KiB".into()),
                    Err(_) => Err("cannot be read".into()),
                };
                (name, text)
            })
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Environment variables a child process of dagon never sees: the signing
/// key's and every key an agent way names.
pub fn secret_env(home: Option<&Path>) -> Vec<String> {
    let mut out = vec![crate::config::KEY_ENV.to_string()];
    for a in Agents::load(home).list {
        for w in a.ways {
            if let Transport::MessagesHttp(h) = w.transport {
                out.push(h.key_env);
            }
        }
    }
    out
}

/// What every AI feature sends: the same two texts whatever the way.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub system: String,
    pub user: String,
    /// A model for ways that pick one (`dagon ai topology --model`).
    pub model: Option<String>,
}

/// One call of an agent: a prompt in, its text out.
pub(crate) type Api<'a> = dyn FnMut(&Prompt) -> Result<String, String> + 'a;

/// The consent held for one call, from the check until the call ends
/// (unlink waits for it), and which agent and way it is for. Only
/// [`before_call`] makes one; the transports take it.
pub(crate) struct Cleared {
    held: crate::consent::Held,
}

impl Cleared {
    pub fn consent(&self) -> &crate::consent::Consent {
        self.held.consent()
    }
}

/// What every call passes just before it leaves: the prompt's size, and
/// the consent, read again from agent.json and held until the call ends.
pub(crate) fn before_call(home: &Path, prompt: &Prompt) -> Result<Cleared, String> {
    if prompt.system.len() + prompt.user.len() > MAX_REQUEST {
        return Err(TOO_LARGE.into());
    }
    Ok(Cleared {
        held: crate::consent::hold(home)?,
    })
}

/// The linked agent, reached the linked way: the one call of every AI
/// feature.
pub(crate) fn call(home: &Path, prompt: &Prompt, timeout: Duration) -> Result<String, String> {
    let cleared = before_call(home, prompt)?;
    let agents = Agents::load(Some(home));
    let c = cleared.consent();
    let (_, way) = agents
        .way(&c.agent, &c.way)
        .ok_or(crate::consent::NOT_LINKED)?;
    match &way.transport {
        Transport::Command(cmd) => command::run(&cleared, cmd, prompt, timeout),
        Transport::MessagesHttp(h) => http::post(&cleared, h, prompt, timeout),
    }
}

/// The linked way's problem before any call: Err(why) when it cannot be
/// used here (no program, no key).
pub(crate) fn ready(home: &Path) -> Result<(), String> {
    let Some(c) = crate::consent::consent(home) else {
        return Err(crate::consent::NOT_LINKED.into());
    };
    let agents = Agents::load(Some(home));
    let (_, way) = agents
        .way(&c.agent, &c.way)
        .ok_or(crate::consent::NOT_LINKED)?;
    match &way.transport {
        Transport::Command(cmd) => cmd.program_path().map(|_| ()).ok_or_else(|| {
            format!(
                "the program of the linked way ({}) is not on PATH",
                cmd.program
            )
        }),
        Transport::MessagesHttp(h) => h.key().map(|_| ()),
    }
}

/// The instruction every prompt ends its system text with: the reply is
/// one JSON object of `schema`.
pub(crate) fn reply_rule(schema: &Value) -> String {
    format!(
        "Reply with one JSON object and nothing else (no other text, no code fence), \
         valid against this JSON schema:\n{schema}"
    )
}

/// A conversation as the user text: one turn as it is, several as turns
/// from DAGON and from you, oldest first.
pub(crate) fn conversation(turns: &[String]) -> String {
    if let [only] = turns {
        return only.clone();
    }
    turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let from = if i % 2 == 0 { "dagon" } else { "you" };
            format!("<turn from=\"{from}\">\n{t}\n</turn>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The JSON object an agent replied with (a code fence or words around it
/// are tolerated).
pub(crate) fn reply_json(text: &str) -> Result<Value, String> {
    let start = text.find('{').ok_or(UNEXPECTED)?;
    let end = text.rfind('}').ok_or(UNEXPECTED)?;
    if end < start {
        return Err(UNEXPECTED.into());
    }
    let v: Value = serde_json::from_str(&text[start..=end]).map_err(|_| UNEXPECTED.to_string())?;
    if v.is_object() {
        Ok(v)
    } else {
        Err(UNEXPECTED.into())
    }
}

/// A fresh empty folder for one command call (removed by the caller).
pub(crate) fn temp_dir() -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let d = std::env::temp_dir().join(format!(
        "dagon-agent-{}-{}-{nanos}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&d).map_err(|_| "cannot make a folder for the agent".to_string())?;
    Ok(d)
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests;
