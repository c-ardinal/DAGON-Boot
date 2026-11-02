//! The ASCII banner is for a person at a terminal: `dagon --version | cat`
//! and `--json` output stay clean (stdout is a pipe here).

use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

#[test]
fn piped_version_is_one_clean_line() {
    let out = Command::new(EXE)
        .arg("--version")
        .output()
        .expect("run dagon");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).expect("utf-8");
    assert_eq!(text, format!("dagon {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn json_output_carries_no_banner() {
    let dir = std::env::temp_dir().join(format!("dagon-banner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let yaml = dir.join("t.yaml");
    std::fs::write(&yaml, "nodes: []\n").expect("write yaml");
    let out = Command::new(EXE)
        .args(["topo", "check", "--json"])
        .arg(&yaml)
        .output()
        .expect("run dagon");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8(out.stdout).expect("utf-8");
    serde_json::from_str::<serde_json::Value>(&text).expect("stdout is JSON only");
}
