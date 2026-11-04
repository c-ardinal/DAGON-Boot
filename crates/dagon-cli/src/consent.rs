//! Linking DAGON to an agent: the user's consent to send to one agent one
//! way (see [`crate::agents`]), asked once and kept in ~/.dagon/agent.json
//! until unlinked. One consent per user, not per project: the agent's login
//! or key belongs to the user's environment, "once" would otherwise mean
//! once per project, and nothing about it lands in a project folder that
//! may be committed. `dagon ai link` / `dagon ai unlink` and serve's
//! /v1/agent share this.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::agents::{AgentDef, Agents, Transport, WayDef};

/// What a linked DAGON sends and never sends; the page and the terminal
/// show these same words when asking for consent.
pub const SENDS: [&str; 3] = [
    "構成と定義の事実（ノード・リンク・部品・設定値）",
    "選んだノードやリンク",
    "確かめた結果（問題・◇ 未対応・更新の記録）",
];
pub const NEVER_SENDS: [&str; 3] = ["鍵", "コマンドとその出力の全文", "ファイルのパス"];

/// Code of the refusal every AI route gives until DAGON is linked.
pub const NOT_LINKED_CODE: &str = "AGENT_NOT_LINKED";
pub const NOT_LINKED: &str =
    "DAGON is not linked to an agent; link it first (consent is asked once)";

const FILE: &str = "agent.json";
/// agent.json holds three short fields; a larger file is not a consent.
const MAX_FILE: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Consent {
    pub agent: String,
    pub way: String,
    /// UTC, RFC 3339 (seconds).
    pub at: String,
}

fn file(home: &Path) -> PathBuf {
    home.join(".dagon").join(FILE)
}

/// The recorded consent; None when there is none, it is unreadable, holds
/// any other field, or an id or time not as [`link`] writes them. Whether
/// the agent and way still exist is [`linked`]'s question.
pub fn consent(home: &Path) -> Option<Consent> {
    let bytes = crate::paths::read_capped(&file(home), MAX_FILE).ok()??;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    if v.as_object()?.len() != 3 {
        return None;
    }
    let id = |k: &str| {
        v[k].as_str()
            .filter(|s| crate::agents::plain_id(s))
            .map(str::to_string)
    };
    Some(Consent {
        agent: id("agent")?,
        way: id("way")?,
        at: v["at"].as_str().filter(|a| is_utc_time(a))?.to_string(),
    })
}

pub fn is_linked(home: &Path) -> bool {
    consent(home).is_some()
}

/// Records consent now to send to `agent` by `way` (ids the caller checked
/// against the definitions). The error never names the file.
pub fn link(home: &Path, agent: &str, way: &str) -> Result<Consent, String> {
    let c = Consent {
        agent: agent.into(),
        way: way.into(),
        at: utc_now(),
    };
    let text = json!({ "agent": c.agent, "way": c.way, "at": c.at }).to_string();
    crate::paths::write_atomic(&file(home), text.as_bytes())
        .map_err(|_| "the consent could not be saved".to_string())?;
    Ok(c)
}

/// Removes the consent; Ok(false) when there was none.
///
/// The consent goes first, so no call passes the check from then on (a
/// call reads it again under the lock); then this waits until no call that
/// passed before is still in flight in any dagon process (they hold the
/// lock through [`hold`]), so on return nothing is being sent.
pub fn unlink(home: &Path) -> Result<bool, String> {
    let removed = match std::fs::remove_file(file(home)) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err("the consent could not be removed".into()),
    };
    if !home.join(".dagon").is_dir() {
        return Ok(removed);
    }
    let lock = lock_file(home)?;
    if !wait_lock(&lock, false, UNLINK_WAIT) {
        return Err(LOCK_UNAVAILABLE.into());
    }
    // Released explicitly: Windows may release a closed handle's lock late.
    let _ = lock.unlock();
    Ok(removed)
}

