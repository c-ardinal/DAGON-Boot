//! Agent API keys in the OS credential store: Windows Credential Manager,
//! the macOS Keychain, the Secret Service on Linux and other Unix desktops
//! (keyring-core with each platform's native store). A key is read only
//! from there and leaves it only into a request header: never an
//! environment variable, a file, a log, an error, a prompt or the page.
//! Where no store can be used, registering is refused with why; there is
//! no file fallback.
//!
//! An entry is service [`SERVICE`], account `<agent id>/<way id>`.

use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Error};

use crate::agents::{AgentDef, Agents, WayDef};

/// The service every agent key is stored under.
pub const SERVICE: &str = "dagon";
/// The longest key taken. Windows Credential Manager holds at most 2560
/// bytes and the key is stored as UTF-16 (2 bytes a character).
pub const MAX_KEY: usize = 1024;

/// The store the keys are in, or why there is none.
#[derive(Clone)]
pub struct Keys(Result<Arc<CredentialStore>, String>);

/// The store's name in reasons.
#[cfg(target_os = "windows")]
const STORE: &str = "Windows Credential Manager";
#[cfg(target_os = "macos")]
const STORE: &str = "the macOS Keychain";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const STORE: &str = "the Secret Service";

/// What to do when the store cannot be used.
#[cfg(any(target_os = "windows", target_os = "macos"))]
const HOW: &str = "sign in to your desktop session and try again";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const HOW: &str = "install and start a Secret Service provider in your desktop session \
                   (for example GNOME Keyring, or KeePassXC with its Secret Service \
                   integration on), then try again";

fn unavailable(detail: &str) -> String {
    format!("{STORE} cannot be used ({detail}); {HOW}. DAGON keeps API keys nowhere else")
}

#[cfg(target_os = "windows")]
fn native() -> keyring_core::Result<Arc<CredentialStore>> {
    let s: Arc<CredentialStore> = windows_native_keyring_store::Store::new()?;
    Ok(s)
}

#[cfg(target_os = "macos")]
fn native() -> keyring_core::Result<Arc<CredentialStore>> {
    let s: Arc<CredentialStore> = apple_native_keyring_store::keychain::Store::new()?;
    Ok(s)
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn native() -> keyring_core::Result<Arc<CredentialStore>> {
    let s: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new()?;
    Ok(s)
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )
)))]
fn native() -> keyring_core::Result<Arc<CredentialStore>> {
    Err(Error::NotSupportedByStore(
        "no credential store on this OS".into(),
    ))
}

/// A store error as a reason; never the stored value (keyring-core's
/// BadEncoding carries the bytes, so no error is shown as it is).
fn why(e: Error) -> String {
    match e {
        Error::NoStorageAccess(err) | Error::PlatformFailure(err) => unavailable(&err.to_string()),
        Error::NoDefaultStore | Error::NotSupportedByStore(_) => unavailable("not found"),
        Error::BadEncoding(_) | Error::BadDataFormat(..) => {
            "the stored key cannot be read; register it again".into()
        }
        _ => format!("{STORE} refused the entry"),
    }
}

impl Keys {
    /// The OS store. Found once per process; when it cannot be used, why
    /// (asked again next time, so a store started later is found).
    pub fn os() -> Keys {
        static FOUND: OnceLock<Arc<CredentialStore>> = OnceLock::new();
        if let Some(s) = FOUND.get() {
            return Keys(Ok(s.clone()));
        }
        match native() {
            Ok(s) => Keys(Ok(FOUND.get_or_init(|| s).clone())),
            Err(e) => Keys(Err(why(e))),
        }
    }

    /// The store dagon uses: the OS store. Unit tests get a fresh empty
    /// in-memory store instead, so no test reads or writes the user's own
    /// store unless it asks for [`Keys::os`].
    pub fn system() -> Keys {
        #[cfg(test)]
        return Keys::double();
        #[cfg(not(test))]
        Keys::os()
    }

    /// An empty in-memory store (keyring-core's mock).
    #[cfg(test)]
    pub fn double() -> Keys {
        Keys(Ok(keyring_core::mock::Store::new().expect("mock store")))
    }

    /// A store that cannot be used, for `why`.
    #[cfg(test)]
    pub fn unavailable(why: &str) -> Keys {
        Keys(Err(unavailable(why)))
    }

    fn entry(&self, agent: &str, way: &str) -> Result<keyring_core::Entry, String> {
        let store = self.0.as_ref().map_err(Clone::clone)?;
        store
            .build(SERVICE, &format!("{agent}/{way}"), None)
            .map_err(why)
    }

    /// The key of `agent`'s `way`; None when none is registered.
    pub fn get(&self, agent: &str, way: &str) -> Result<Option<String>, String> {
        match self.entry(agent, way)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(why(e)),
        }
    }

    /// Whether a key is registered for the way (false when the store
    /// cannot be used).
    pub fn is_set(&self, agent: &str, way: &str) -> bool {
        matches!(self.get(agent, way), Ok(Some(_)))
    }

    /// Registers `key` (checked by [`check_key`]) for the way, replacing
    /// the one before.
    pub fn set(&self, agent: &str, way: &str, key: &str) -> Result<(), String> {
        let key = check_key(key)?;
        self.entry(agent, way)?.set_password(key).map_err(why)
    }

    /// Removes the way's key; Ok(false) when none was registered.
    pub fn delete(&self, agent: &str, way: &str) -> Result<bool, String> {
        match self.entry(agent, way)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(Error::NoEntry) => Ok(false),
            Err(e) => Err(why(e)),
        }
    }
}

