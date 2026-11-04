//! Linking DAGON to an agent: the user's consent to send, asked once and
//! kept in ~/.dagon/agent.json until unlinked. One consent per user, not per
//! project: the key it goes with (ANTHROPIC_API_KEY) belongs to the user's
//! environment, "once" would otherwise mean once per project, and nothing
//! about it lands in a project folder that may be committed.
//! `dagon ai link` / `dagon ai unlink` and serve's /v1/agent share this.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock, RwLockReadGuard};

use serde_json::{json, Value};

/// The one agent DAGON talks to (no other implementation exists).
pub const PROVIDER_ID: &str = "anthropic";
pub const PROVIDER_NAME: &str = "Claude (Anthropic)";

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
/// agent.json holds two short fields; a larger file is not a consent.
const MAX_FILE: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Consent {
    pub provider: String,
    /// UTC, RFC 3339 (seconds).
    pub at: String,
}

fn file(home: &Path) -> PathBuf {
    home.join(".dagon").join(FILE)
}

/// The recorded consent; None when there is none, it is unreadable, holds
/// any other field, names another provider or a time not as [`link`]
/// writes it.
pub fn consent(home: &Path) -> Option<Consent> {
    let bytes = crate::paths::read_capped(&file(home), MAX_FILE).ok()??;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    if v.as_object()?.len() != 2 {
        return None;
    }
    let provider = v["provider"].as_str().filter(|p| *p == PROVIDER_ID)?;
    let at = v["at"].as_str().filter(|a| is_utc_time(a))?;
    Some(Consent {
        provider: provider.into(),
        at: at.into(),
    })
}

pub fn is_linked(home: &Path) -> bool {
    consent(home).is_some()
}

/// Records consent now. The error never names the file (serve passes it on).
pub fn link(home: &Path) -> Result<Consent, String> {
    let c = Consent {
        provider: PROVIDER_ID.into(),
        at: utc_now(),
    };
    let text = json!({ "provider": c.provider, "at": c.at }).to_string();
    crate::paths::write_atomic(&file(home), text.as_bytes())
        .map_err(|_| "the consent could not be saved".to_string())?;
    Ok(c)
}

/// Removes the consent; Ok(false) when there was none.
///
/// Waits until no model call is in flight in any dagon process (they hold
/// the consent through [`hold`]); no call passes the check afterwards.
pub fn unlink(home: &Path) -> Result<bool, String> {
    if !file(home).exists() {
        return Ok(false);
    }
    let _calls = CONSENT.write().unwrap_or_else(PoisonError::into_inner);
    let lock = lock_file(home)?;
    lock.lock().map_err(|_| LOCK_UNAVAILABLE.to_string())?;
    match std::fs::remove_file(file(home)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("the consent could not be removed".into()),
    }
}

/// One consent lock: model calls of this process share the read side,
/// unlink takes the write side; ~/.dagon/agent.lock does the same between
/// processes (`dagon ai unlink` while `dagon serve` runs). The OS drops a
/// file lock when its process ends, so no lock is ever left stale.
static CONSENT: RwLock<()> = RwLock::new(());
const LOCK_FILE: &str = "agent.lock";
const LOCK_UNAVAILABLE: &str = "the consent lock is not available";

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
    _file: std::fs::File,
    _calls: RwLockReadGuard<'static, ()>,
}

/// Holds the consent; Err(NOT_LINKED) when there is none.
pub fn hold(home: &Path) -> Result<Held, String> {
    if !is_linked(home) {
        return Err(NOT_LINKED.into());
    }
    let calls = CONSENT.read().unwrap_or_else(PoisonError::into_inner);
    let file = lock_file(home)?;
    file.lock_shared()
        .map_err(|_| LOCK_UNAVAILABLE.to_string())?;
    // Checked again under the lock: an unlink may have just finished.
    if !is_linked(home) {
        return Err(NOT_LINKED.into());
    }
    Ok(Held {
        _file: file,
        _calls: calls,
    })
}

