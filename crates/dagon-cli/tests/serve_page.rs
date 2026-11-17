//! A `dagon serve` with no DAGON Console page says in one line how to build
//! one, rather than serving a blank Console.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

#[test]
// Makes and removes its own scratch folder.
#[allow(clippy::disallowed_methods)]
fn serve_without_a_page_prints_how_to_build_it() {
    let dir = std::env::temp_dir().join(format!("dagon-no-page-{}", std::process::id()));
    let web = dir.join("empty-page");
    std::fs::create_dir_all(&web).expect("temp dir");
    let mut child = Command::new(EXE)
        .args(["serve", "--no-open", "--port", "0", "--web"])
        .arg(&web)
        .current_dir(&dir)
        .env_remove("DAGON_SIGNING_KEY")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run dagon");
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let hint = rx.recv_timeout(Duration::from_secs(30)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(hint.contains("python scripts/page.py build"), "{hint}");
    assert!(hint.contains("--web"), "{hint}");
}
