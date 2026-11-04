//! Transport kind `command`: a local program gets the prompt on stdin and
//! prints its answer on stdout.
//!
//! Trust boundary: the program is one the user installed and named (the
//! builtin definitions or their own ~/.dagon/agents); it runs with the
//! user's permissions and DAGON does not sandbox it. DAGON controls what it
//! sends (the prompt) and what the child inherits: a short environment
//! allow-list plus the variables the way lists in `pass_env` (never DAGON's
//! or an agent key's), a fresh empty working folder (not the project's;
//! hygiene, not confinement), the call's time (the whole process tree is
//! stopped) and 1 MiB of stdout; stderr is never read.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::{Cleared, Prompt, TIMED_OUT};

/// Stdout kept at most; a longer answer is refused.
const MAX_OUTPUT: usize = 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);
/// The arg that becomes the path of the file holding the system text.
const SYSTEM_FILE: &str = "{system_file}";
const SYSTEM_FILE_NAME: &str = "system.txt";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// A name looked up on PATH, or a path.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// How the system text reaches the program.
    #[serde(default)]
    pub system: SystemVia,
    /// Environment variables passed on besides [`BASE_ENV`] (names only).
    #[serde(default)]
    pub pass_env: Vec<String>,
}

/// What every command child inherits, if set (names compared without
/// case; `LC_*` too): enough to find programs, the user's profile and
/// temporary folders, and the locale.
const BASE_ENV: [&str; 14] = [
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "TMPDIR",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "LANG",
    "TERM",
];

/// Never passed on, even when a way lists it: DAGON's own and every agent
/// key a way names.
fn forbidden(name: &str, secrets: &[String]) -> bool {
    let up = name.to_ascii_uppercase();
    up.starts_with("DAGON_") || secrets.iter().any(|s| s.eq_ignore_ascii_case(name))
}

fn env_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The child's whole environment, from this process's `vars`.
fn child_env(
    vars: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    pass_env: &[String],
    secrets: &[String],
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    vars.filter(|(k, _)| {
        let Some(k) = k.to_str() else { return false };
        let up = k.to_ascii_uppercase();
        let allowed = BASE_ENV.contains(&up.as_str())
            || up.starts_with("LC_")
            || pass_env.iter().any(|p| p.eq_ignore_ascii_case(k));
        allowed && !forbidden(k, secrets)
    })
    .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemVia {
    /// Before the user text on stdin.
    #[default]
    Stdin,
    /// In a file in the call's folder, named by the `{system_file}` arg.
    File,
}

impl Command {
    pub(super) fn check(&self) -> Result<(), String> {
        if self.program.trim().is_empty() {
            return Err("program is empty".into());
        }
        let named = self.args.iter().any(|a| a == SYSTEM_FILE);
        if named != (self.system == SystemVia::File) {
            return Err(format!(
                "system = \"file\" and an arg {SYSTEM_FILE} go together"
            ));
        }
        if self
            .args
            .iter()
            .any(|a| a != SYSTEM_FILE && a.contains('{') && a.contains('}'))
        {
            return Err(format!(
                "the only placeholder is {SYSTEM_FILE}, as a whole arg"
            ));
        }
        if let Some(n) = self
            .pass_env
            .iter()
            .find(|n| !env_name(n) || n.to_ascii_uppercase().starts_with("DAGON_"))
        {
            return Err(format!(
                "pass_env {n}: names of A-Z a-z 0-9 _ only, never DAGON_*"
            ));
        }
        Ok(())
    }

    /// The program to run: a path as given, else the first match on PATH
    /// (on Windows with `.exe` or `.com` added).
    pub fn program_path(&self) -> Option<PathBuf> {
        let p = PathBuf::from(&self.program);
        let exts: &[&str] = if cfg!(windows) {
            &["", ".exe", ".com"]
        } else {
            &[""]
        };
        let found = |base: PathBuf| {
            exts.iter()
                .map(|e| PathBuf::from(format!("{}{e}", base.display())))
                .find(|c| c.is_file() && (!cfg!(windows) || has_ext(c)))
        };
        if p.components().count() > 1 {
            return found(p);
        }
        std::env::split_paths(&std::env::var_os("PATH")?).find_map(|d| found(d.join(&p)))
    }
}

fn has_ext(p: &std::path::Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("com"))
}

