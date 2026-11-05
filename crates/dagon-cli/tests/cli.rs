//! Exit codes and streams of the command line itself: help on stdout with
//! 0, usage errors on stderr with 2 (a command that runs and fails exits 1).

use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

fn dagon(args: &[&str]) -> Output {
    Command::new(EXE).args(args).output().expect("run dagon")
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("utf-8")
}

#[test]
fn help_goes_to_stdout_with_exit_0() {
    for args in [&[][..], &["--help"], &["-h"], &["help"]] {
        let out = dagon(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        let help = text(&out.stdout);
        assert!(help.contains("Usage: dagon"), "{args:?}: {help}");
        assert!(help.contains("deploy"), "{args:?}: {help}");
    }
    for args in [
        &["status", "--help"][..],
        &["help", "status"],
        &["ai", "key", "set", "-h"],
    ] {
        let out = dagon(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(text(&out.stdout).contains("Usage: dagon "), "{args:?}");
    }
}

#[test]
fn usage_errors_go_to_stderr_with_exit_2() {
    for args in [
        &["bogus"][..],
        &["status", "--bogus"],
        &["key", "gen"],
        &["topo", "check"],
        &["flash"],
    ] {
        let out = dagon(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let err = text(&out.stderr);
        assert!(err.starts_with("error: "), "{args:?}: {err}");
        assert!(err.contains("--help"), "{args:?}: {err}");
    }
}

#[test]
fn version_word_and_flag_print_the_same() {
    let flag = dagon(&["--version"]);
    let word = dagon(&["version"]);
    assert_eq!(flag.status.code(), Some(0));
    assert_eq!(flag.stdout, word.stdout);
}

#[test]
fn a_command_that_fails_exits_1() {
    let out = dagon(&["topo", "check", "no-such-file.yaml"]);
    assert_eq!(out.status.code(), Some(1));
}