/// The one consent lock, ~/.dagon/agent.lock: every call holds it shared
/// from its check to its end, unlink takes it exclusive. Each call opens its
/// own handle, so threads of one process exclude each other as processes do
/// (`dagon ai unlink` while `dagon serve` runs); it is per home, so homes
/// never wait for each other. The OS drops a file lock when its process
/// ends, so no lock is ever left stale; waits are bounded.
const LOCK_FILE: &str = "agent.lock";
const LOCK_UNAVAILABLE: &str = "the consent lock is not available";
/// How long a call waits for another process's unlink to finish, and an
/// unlink for another process's call (the longest call is 600 s).
const CALL_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
const UNLINK_WAIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Takes the file lock (shared or exclusive), giving up after `max`.
fn wait_lock(f: &std::fs::File, shared: bool, max: std::time::Duration) -> bool {
    let until = std::time::Instant::now() + max;
    loop {
        let r = if shared {
            f.try_lock_shared()
        } else {
            f.try_lock()
        };
        match r {
            Ok(()) => return true,
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => return false,
        }
    }
}

fn lock_file(home: &Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join(".dagon").join(LOCK_FILE))
        .map_err(|_| LOCK_UNAVAILABLE.to_string())
}

/// The consent, held for one model call: from the check until it is
/// dropped, unlink waits.
pub struct Held {
    consent: Consent,
    _file: std::fs::File,
}

impl Drop for Held {
    fn drop(&mut self) {
        // Released explicitly: Windows may release a closed handle's lock late.
        let _ = self._file.unlock();
    }
}

impl Held {
    /// The agent and way the held consent is for.
    pub fn consent(&self) -> &Consent {
        &self.consent
    }
}

/// Holds the consent; Err(NOT_LINKED) when there is none.
pub fn hold(home: &Path) -> Result<Held, String> {
    if !is_linked(home) {
        return Err(NOT_LINKED.into());
    }
    let file = lock_file(home)?;
    if !wait_lock(&file, true, CALL_WAIT) {
        return Err(LOCK_UNAVAILABLE.into());
    }
    // Read again under the lock: an unlink may have just finished.
    let consent = consent(home).ok_or(NOT_LINKED)?;
    Ok(Held {
        consent,
        _file: file,
    })
}

type LinkedWay<'a> = (Consent, &'a AgentDef, &'a WayDef);

/// The consent when its agent and way are still defined.
pub fn linked<'a>(home: &Path, agents: &'a Agents) -> Option<LinkedWay<'a>> {
    let c = consent(home)?;
    let (a, w) = agents.way(&c.agent, &c.way)?;
    Some((c.clone(), a, w))
}

/// GET /v1/agent: the agents and ways (with whether each can be used here:
/// program on PATH, key set; never a key, path or command), the consent,
/// and what linking sends. `available` and `provider` describe the linked
/// way, or the default one before linking.
pub fn status(home: Option<&Path>, agents: &Agents) -> Value {
    let linked = home.and_then(|h| linked(h, agents));
    let (a, w) = linked
        .as_ref()
        .map(|(_, a, w)| (*a, *w))
        .unwrap_or_else(|| agents.default_way());
    let list: Vec<Value> = agents
        .list
        .iter()
        .map(|a| {
            let ways: Vec<Value> = a
                .ways
                .iter()
                .map(|w| {
                    json!({
                        "id": w.id,
                        "name": w.name,
                        "kind": w.transport.kind(),
                        "available": w.available(),
                    })
                })
                .collect();
            json!({ "id": a.id, "name": a.name, "provider": a.provider, "ways": ways })
        })
        .collect();
    let (da, dw) = agents.default_way();
    json!({
        "available": w.available(),
        "linked": linked.is_some(),
        "provider": { "id": a.id, "name": a.label() },
        "consent": linked.map(|(c, _, _)| json!({ "agent": c.agent, "way": c.way, "at": c.at })),
        "agents": list,
        "default": { "agent": da.id, "way": dw.id },
        "errors": agents.errors,
        "sends": SENDS,
        "never_sends": NEVER_SENDS,
    })
}

/// `dagon ai link` and `dagon ai unlink` use the user's home.
pub fn home() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "cannot find the home folder".into())
}