/// The key without surrounding blanks, or why it is refused. It goes into
/// an HTTP header, so only visible ASCII (RFC 9110 field values) is taken.
pub fn check_key(key: &str) -> Result<&str, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("the key is empty".into());
    }
    if k.len() > MAX_KEY {
        return Err(format!("the key is longer than {MAX_KEY} characters"));
    }
    if !k.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("the key may hold only visible ASCII characters, no spaces".into());
    }
    Ok(k)
}

const USAGE: &str =
    "usage: dagon ai key set|status|delete [--agent <id>] [--way <id>] (status lists every key way)";

/// The way `--agent` / `--way` name; default: the first agent, its first
/// way that takes a key.
fn chosen<'a>(agents: &'a Agents, args: &[&str]) -> Result<(&'a AgentDef, &'a WayDef), String> {
    let (a, w) = crate::consent::chosen(agents, args, USAGE, |w| w.transport.takes_key())?;
    if !w.transport.takes_key() {
        return Err(format!("{}/{} takes no key; {USAGE}", a.id, w.id));
    }
    Ok((a, w))
}

/// `dagon ai key set|status|delete`.
pub fn command(args: &[&str]) -> Result<(), String> {
    let home = crate::consent::home()?;
    let agents = Agents::load(Some(&home));
    let keys = Keys::system();
    match args {
        ["set", rest @ ..] => {
            let (a, w) = chosen(&agents, rest)?;
            // Refused before anything is typed: no store, or no terminal to type into.
            keys.0.as_ref().map_err(Clone::clone)?;
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                return Err(
                    "the key is typed in a terminal (without echo); run this in one".into(),
                );
            }
            let prompt = format!("{} の {} のキー（表示されません）: ", a.name, w.name);
            let key = rpassword::prompt_password(prompt).map_err(|_| {
                "the key is read from a terminal (without echo); run this in one".to_string()
            })?;
            keys.set(&a.id, &w.id, &key)?;
            println!("{}/{}: 登録済み", a.id, w.id);
            Ok(())
        }
        ["delete", rest @ ..] => {
            let (a, w) = chosen(&agents, rest)?;
            let removed = keys.delete(&a.id, &w.id)?;
            println!(
                "{}/{}: {}",
                a.id,
                w.id,
                if removed { "削除済み" } else { "未登録" }
            );
            Ok(())
        }
        ["status"] => {
            for a in &agents.list {
                for w in a.ways.iter().filter(|w| w.transport.takes_key()) {
                    let state = match keys.get(&a.id, &w.id) {
                        Ok(Some(_)) => "登録済み".to_string(),
                        Ok(None) => "未登録".to_string(),
                        Err(e) => format!("未登録 ({e})"),
                    };
                    println!("{}/{} ({}, {}): {state}", a.id, w.id, a.name, w.name);
                }
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-0123456789abcdef";

    #[test]
    fn a_key_round_trips_through_the_store_interface() {
        let k = Keys::double();
        assert_eq!(k.get("claude", "api"), Ok(None));
        assert!(!k.is_set("claude", "api"));
        k.set("claude", "api", &format!("  {KEY}\n")).unwrap();
        assert_eq!(k.get("claude", "api"), Ok(Some(KEY.to_string())));
        assert!(k.is_set("claude", "api"));
        // One entry per agent and way.
        assert_eq!(k.get("claude", "other"), Ok(None));
        assert_eq!(k.get("other", "api"), Ok(None));
        k.set("claude", "api", "sk-second").unwrap();
        assert_eq!(k.get("claude", "api"), Ok(Some("sk-second".to_string())));
        assert_eq!(k.delete("claude", "api"), Ok(true));
        assert_eq!(k.delete("claude", "api"), Ok(false));
        assert_eq!(k.get("claude", "api"), Ok(None));
    }

    #[test]
    fn an_unavailable_store_refuses_with_its_reason_and_how() {
        let k = Keys::unavailable("no session bus");
        let e = k.set("claude", "api", KEY).unwrap_err();
        assert!(
            e.contains("no session bus") && e.contains(HOW) && e.contains(STORE),
            "{e}"
        );
        assert!(!e.contains(KEY));
        assert!(k.get("claude", "api").is_err() && !k.is_set("claude", "api"));
        assert!(k.delete("claude", "api").is_err());
    }

    #[test]
    fn keys_that_cannot_go_in_a_header_are_refused_without_being_shown() {
        let long = "k".repeat(MAX_KEY + 1);
        for bad in [
            "",
            " \t",
            "sk with space",
            "sk\u{7f}",
            "sk-日本",
            long.as_str(),
        ] {
            let e = check_key(bad).unwrap_err();
            assert!(bad.trim().is_empty() || !e.contains(bad.trim()), "{e}");
        }
        assert_eq!(check_key(&"k".repeat(MAX_KEY)).map(str::len), Ok(MAX_KEY));
        assert!(Keys::double().set("claude", "api", "a b").is_err());
    }

    #[test]
    fn a_stored_value_never_shows_in_a_reason() {
        let e = why(Error::BadEncoding(KEY.as_bytes().to_vec()));
        assert!(!e.contains(KEY) && e.contains("register it again"), "{e}");
    }

    /// Manual, Windows only (the user's own Credential Manager): one entry
    /// under a test account is written, read, and removed.
    /// `cargo test -p dagon-cli os_store_round_trip -- --ignored`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn os_store_round_trip() {
        let k = Keys::os();
        let (agent, way) = ("dagon-test", "round-trip");
        let _ = k.delete(agent, way);
        assert_eq!(k.get(agent, way), Ok(None));
        k.set(agent, way, KEY).unwrap();
        assert_eq!(
            k.get(agent, way)
                .as_ref()
                .map(|v| v.as_deref() == Some(KEY)),
            Ok(true)
        );
        assert_eq!(k.delete(agent, way), Ok(true));
        assert_eq!(k.get(agent, way), Ok(None));
    }
}
