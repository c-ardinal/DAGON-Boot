#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "dagon - DAGON Boot command line tool

Usage:
  dagon key gen --out <dir>          generate a signing key pair (dir must be outside any git work tree)
  dagon topo check [--json] <yaml>   validate a topology file
  dagon --help";

/// Exit code for bad command-line usage.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["--help" | "-h" | "help", ..] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["key", "gen", rest @ ..] => key_gen(rest),
        ["topo", "check", rest @ ..] => topo_check(rest),
        _ => usage_error("unknown command"),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(EXIT_USAGE)
}

fn key_gen(args: &[&str]) -> ExitCode {
    let ["--out", dir] = args else {
        return usage_error("key gen needs --out <dir>");
    };
    match dagon_sign::generate_to_dir(Path::new(dir)) {
        Ok(k) => {
            println!("public_key: {}", dagon_sign::to_hex(&k.public_key));
            println!("key_id:     {}", dagon_sign::to_hex(&k.key_id));
            println!("public key file: {}", k.public_path.display());
            println!(
                "secret key file: {} (keep it private; never commit it)",
                k.secret_path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn topo_check(args: &[&str]) -> ExitCode {
    let (json, path) = match args {
        ["--json", path] | [path, "--json"] => (true, *path),
        [path] if !path.starts_with('-') => (false, *path),
        _ => return usage_error("topo check needs [--json] <yaml>"),
    };
    let yaml = match std::fs::read_to_string(path) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = dagon_topology::check_str(&yaml);
    if json {
        println!("{}", report.to_json());
    } else {
        for i in &report.issues {
            let at = if i.path.is_empty() {
                String::new()
            } else {
                format!(" {}", i.path)
            };
            println!("{} {}{at}: {}", i.severity.as_str(), i.rule, i.message);
        }
        println!(
            "{path}: {} error(s), {} warning(s)",
            report.errors().count(),
            report.warnings().count()
        );
    }
    if report.is_pushable() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