/// The error `dagon ai topology` and `ask` give before linking.
pub fn require_linked(home: &Path) -> Result<(), String> {
    if linked(home, &Agents::load(Some(home))).is_some() {
        Ok(())
    } else {
        Err(
            "DAGON is not linked to an agent; run dagon ai link first (consent is asked once)"
                .into(),
        )
    }
}

/// The consent text of the terminal: the page's linking dialog, worded for
/// the CLI.
fn consent_text(a: &AgentDef, w: &WayDef) -> String {
    let list = |l: &[&str]| l.join("、");
    let key = match &w.transport {
        Transport::Command(c) => format!(
            "鍵: {} の設定とログインを使います。DAGON は鍵を読みません",
            c.program
        ),
        Transport::MessagesHttp(h) => format!(
            "鍵: 環境変数 {} に置いてください。鍵は画面にも送る文にも出しません",
            h.key_env
        ),
    };
    format!(
        "DAGON と連携する\n\n\
         DAGON は連携したエージェントに構成について問い合わせて答えます。\
         送ることへの同意は、この連携のときに一度だけいただきます。\
         連携したあとの質問は確かめずに送ります。\n\n\
         送り先: {}　つなぎ方: {}\n\
         {key}\n\
         送るもの: {}\n\
         送らないもの: {}\n\n\
         連携は dagon ai unlink でいつでも外せます。外すと同意も取り消します。",
        a.label(),
        w.name,
        list(&SENDS),
        list(&NEVER_SENDS),
    )
}

const LINK_USAGE: &str = "usage: dagon ai link [--agent <id>] [--way <id>]";

/// The agent and way `dagon ai link` names (default: the first agent, its
/// first way).
fn chosen<'a>(agents: &'a Agents, args: &[&str]) -> Result<(&'a AgentDef, &'a WayDef), String> {
    let (mut agent, mut way) = (None, None);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match (*flag, it.next()) {
            ("--agent", Some(v)) => agent = Some(*v),
            ("--way", Some(v)) => way = Some(*v),
            _ => return Err(format!("unknown argument {flag}; {LINK_USAGE}")),
        }
    }
    let a = match agent {
        Some(id) => agents.list.iter().find(|a| a.id == id),
        None => agents.list.first(),
    }
    .ok_or_else(|| format!("no agent {}; {LINK_USAGE}", agent.unwrap_or_default()))?;
    let w = match way {
        Some(id) => a.ways.iter().find(|w| w.id == id),
        None => a.ways.first(),
    }
    .ok_or_else(|| {
        format!(
            "{} has no way {}; {LINK_USAGE}",
            a.id,
            way.unwrap_or_default()
        )
    })?;
    Ok((a, w))
}

/// `dagon ai link [--agent <id>] [--way <id>]`: shows what is sent and to
/// whom, and records consent on "y".
pub fn link_command(args: &[&str]) -> Result<(), String> {
    let home = home()?;
    let agents = Agents::load(Some(&home));
    for e in &agents.errors {
        eprintln!("warning: ~/.dagon/agents/{e}");
    }
    let (a, w) = chosen(&agents, args)?;
    if let Some((c, _, _)) = linked(&home, &agents) {
        if c.agent == a.id && c.way == w.id {
            println!(
                "DAGON はすでに連携しています。同意は {} に記録しました",
                c.at
            );
            return Ok(());
        }
    }
    eprintln!("{}\n", consent_text(a, w));
    eprint!("同意して連携しますか？ [y/N] ");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if !matches!(line.trim(), "y" | "Y" | "yes") {
        println!("連携しませんでした");
        return Ok(());
    }
    let c = link(&home, &a.id, &w.id)?;
    println!(
        "DAGON と連携しました。送り先 {}（{}）と送る中身への同意を {} に記録しました",
        a.label(),
        w.name,
        c.at
    );
    if !w.available() {
        println!("このつなぎ方はまだ使えません。{}", unavailable_hint(w));
    }
    Ok(())
}

