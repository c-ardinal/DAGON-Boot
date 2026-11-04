//! Transport kind `command`: a local program gets the prompt on stdin and
//! prints its answer on stdout.
//!
//! Trust boundary: the program is one the user installed and named (the
//! builtin definitions or their own ~/.dagon/agents); it runs with the
//! user's permissions and DAGON does not sandbox it. DAGON controls what it
//! sends (the prompt) and what the child inherits: a short environment
//! allow-list plus the variables the way lists in `pass_env` (never DAGON's
//! or an agent key's), a fresh empty working folder (not the project's;
//! hygiene, not confinement), the call's time and 1 MiB of stdout; stderr
//! is never read. The program runs in an OS group (a Windows job object, a
//! Unix process group) that is stopped when the deadline passes and as soon
//! as the program itself ends, so nothing it started outlives the call.

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
pub(super) const BASE_ENV: [&str; 14] = [
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
    let mut group = spawn(exe, &args, dir, &cmd.pass_env, &secrets)?;
    let child = group.child();
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
        if let Some(s) = group
            .child()
            .try_wait()
            .map_err(|_| "the agent program could not be watched".to_string())?
        {
            break s;
        }
        if Instant::now() >= deadline {
            group.stop();
            return Err(TIMED_OUT.into());
        }
        std::thread::sleep(POLL);
    };
    // The program has ended: whatever it left running is stopped now, so
    // its stdout closes and nothing outlives the call.
    group.stop();
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

/// The agent program and everything it starts.
///
/// Windows: a job object, killed when closed, so also when dagon itself
/// ends. Unix: a process group led by a small keeper process (`cat` reading
/// a pipe only dagon holds) that the program joins. The keeper stays
/// unreaped until the group has been signalled, so the group id can never
/// belong to anyone else when dagon signals it, even after the program has
/// ended and been reaped. When dagon itself is killed on Unix, the keeper
/// ends (its pipe closes) but the program and what it started keep running
/// until they end on their own: Unix has no kill-on-close for groups.
struct Group {
    #[cfg(windows)]
    inner: command_group::GroupChild,
    #[cfg(not(windows))]
    inner: std::process::Child,
    #[cfg(not(windows))]
    keeper: std::process::Child,
    stopped: bool,
}

impl Group {
    fn child(&mut self) -> &mut std::process::Child {
        #[cfg(windows)]
        return self.inner.inner();
        #[cfg(not(windows))]
        return &mut self.inner;
    }

    /// Stops every process of the group, also after the program itself
    /// has ended (no PID of a live parent is needed). Once only.
    fn stop(&mut self) {
        if std::mem::replace(&mut self.stopped, true) {
            return;
        }
        #[cfg(windows)]
        let _ = self.inner.kill();
        #[cfg(not(windows))]
        let _ = std::process::Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{}", self.keeper.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let child = self.child();
        let _ = child.kill();
        let _ = child.wait();
        // Reaped last: until here the group id is the keeper's.
        #[cfg(not(windows))]
        {
            let _ = self.keeper.kill();
            let _ = self.keeper.wait();
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the program in its group with piped stdin/stdout, the
/// allow-listed environment, in `dir`.
fn spawn(
    exe: &std::path::Path,
    args: &[String],
    dir: &std::path::Path,
    pass_env: &[String],
    secrets: &[String],
) -> Result<Group, String> {
    let mut c = std::process::Command::new(exe);
    c.args(args)
        .current_dir(dir)
        .env_clear()
        .envs(child_env(std::env::vars_os(), pass_env, secrets))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let failed = |_| "the agent program could not be started".to_string();
    #[cfg(windows)]
    let group = {
        use command_group::CommandGroup;
        Group {
            inner: c.group().kill_on_drop(true).spawn().map_err(failed)?,
            stopped: false,
        }
    };
    #[cfg(not(windows))]
    let group = {
        use std::os::unix::process::CommandExt;
        let keeper = std::process::Command::new("cat")
            .current_dir(dir)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(failed)?;
        let pgid = i32::try_from(keeper.id()).expect("PIDs fit in i32");
        c.process_group(pgid);
        match c.spawn() {
            Ok(inner) => Group {
                inner,
                keeper,
                stopped: false,
            },
            Err(e) => {
                let mut keeper = keeper;
                let _ = keeper.kill();
                let _ = keeper.wait();
                return Err(failed(e));
            }
        }
    };
    Ok(group)
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
    fn stopping_the_group_stops_the_whole_tree() {
        let (exe, args) = parent_of_a_sleeper();
        let dir = super::super::temp_dir().unwrap();
        let mut group = spawn(&exe, &args, &dir, &[], &[]).unwrap();
        let mut out = group.child().stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = out.read_to_end(&mut sink);
            let _ = tx.send(());
        });
        std::thread::sleep(Duration::from_millis(500));
        group.stop();
        // The grandchild holds the same stdout: it closes only once the
        // grandchild is gone too (it would live 30 s otherwise).
        rx.recv_timeout(Duration::from_secs(10))
            .expect("a process of the tree outlived the stop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whether a process whose command line holds `marker` is running
    /// (Windows), or whether `pid` is (Unix).
    fn alive(marker: &str) -> bool {
        if cfg!(windows) {
            let q = format!(
                "@(Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'PING.EXE' -and $_.CommandLine -like '*{marker}*' }}).Count"
            );
            let o = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &q])
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout).trim() != "0"
        } else {
            std::process::Command::new("kill")
                .args(["-0", marker])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        }
    }

