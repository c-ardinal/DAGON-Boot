//! `dagon demo` reads and writes none of the user's settings, run as the user
//! runs it: the same home that `dagon serve` finds linked is unlinked in the
//! demo, and stays byte for byte as it was.

// The test sets up and clears its own scratch folder directly.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

struct Running {
    child: Child,
    base: String,
    pass: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Running {
    fn start(args: &[&str], home: &Path, temp: &Path) -> Running {
        let mut child = Command::new(EXE)
            .args(args)
            .args(["--no-open", "--port", "0"])
            .env("USERPROFILE", home)
            .env("HOME", home)
            .env("TMP", temp)
            .env("TEMP", temp)
            .env("TMPDIR", temp)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("run dagon");
        let out = BufReader::new(child.stdout.take().unwrap());
        let line = out
            .lines()
            .map_while(Result::ok)
            .find(|l| l.starts_with("open DAGON Console: "))
            .expect("the page's address");
        let url = line.trim_start_matches("open DAGON Console: ");
        let (base, pass) = url.split_once("/#").unwrap();
        Running {
            child,
            base: base.into(),
            pass: pass.into(),
        }
    }

    fn get(&self, path: &str) -> serde_json::Value {
        let body = ureq::get(format!("{}/v1/{path}", self.base))
            .header("Authorization", format!("Bearer {}", self.pass))
            .call()
            .expect("GET")
            .into_body()
            .read_to_string()
            .expect("body");
        serde_json::from_str(&body).expect("json")
    }

    fn post_status(&self, path: &str, body: &str) -> u16 {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        agent
            .post(format!("{}/v1/{path}", self.base))
            .header("Authorization", format!("Bearer {}", self.pass))
            .content_type("application/json")
            .send(body)
            .expect("POST")
            .status()
            .as_u16()
    }
}

fn snapshot(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            snapshot(&p, out);
        } else {
            out.insert(p.clone(), std::fs::read(&p).unwrap());
        }
    }
}

#[test]
fn a_demo_run_starts_unlinked_and_leaves_the_users_home_as_it_was() {
    let root = std::env::temp_dir().join(format!("dagon-demo-settings-{}", std::process::id()));
    let (home, temp) = (root.join("home"), root.join("temp"));
    std::fs::create_dir_all(home.join(".dagon")).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(
        home.join(".dagon").join("agent.json"),
        r#"{"version":1,"agent":"claude","way":"claude-code","at":"2026-01-01T00:00:00Z"}"#,
    )
    .unwrap();
    // A project the user opened: the demo does not offer it.
    let theirs = root.join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(
        home.join(".dagon").join("recent.json"),
        serde_json::json!([theirs]).to_string(),
    )
    .unwrap();

    let real = Running::start(&["serve"], &home, &temp);
    assert_eq!(
        real.get("agent")["linked"],
        true,
        "serve still follows agent.json"
    );
    drop(real);

    let mut before = BTreeMap::new();
    snapshot(&home, &mut before);

    let start = |_: u8| Running::start(&["demo"], &home, &temp);
    let demo = start(1);
    assert_eq!(demo.get("agent")["linked"], false);
    assert_eq!(
        demo.get("project")["recent"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        demo.post_status(
            "agent/link",
            r#"{"agent":"claude","way":"claude-code","consent":true}"#
        ),
        403
    );
    drop(demo);

    // Nothing remains for the next run, and the user's home is untouched by either.
    let again = start(2);
    assert_eq!(again.get("agent")["linked"], false);
    assert_eq!(
        again.get("project")["recent"].as_array().map(Vec::len),
        Some(0)
    );
    drop(again);

    let mut after = BTreeMap::new();
    snapshot(&home, &mut after);
    assert_eq!(before, after);
    let _ = std::fs::remove_dir_all(&root);
}
