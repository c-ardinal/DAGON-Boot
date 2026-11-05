#![forbid(unsafe_code)]

mod agents;
mod ai;
mod ask;
mod cli;
mod commands;
mod config;
mod conform;
mod consent;
mod dagonsrc;
mod defs;
mod demo;
mod deploy;
mod device;
mod doctor;
mod esp;
mod extgen;
mod flash;
mod flashdev;
// Writes outside genfs: the one guarded path for generated files.
#[allow(clippy::disallowed_methods)]
mod genfs;
mod kept;
mod keystore;
mod nodebuild;
// Writes outside genfs: implements the atomic write genfs and the commands use.
#[allow(clippy::disallowed_methods)]
mod paths;
mod scaffold;
mod serial;
mod serve;
mod simrun;
mod srcver;
mod versions;

use std::path::Path;
use std::process::ExitCode;

use cli::{AiCmd, Cli, Command, ImageCmd, KeyCmd, ReleaseCmd, SimCmd, TopoCmd};

fn main() -> ExitCode {
    use clap::{CommandFactory, Parser};
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        // --help (exit 0) or a usage error (exit 2), as clap prints them.
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2));
        }
    };
    let command = match cli.command {
        _ if cli.version => Command::Version,
        Some(c) => c,
        None => {
            let _ = Cli::command().print_long_help();
            return ExitCode::SUCCESS;
        }
    };
    match command {
        Command::Version => {
            println!("{}", version_text(banner()));
            ExitCode::SUCCESS
        }
        Command::HelpMarkdown => {
            print!("{}", cli::markdown());
            ExitCode::SUCCESS
        }
        Command::Key(KeyCmd::Gen { out }) => key_gen(&out),
        Command::Key(KeyCmd::Pub { key }) => run(key_pub(key.as_deref())),
        Command::Topo(TopoCmd::Check(a)) => topo_check(a.json, &a.yaml),
        Command::Topo(TopoCmd::Compile(a)) => run(commands::topo_compile(&a)),
        Command::Topo(TopoCmd::Push(a)) => run(device::topo_push(&a)),
        Command::Image(ImageCmd::Sign(a)) => run(commands::image_sign(&a)),
        Command::Release(ReleaseCmd::Build(a)) => run(commands::release_build(&a)),
        Command::Provision(a) => run(commands::provision(&a)),
        Command::Rescue(a) => run(commands::rescue(&a)),
        Command::Status(a) => run(device::status(&a)),
        Command::Apply(a) => run(device::apply(&a)),
        Command::Conform(a) => run(conform::command(&a)),
        Command::Recover(a) => run(device::recover(&a)),
        Command::Rollback(a) => run(device::rollback(&a)),
        Command::New(a) => run(scaffold::new_command(&a)),
        Command::Gen(a) => run(scaffold::gen_command(&a)),
        Command::Build(a) => run(nodebuild::command(&a)),
        Command::Deploy(a) => run(deploy::command(&a)),
        Command::Flash(a) => run(flash::command(&a)),
        Command::Doctor(a) => run(doctor::command(&a)),
        Command::Serve(a) => run(serve::command(&a)),
        Command::Sim(SimCmd::Run) => run(simrun::run()),
        Command::Demo(a) => run(demo::command(&a)),
        Command::Ai(AiCmd::Link(a)) => run(consent::link_command(&a)),
        Command::Ai(AiCmd::Unlink) => run(consent::unlink_command()),
        Command::Ai(AiCmd::Key(k)) => run(keystore::command(&k)),
        Command::Ai(AiCmd::Ask(a)) => run(ask::command(&a.question)),
        Command::Ai(AiCmd::Topology(a)) => run(ai::topology(&a)),
    }
}

fn run(r: Result<(), String>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Prints the public half of the signing key (never the secret).
fn key_pub(key: Option<&str>) -> Result<(), String> {
    let k = config::load_key(key)?;
    println!("public_key: {}", dagon_sign::to_hex(&*k.pk));
    println!("key_id:     {}", config::key_id_text(&k.pk));
    Ok(())
}

fn key_gen(dir: &str) -> ExitCode {
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

fn topo_check(json: bool, path: &str) -> ExitCode {
    let yaml = match std::fs::read_to_string(path) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The definitions of the dagon.toml here, if any, else the builtin set.
    let reg = match crate::config::Config::load(None).and_then(|c| crate::defs::registry(&c)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = dagon_topology::check_str_with(&reg, &yaml);
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
        // What updating skips is only said of a 構成 without errors.
        if let Some(t) = dagon_topology::parse_str(&yaml)
            .ok()
            .filter(|_| report.is_pushable())
        {
            for line in crate::config::skipped_lines(&dagon_topology::cut_with(&reg, &t)) {
                println!("{line}");
            }
        }
    }
    if report.is_pushable() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

const BANNER: &str = include_str!("../../../assets/ascii-art/banner.txt");

/// The ASCII banner, only for a person at a terminal: piped and
/// machine-readable output never carries it.
fn banner() -> Option<String> {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal().then(banner_text)
}

fn banner_text() -> String {
    let lines: Vec<&str> = BANNER.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_owned()
}

fn version_text(banner: Option<String>) -> String {
    let line = versions::version_lines();
    match banner {
        Some(b) => format!("{b}\n\n{line}"),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_without_terminal_starts_with_one_clean_version_line() {
        let v = version_text(None);
        assert_eq!(v, versions::version_lines());
        let first = format!("dagon {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(v.lines().next(), Some(first.as_str()));
    }

    #[test]
    fn version_at_terminal_starts_with_the_banner() {
        let v = version_text(Some(banner_text()));
        assert!(v.starts_with(banner_text().as_str()));
        assert!(v.ends_with(&format!("\n\n{}", versions::version_lines())));
        assert!(!banner_text().contains('\r'));
    }
}