    /// The program exits at once after starting a grandchild that keeps its
    /// stdout open for 30 s: the call returns long before its deadline and
    /// the grandchild is gone.
    #[test]
    fn a_grandchild_left_holding_stdout_is_stopped_when_the_program_ends() {
        let marker = "127.0.0.37";
        let (program, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/C".to_string(), format!("start /B ping -n 30 {marker}")],
            )
        } else {
            (
                "sh",
                vec!["-c".to_string(), "sleep 30 & echo $!".to_string()],
            )
        };
        let cmd = Command {
            program: program.into(),
            args,
            system: SystemVia::Stdin,
            pass_env: Vec::new(),
        };
        let t0 = Instant::now();
        let quiet = Prompt {
            system: String::new(),
            user: String::new(),
            model: None,
        };
        // Whatever the grandchild printed before it was stopped (ping's
        // output may not be UTF-8) does not matter here; the time does.
        let out = super::run_unchecked(&cmd, &quiet, Duration::from_secs(25));
        assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
        let marker = if cfg!(windows) {
            marker.to_string()
        } else {
            out.expect("the program's own output").trim().to_string()
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(!alive(&marker), "the grandchild outlived the call");
    }

    /// Set for the helper below when a test starts it as its own process.
    #[cfg(windows)]
    const OWNER_ENV: &str = "DAGON_TEST_AGENT_OWNER";
    #[cfg(windows)]
    const OWNER_MARKER: &str = "127.0.0.38";

    /// Not a test on its own: the owner process of the next test. It starts
    /// a group whose program leaves a 30 s grandchild, says "ready", and
    /// waits to be killed.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn owner_of_a_group() {
        if std::env::var_os(OWNER_ENV).is_none() {
            return;
        }
        let exe = Command {
            program: "cmd".into(),
            args: Vec::new(),
            system: SystemVia::Stdin,
            pass_env: Vec::new(),
        };
        let dir = super::super::temp_dir().unwrap();
        let args = vec!["/C".to_string(), format!("ping -n 30 {OWNER_MARKER}")];
        let _group = spawn(&exe.program_path().unwrap(), &args, &dir, &[], &[]).unwrap();
        println!("ready");
        std::thread::sleep(Duration::from_secs(60));
    }

    /// dagon itself is killed while an agent call runs: on Windows the job
    /// closes with the process and takes every process in it along
    /// (kill-on-close). Unix has no such guarantee for process groups (see
    /// [`Group`]), so this runs on Windows only.
    #[cfg(windows)]
    #[test]
    fn killing_the_owning_process_stops_the_agent_tree() {
        use std::io::BufRead;
        let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agents::command::tests::owner_of_a_group",
                "--ignored",
                "--nocapture",
            ])
            .env(OWNER_ENV, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(owner.stdout.take().unwrap());
        let ready = out
            .lines()
            .map_while(Result::ok)
            .any(|l| l.contains("ready"));
        assert!(ready, "the owner never started its group");
        std::thread::sleep(Duration::from_millis(500));
        assert!(alive(OWNER_MARKER), "the grandchild never started");
        owner.kill().unwrap();
        let _ = owner.wait();
        let t0 = Instant::now();
        while alive(OWNER_MARKER) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(!alive(OWNER_MARKER), "the grandchild outlived its owner");
    }
}