fn unavailable_hint(w: &WayDef) -> String {
    match &w.transport {
        Transport::Command(c) => format!("{} を PATH から使えるようにしてください", c.program),
        Transport::MessagesHttp(h) => format!("環境変数 {} を設定してください", h.key_env),
    }
}

/// `dagon ai unlink`: removes the consent.
pub fn unlink_command(args: &[&str]) -> Result<(), String> {
    no_args(args, "unlink")?;
    if unlink(&home()?)? {
        println!(
            "DAGON との連携を外し、同意を取り消しました。鍵やログインはエージェントの側で消してください"
        );
    } else {
        println!("DAGON は連携していません");
    }
    Ok(())
}

fn no_args(args: &[&str], cmd: &str) -> Result<(), String> {
    match args.first() {
        None => Ok(()),
        Some(a) => Err(format!("unknown argument {a}; usage: dagon ai {cmd}")),
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` with a real month, day and time of day.
fn is_utc_time(s: &str) -> bool {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let t = s.get(r)?;
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())?
    };
    let shape = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z';
    shape
        && num(0..4).is_some()
        && num(5..7).is_some_and(|m| (1..=12).contains(&m))
        && num(8..10).is_some_and(|d| {
            let (y, m) = (num(0..4).unwrap_or(0), num(5..7).unwrap_or(0));
            let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let days = match m {
                2 if leap => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            (1..=days).contains(&d)
        })
        && num(11..13).is_some_and(|h| h < 24)
        && num(14..16).is_some_and(|m| m < 60)
        && num(17..19).is_some_and(|x| x < 60)
}

fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

/// UTC date and time of a Unix time (civil-from-days, H. Hinnant).
fn rfc3339(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn utc_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_793_793_599), "2026-11-04T11:59:59Z");
        assert!(is_utc_time(&rfc3339(1_793_793_599)) && is_utc_time(&utc_now()));
        for bad in [
            "2026-11-04 11:59:59Z",
            "2026-11-04T24:00:00Z",
            "2026-11-04T11:59:59",
            "２026-11-04T11:59:59Z",
            "2026-02-31T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
        ] {
            assert!(!is_utc_time(bad), "{bad}");
        }
        assert!(is_utc_time("2028-02-29T00:00:00Z") && is_utc_time("2000-02-29T00:00:00Z"));
    }

    #[test]
    fn link_records_once_and_unlink_removes() {
        let h = tmp_home("cycle");
        assert!(!is_linked(&h));
        assert!(require_linked(&h).is_err());
        let agents = Agents::load(None);
        let s = status(Some(&h), &agents);
        assert_eq!(s["linked"], false);
        // Before linking: the default way, Claude Code.
        assert_eq!(
            s["default"],
            json!({ "agent": "claude", "way": "claude-code" })
        );
        assert_eq!(
            s["provider"],
            json!({ "id": "claude", "name": "Claude (Anthropic)" })
        );
        let c = link(&h, "claude", "api").unwrap();
        assert_eq!(consent(&h), Some(c.clone()));
        let s = status(Some(&h), &agents);
        assert_eq!(s["linked"], true);
        assert_eq!(
            s["consent"],
            json!({ "agent": "claude", "way": "api", "at": c.at })
        );
        let ways = &s["agents"][0]["ways"];
        assert_eq!(ways[0]["id"], "claude-code");
        assert_eq!(ways[0]["kind"], "command");
        assert_eq!(ways[1]["kind"], "messages-http");
        assert!(ways[0]["available"].is_boolean() && ways[1]["available"].is_boolean());
        // Never a program, path, argument or key variable.
        for never in [
            "program",
            "--tools",
            "ANTHROPIC_API_KEY",
            "https://",
            "key_env",
        ] {
            assert!(!s["agents"].to_string().contains(never), "{never}");
        }
        // A consent for a way that is not defined is no link.
        link(&h, "claude", "gone").unwrap();
        assert!(linked(&h, &agents).is_none() && require_linked(&h).is_err());
        assert!(unlink(&h).unwrap());
        assert!(!unlink(&h).unwrap());
        assert_eq!(status(Some(&h), &agents)["consent"], Value::Null);
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn unlink_waits_for_a_call_in_flight_and_no_call_passes_after_it() {
        let h = tmp_home("hold");
        assert_eq!(hold(&h).err().as_deref(), Some(NOT_LINKED));
        link(&h, "claude", "api").unwrap();
        let held = hold(&h).unwrap();
        assert_eq!(
            (held.consent().agent.as_str(), held.consent().way.as_str()),
            ("claude", "api")
        );
        let (done_tx, done) = std::sync::mpsc::channel();
        let h2 = h.clone();
        let t = std::thread::spawn(move || {
            let r = unlink(&h2);
            done_tx.send(()).unwrap();
            r
        });
        // The consent goes at once: no new call passes the check...
        let t0 = std::time::Instant::now();
        while is_linked(&h) && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!is_linked(&h));
        assert_eq!(hold(&h).err().as_deref(), Some(NOT_LINKED));
        // ...but unlink returns only once the call in flight has ended.
        let waited = done.recv_timeout(std::time::Duration::from_millis(300));
        assert!(
            waited.is_err(),
            "unlink returned while a call was in flight"
        );
        drop(held);
        done.recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(t.join().unwrap().unwrap());
        assert_eq!(hold(&h).err().as_deref(), Some(NOT_LINKED));
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn unlink_waits_for_another_processs_call_through_the_lock_file() {
        let h = tmp_home("file-lock");
        link(&h, "claude", "api").unwrap();
        // What a call in another dagon holds: the shared file lock only.
        let other = lock_file(&h).unwrap();
        other.lock_shared().unwrap();
        let (done_tx, done) = std::sync::mpsc::channel();
        let h2 = h.clone();
        let t = std::thread::spawn(move || {
            let r = unlink(&h2);
            done_tx.send(()).unwrap();
            r
        });
        let waited = done.recv_timeout(std::time::Duration::from_millis(300));
        assert!(waited.is_err(), "unlink did not wait for the file lock");
        drop(other);
        done.recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(t.join().unwrap().unwrap());
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn another_provider_or_a_broken_file_is_no_consent() {
        let h = tmp_home("broken");
        std::fs::create_dir_all(h.join(".dagon")).unwrap();
        for text in [
            "",
            "{",
            // The round 1 form (one provider) asks for consent again.
            r#"{"provider":"anthropic","at":"2026-11-04T04:14:35Z"}"#,
            r#"{"agent":"claude","way":"api"}"#,
            r#"{"agent":"claude","way":"api","at":"yesterday"}"#,
            r#"{"agent":"claude","way":"api","at":"2026-13-04T04:14:35Z"}"#,
            r#"{"agent":"Claude!","way":"api","at":"2026-11-04T04:14:35Z"}"#,
            r#"{"agent":"claude","way":"api","at":"2026-11-04T04:14:35Z","scope":"all"}"#,
        ] {
            std::fs::write(file(&h), text).unwrap();
            assert!(!is_linked(&h), "{text}");
        }
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn the_terminal_shows_the_same_lists_as_the_page() {
        let agents = Agents::load(None);
        let (a, cc) = agents.way("claude", "claude-code").unwrap();
        let (_, api) = agents.way("claude", "api").unwrap();
        for (w, key) in [(cc, "claude の設定とログイン"), (api, "ANTHROPIC_API_KEY")] {
            let t = consent_text(a, w);
            for s in SENDS.iter().chain(&NEVER_SENDS) {
                assert!(t.contains(s), "{s}");
            }
            assert!(t.contains("Claude (Anthropic)") && t.contains(&w.name) && t.contains(key));
        }
        assert_eq!(chosen(&agents, &[]).unwrap().1.id, "claude-code");
        assert_eq!(chosen(&agents, &["--way", "api"]).unwrap().1.id, "api");
        assert!(chosen(&agents, &["--agent", "other"]).is_err());
        assert!(chosen(&agents, &["--way"]).is_err());
    }
}
