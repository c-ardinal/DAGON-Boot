#![forbid(unsafe_code)]

mod commands;
mod config;
mod device;
mod serial;
mod simrun;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "dagon - DAGON Boot command line tool

Usage:
  dagon key gen --out <dir>          generate a signing key pair (dir must be outside any git work tree)
  dagon topo check [--json] <yaml>   validate a topology file
  dagon topo compile <yaml> [--out <bin>]
                                     topology canonical binary
  dagon image sign <elf|bin> --node <id> --version <x.y.z> --out <img>
        [--slot a|b] [--kind app|bootcore|agent] [--seq <n>] [--min-bootcore <x.y.z>]
        [--proto <n>] [--flags <list>] [--caps <list>] [--topology <yaml>] [--key <file>]
                                     build and sign an image for a topology node
  dagon release build <img|dir>... --name <name> [--out <dgb>] [--topology <yaml>]
        [--topology-bin <bin>] [--key <file>]
                                     check, sign the manifest and write the .dgb
  dagon provision <node> [--out <dir>] [--topology <yaml>] [--key <file>]
                                     first-install files from [provision.<node>] in dagon.toml
  dagon rescue hub [--out <dir>] [--topology <yaml>] [--key <file>]
                                     hub rescue UF2 (boot2, stage0, logs, boot core, app)
  dagon status [--port <name>] [--topology <yaml>]
                                     HELLO and STATUS of the hub and each topology node
  dagon apply [<dgb>] [--node <id>]... [--port <name>] [--topology <yaml>]
        [--pubkey <file>] [--key <file>]
                                     verify the release, check it matches the topology,
                                     push the topology, update children then the hub
  dagon topo push [<yaml>] [--port <name>]
                                     send the topology canonical binary to the hub
  dagon recover <node> [--release <dgb>] [--bootcore <img>] [--port <name>]
        [--topology <yaml>] [--pubkey <file>] [--key <file>]
                                     rewrite a node from the release: normal transfer if
                                     it answers, else the F401 ROM loader (boot core from
                                     <provision_dir>/<node>/bootcore.img by default)
  dagon sim run                      the four simulator scenarios (no hardware)
  dagon --help

The hub is found by USB ID c0de:cafe or product \"DAGON hub\" unless --port
is given. Defaults come from ./dagon.toml (or --config <file>). The secret key file is
--key <file> or $DAGON_SIGNING_KEY and must lie outside any git work tree.
--flags: verify-enforced,debug,sim-only  --caps: confirm-by-hub,reboot-to-update,
rom-entry-coop,sig-verifier";

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
        ["topo", "compile", rest @ ..] => run(commands::topo_compile(rest)),
        ["image", "sign", rest @ ..] => run(commands::image_sign(rest)),
        ["release", "build", rest @ ..] => run(commands::release_build(rest)),
        ["provision", rest @ ..] => run(commands::provision(rest)),
        ["rescue", rest @ ..] => run(commands::rescue(rest)),
        ["status", rest @ ..] => run(device::status(rest)),
        ["apply", rest @ ..] => run(device::apply(rest)),
        ["topo", "push", rest @ ..] => run(device::topo_push(rest)),
        ["recover", rest @ ..] => run(device::recover(rest)),
        ["sim", "run", rest @ ..] => run(simrun::run(rest)),
        _ => usage_error("unknown command"),
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
