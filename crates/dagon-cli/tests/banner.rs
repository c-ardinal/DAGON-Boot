//! The ASCII banner is for a person at a terminal: `dagon --version | cat`
//! and `--json` output stay clean (stdout is a pipe here).

use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

#[test]
fn piped_version_is_the_version_line_then_the_formats_without_banner() {
    let out = Command::new(EXE)
        .arg("--version")
        .output()
        .expect("run dagon");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).expect("utf-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("dagon {}", env!("CARGO_PKG_VERSION")));
    assert!(
        lines[1].starts_with("formats (read and written): wire "),
        "{text}"
    );
    assert_eq!(lines.len(), 2, "{text}");
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

#[test]
fn the_serve_log_starts_with_the_banner_even_in_a_file() {
    const BANNER: &str = include_str!("../../../assets/ascii-art/banner.txt");
    let first = BANNER
        .lines()
        .map(str::trim_end)
        .find(|l| !l.is_empty())
        .unwrap();
    // A bad option ends serve right after its log opens (stdout is a pipe).
    let out = Command::new(EXE)
        .args(["serve", "--no-open", "--port", "not-a-number"])
        .output()
        .expect("run dagon");
    assert!(!out.status.success());
    let text = String::from_utf8(out.stdout).expect("utf-8");
    let top = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    assert_eq!(top.trim_end(), first, "{text}");
}