/// GET /v1/agent. `available`: this CLI has the key (never the key itself).
pub fn status(home: Option<&Path>, available: bool) -> Value {
    let c = home.and_then(consent);
    json!({
        "available": available,
        "linked": c.is_some(),
        "provider": { "id": PROVIDER_ID, "name": PROVIDER_NAME },
        "consent": c.map(|c| json!({ "at": c.at, "provider": c.provider })),
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

/// The error `dagon ai topology` gives before linking.
pub fn require_linked(home: &Path) -> Result<(), String> {
    if is_linked(home) {
        Ok(())
    } else {
        Err(
            "DAGON is not linked to an agent; run dagon ai link first (consent is asked once)"
                .into(),
        )
    }
}

/// The consent text of the terminal: the page's linking dialog, worded for
/// the CLI (the key is this environment's, not the page's).
fn consent_text() -> String {
    let list = |l: &[&str]| l.join("、");
    format!(
        "DAGON と連携する\n\n\
         DAGON は連携したエージェントに構成について問い合わせて答えます。\
         送ることへの同意は、この連携のときに一度だけいただきます。\
         連携したあとの質問は確かめずに送ります。\n\n\
         送り先: {PROVIDER_NAME}　今使えるのはこの 1 つです\n\
         鍵: 環境変数 {key} に置いてください。鍵は画面にも送る文にも出しません\n\
         送るもの: {}\n\
         送らないもの: {}\n\n\
         連携は dagon ai unlink でいつでも外せます。外すと同意も取り消します。",
        list(&SENDS),
        list(&NEVER_SENDS),
        key = crate::ai::KEY_ENV,
    )
}

/// `dagon ai link`: shows what is sent and records consent on "y".
pub fn link_command(args: &[&str]) -> Result<(), String> {
    no_args(args, "link")?;
    let home = home()?;
    if let Some(c) = consent(&home) {
        println!(
            "DAGON はすでに連携しています。同意は {} に記録しました",
            c.at
        );
        return Ok(());
    }
    eprintln!("{}\n", consent_text());
    eprint!("同意して連携しますか？ [y/N] ");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if !matches!(line.trim(), "y" | "Y" | "yes") {
        println!("連携しませんでした");
        return Ok(());
    }
    let c = link(&home)?;
    println!(
        "DAGON と連携しました。送り先 {PROVIDER_NAME} と送る中身への同意を {} に記録しました",
        c.at
    );
    if !crate::ai::api_key_present() {
        println!(
            "{} が設定されていません。設定するまで DAGON は答えられません",
            crate::ai::KEY_ENV
        );
    }
    Ok(())
}

/// `dagon ai unlink`: removes the consent.
pub fn unlink_command(args: &[&str]) -> Result<(), String> {
    no_args(args, "unlink")?;
    if unlink(&home()?)? {
        println!(
            "DAGON との連携を外し、同意を取り消しました。鍵は環境変数 {} から消してください",
            crate::ai::KEY_ENV
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
        let c = link(&h).unwrap();
        assert_eq!(consent(&h), Some(c.clone()));
        let s = status(Some(&h), false);
        assert_eq!(s["linked"], true);
        assert_eq!(s["consent"]["at"], c.at.as_str());
        assert_eq!(s["consent"]["provider"], PROVIDER_ID);
        assert!(unlink(&h).unwrap());
        assert!(!unlink(&h).unwrap());
        assert_eq!(status(Some(&h), true)["consent"], Value::Null);
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn unlink_waits_for_a_call_in_flight_and_no_call_passes_after_it() {
        let h = tmp_home("hold");
        assert_eq!(hold(&h).err().as_deref(), Some(NOT_LINKED));
        link(&h).unwrap();
        let held = hold(&h).unwrap();
        let (done_tx, done) = std::sync::mpsc::channel();
        let h2 = h.clone();
        let t = std::thread::spawn(move || {
            let r = unlink(&h2);
            done_tx.send(()).unwrap();
            r
        });
        let waited = done.recv_timeout(std::time::Duration::from_millis(300));
        assert!(
            waited.is_err(),
            "unlink returned while a call was in flight"
        );
        assert!(is_linked(&h));
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
        link(&h).unwrap();
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
            r#"{"provider":"other","at":"2026-11-04T04:14:35Z"}"#,
            r#"{"provider":"anthropic"}"#,
            r#"{"provider":"anthropic","at":"yesterday"}"#,
            r#"{"provider":"anthropic","at":"2026-13-04T04:14:35Z"}"#,
            r#"{"provider":"anthropic","at":"2026-11-04T04:14:35Z","scope":"all"}"#,
        ] {
            std::fs::write(file(&h), text).unwrap();
            assert!(!is_linked(&h), "{text}");
        }
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn the_terminal_shows_the_same_lists_as_the_page() {
        let t = consent_text();
        for s in SENDS.iter().chain(&NEVER_SENDS) {
            assert!(t.contains(s), "{s}");
        }
        assert!(t.contains(PROVIDER_NAME) && t.contains(crate::ai::KEY_ENV));
    }
}
