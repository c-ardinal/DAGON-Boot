//! Transport kind `command`: a local program gets the prompt on stdin and
//! prints its answer on stdout. It runs in an empty temporary folder (never
//! the project's), without the keys' environment variables, within the
//! call's time and output size; stderr is never read.

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
    let mut c = std::process::Command::new(exe);
    c.args(&args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for k in super::secret_env(crate::consent::home().ok().as_deref()) {
        c.env_remove(k);
    }
    let mut child = c
        .spawn()
        .map_err(|_| "the agent program could not be started".to_string())?;
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
            let _ = child.kill();
            let _ = child.wait();
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
