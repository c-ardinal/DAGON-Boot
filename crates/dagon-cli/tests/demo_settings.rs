//! `dagon demo` is closed: it is not in the help and running it fails as an
//! unknown command, so it can neither build nor touch the user's settings.
//! The online demo (Pages) stays the one way to try DAGON without boards.

use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_dagon");

#[test]
fn dagon_demo_is_no_command() {
    let help = Command::new(EXE).arg("--help").output().expect("run dagon");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();
    assert!(help.contains("Usage: dagon"), "{help}");
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("demo")),
        "{help}"
    );
    for args in [&["demo"][..], &["demo", "--no-open"]] {
        let out = Command::new(EXE).args(args).output().expect("run dagon");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            err.contains("unrecognized subcommand 'demo'"),
            "{args:?}: {err}"
        );
    }
}