/// One call: the prompt in, stdout out.
pub(crate) fn run(
    _cleared: &Cleared,
    cmd: &Command,
    prompt: &Prompt,
    timeout: Duration,
) -> Result<String, String> {
    let exe = cmd
        .program_path()
        .ok_or_else(|| format!("the agent program {} is not on PATH", cmd.program))?;
    let dir = super::temp_dir()?;
    let r = run_in(&dir, &exe, cmd, prompt, timeout);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

fn run_in(
    dir: &std::path::Path,
    exe: &std::path::Path,
    cmd: &Command,
    prompt: &Prompt,
    timeout: Duration,
) -> Result<String, String> {
    let mut input = prompt.user.clone();
    let mut args = cmd.args.clone();
    match cmd.system {
        SystemVia::Stdin => input = format!("{}\n\n{input}", prompt.system),
        SystemVia::File => {
            let f = dir.join(SYSTEM_FILE_NAME);
            std::fs::write(&f, &prompt.system)
                .map_err(|_| "cannot write the agent's instructions".to_string())?;
            for a in args.iter_mut().filter(|a| *a == SYSTEM_FILE) {
                *a = f.display().to_string();
            }
        }
    }
    let secrets = super::secret_env(crate::consent::home().ok().as_deref());
    let mut child = spawn(exe, &args, dir, &cmd.pass_env, &secrets)?;
    let mut stdin = child.stdin.take().expect("piped");
    // A program that never reads stdin only loses this write. Neither
    // thread is waited for beyond the deadline: a process the program
    // left behind may hold its pipes open.
    std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stdout = child.stdout.take().expect("piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        // Read to the end (so the program never blocks), keep the cap + 1.
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            let room = (MAX_OUTPUT + 1).saturating_sub(kept.len());
            kept.extend_from_slice(&buf[..n.min(room)]);
        }
        let _ = tx.send(kept);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(s) = child
            .try_wait()
            .map_err(|_| "the agent program could not be watched".to_string())?
        {
            break s;
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            return Err(TIMED_OUT.into());
        }
        std::thread::sleep(POLL);
    };
    let out = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| TIMED_OUT.to_string())?;
    if !status.success() {
        return Err(match status.code() {
            Some(c) => format!("the agent program ended with exit code {c}"),
            None => "the agent program was stopped".into(),
        });
    }
    if out.len() > MAX_OUTPUT {
        return Err("the agent's answer is longer than 1 MiB".into());
    }
    String::from_utf8(out).map_err(|_| "the agent's answer is not UTF-8 text".into())
}

/// Starts the program with piped stdin/stdout, the allow-listed
/// environment, in `dir`; on Unix in a process group of its own (so the
/// whole tree can be stopped).
fn spawn(
    exe: &std::path::Path,
    args: &[String],
    dir: &std::path::Path,
    pass_env: &[String],
    secrets: &[String],
) -> Result<std::process::Child, String> {
    let mut c = std::process::Command::new(exe);
    c.args(args)
        .current_dir(dir)
        .env_clear()
        .envs(child_env(std::env::vars_os(), pass_env, secrets))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    c.spawn()
        .map_err(|_| "the agent program could not be started".to_string())
}

/// Stops the program and every process it started.
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let tree = {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        std::process::Command::new(
            std::path::Path::new(&root)
                .join("System32")
                .join("taskkill.exe"),
        )
        .args(["/F", "/T", "/PID", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    };
    #[cfg(unix)]
    let tree = std::process::Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = tree;
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
pub(super) fn run_unchecked(
    cmd: &Command,
    prompt: &Prompt,
    timeout: Duration,
) -> Result<String, String> {
    let exe = cmd.program_path().ok_or("not on PATH")?;
    let dir = super::temp_dir()?;
    let r = run_in(&dir, &exe, cmd, prompt, timeout);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn vars(list: &[(&str, &str)]) -> impl Iterator<Item = (OsString, OsString)> {
        list.iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_child_gets_the_allow_list_and_what_the_way_lists_never_secrets() {
        let secrets = vec![
            "ANTHROPIC_API_KEY".to_string(),
            "DAGON_SIGNING_KEY".to_string(),
        ];
        let env = child_env(
            vars(&[
                ("Path", "p"),
                ("USERPROFILE", "u"),
                ("SystemRoot", "s"),
                ("LC_ALL", "C"),
                ("ANTHROPIC_API_KEY", "sk-1"),
                ("DAGON_SIGNING_KEY", "k"),
                ("DAGON_AI_MODEL", "m"),
                ("AWS_SECRET_ACCESS_KEY", "a"),
                ("CARGO_MANIFEST_DIR", "c"),
                ("MY_AGENT_TOKEN", "t"),
            ]),
            &[
                "MY_AGENT_TOKEN".into(),
                "ANTHROPIC_API_KEY".into(),
                "DAGON_AI_MODEL".into(),
            ],
            &secrets,
        );
        let names: Vec<String> = env
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Path",
                "USERPROFILE",
                "SystemRoot",
                "LC_ALL",
                "MY_AGENT_TOKEN"
            ]
        );
    }

    #[test]
    fn pass_env_names_are_checked() {
        let mut c = Command {
            program: "x".into(),
            args: Vec::new(),
            system: SystemVia::Stdin,
            pass_env: vec!["MY_TOKEN".into()],
        };
        assert!(c.check().is_ok());
        for bad in ["", "A=B", "DAGON_SIGNING_KEY", "dagon_x", "A B"] {
            c.pass_env = vec![bad.into()];
            assert!(c.check().is_err(), "{bad}");
        }
    }

    /// A program that starts a long-running child and waits for it.
    fn parent_of_a_sleeper() -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            let exe = Command {
                program: "cmd".into(),
                args: Vec::new(),
                system: SystemVia::Stdin,
                pass_env: Vec::new(),
            };
            (
                exe.program_path().unwrap(),
                vec!["/C".into(), "ping -n 30 127.0.0.1".into()],
            )
        } else {
            (
                PathBuf::from("/bin/sh"),
                vec!["-c".into(), "sleep 30; true".into()],
            )
        }
    }

    #[test]
    fn a_timeout_stops_the_whole_process_tree() {
        let (exe, args) = parent_of_a_sleeper();
        let dir = super::super::temp_dir().unwrap();
        let mut child = spawn(&exe, &args, &dir, &[], &[]).unwrap();
        let mut out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = out.read_to_end(&mut sink);
            let _ = tx.send(());
        });
        std::thread::sleep(Duration::from_millis(500));
        kill_tree(&mut child);
        // The grandchild holds the same stdout: it closes only once the
        // grandchild is gone too (it would live 30 s otherwise).
        rx.recv_timeout(Duration::from_secs(10))
            .expect("a process of the tree outlived the stop");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
