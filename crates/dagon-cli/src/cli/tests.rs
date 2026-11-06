use clap::error::ErrorKind;
use clap::CommandFactory;

use super::*;

fn parse(line: &str) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("dagon").chain(line.split_whitespace()))
}

fn command(line: &str) -> Command {
    match parse(line) {
        Ok(Cli {
            command: Some(c), ..
        }) => c,
        other => panic!("{line}: {other:?}"),
    }
}

fn s(v: &str) -> Option<String> {
    Some(v.into())
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| i.to_string()).collect()
}

fn files(out: Option<String>, key: Option<String>) -> FirstInstallOpts {
    FirstInstallOpts {
        out,
        key,
        ..Default::default()
    }
}

fn sign(input: &str, node: &str, version: &str, out: &str) -> ImageSignArgs {
    ImageSignArgs {
        input: input.into(),
        node: node.into(),
        version: version.into(),
        out: out.into(),
        slot: "a".into(),
        kind: "app".into(),
        seq: "1".into(),
        proto: "1".into(),
        ..Default::default()
    }
}

fn demo() -> DemoArgs {
    DemoArgs {
        sim_speed: "1".into(),
        uplink: "usb-cdc".into(),
        ..Default::default()
    }
}

/// The invocations the README, the docs, the samples, the scripts and the
/// tests run, each with the request it stands for.
fn todays_invocations() -> Vec<(&'static str, Command)> {
    use Command as C;
    vec![
        ("key gen --out keys", C::Key(KeyCmd::Gen { out: "keys".into() })),
        ("key pub --key k.key", C::Key(KeyCmd::Pub { key: s("k.key") })),
        ("key pub", C::Key(KeyCmd::Pub { key: None })),
        (
            "topo check topology.yaml",
            C::Topo(TopoCmd::Check(TopoCheckArgs {
                json: false,
                yaml: "topology.yaml".into(),
            })),
        ),
        (
            "topo check topology.yaml --json",
            C::Topo(TopoCmd::Check(TopoCheckArgs {
                json: true,
                yaml: "topology.yaml".into(),
            })),
        ),
        (
            "topo check --json t.yaml",
            C::Topo(TopoCmd::Check(TopoCheckArgs {
                json: true,
                yaml: "t.yaml".into(),
            })),
        ),
        (
            "topo compile t.yaml --config d.toml --out t.bin",
            C::Topo(TopoCmd::Compile(TopoCompileArgs {
                yaml: "t.yaml".into(),
                out: s("t.bin"),
                config: s("d.toml"),
            })),
        ),
        (
            "topo push",
            C::Topo(TopoCmd::Push(TopoPushArgs::default())),
        ),
        (
            "topo push t.yaml --port COM5",
            C::Topo(TopoCmd::Push(TopoPushArgs {
                yaml: s("t.yaml"),
                port: s("COM5"),
                config: None,
            })),
        ),
        (
            "image sign out/watch-good.bin --node watch --version 1.0.0 --out out/watch-good-1.0.0.img",
            C::Image(ImageCmd::Sign(sign(
                "out/watch-good.bin",
                "watch",
                "1.0.0",
                "out/watch-good-1.0.0.img",
            ))),
        ),
        (
            "image sign app.elf --node seg --version 1.2.0 --slot b --out out/seg-b-1.2.0.img --key k.key",
            C::Image(ImageCmd::Sign(ImageSignArgs {
                slot: "b".into(),
                key: s("k.key"),
                ..sign("app.elf", "seg", "1.2.0", "out/seg-b-1.2.0.img")
            })),
        ),
        (
            "image sign hub.elf --node hub --version 1.1.0 --slot a --out i/hub-a.img --topology t.yaml --key k.key",
            C::Image(ImageCmd::Sign(ImageSignArgs {
                topology: s("t.yaml"),
                key: s("k.key"),
                ..sign("hub.elf", "hub", "1.1.0", "i/hub-a.img")
            })),
        ),
        (
            "image sign a.bin --node seg --slot b --version 1.1.0 --config d.toml --key k --out o.img",
            C::Image(ImageCmd::Sign(ImageSignArgs {
                slot: "b".into(),
                config: s("d.toml"),
                key: s("k"),
                ..sign("a.bin", "seg", "1.1.0", "o.img")
            })),
        ),
        (
            "image sign a.elf --node n --version 1.0.0 --out o --kind bootcore --seq 7 --min-bootcore 1.0.0 --proto 2 --flags debug --caps sig-verifier",
            C::Image(ImageCmd::Sign(ImageSignArgs {
                kind: "bootcore".into(),
                seq: "7".into(),
                min_bootcore: s("1.0.0"),
                proto: "2".into(),
                flags: s("debug"),
                caps: s("sig-verifier"),
                ..sign("a.elf", "n", "1.0.0", "o")
            })),
        ),
        (
            "release build images/1.1 --name release-1.1 --out r.dgb --topology t.yaml --key k",
            C::Release(ReleaseCmd::Build(ReleaseBuildArgs {
                inputs: v(&["images/1.1"]),
                name: "release-1.1".into(),
                out: s("r.dgb"),
                topology: s("t.yaml"),
                key: s("k"),
                ..Default::default()
            })),
        ),
        (
            "release build a.img b.img --name r --topology-bin t.bin --config d.toml",
            C::Release(ReleaseCmd::Build(ReleaseBuildArgs {
                inputs: v(&["a.img", "b.img"]),
                name: "r".into(),
                topology_bin: s("t.bin"),
                config: s("d.toml"),
                ..Default::default()
            })),
        ),
        (
            "provision seg --key /keys/dagon-signing.key",
            C::Provision(ProvisionArgs {
                node: "seg".into(),
                files: files(None, s("/keys/dagon-signing.key")),
            }),
        ),
        (
            "provision seg --config d.toml --key k --out prov",
            C::Provision(ProvisionArgs {
                node: "seg".into(),
                files: FirstInstallOpts {
                    config: s("d.toml"),
                    ..files(s("prov"), s("k"))
                },
            }),
        ),
        (
            "rescue hub --key /keys/dagon-signing.key",
            C::Rescue(RescueArgs {
                target: "hub".into(),
                files: files(None, s("/keys/dagon-signing.key")),
            }),
        ),
        (
            "rescue --out r hub",
            C::Rescue(RescueArgs {
                target: "hub".into(),
                files: files(s("r"), None),
            }),
        ),
        ("status", C::Status(StatusArgs::default())),
        (
            "status --port COM5 --topology t.yaml",
            C::Status(StatusArgs {
                port: s("COM5"),
                topology: s("t.yaml"),
                config: None,
            }),
        ),
        ("apply", C::Apply(ApplyArgs::default())),
        (
            "apply out/deploy/v1.0.0.dgb",
            C::Apply(ApplyArgs {
                dgb: s("out/deploy/v1.0.0.dgb"),
                ..Default::default()
            }),
        ),
        (
            "apply r.dgb --node seg --node watch --no-host-verify --pubkey p.pub",
            C::Apply(ApplyArgs {
                dgb: s("r.dgb"),
                node: v(&["seg", "watch"]),
                no_host_verify: true,
                pubkey: s("p.pub"),
                ..Default::default()
            }),
        ),
        (
            "conform --node watch",
            C::Conform(ConformArgs {
                node: "watch".into(),
                ..Default::default()
            }),
        ),
        (
            "conform --node seg --usb --images imgs --out res",
            C::Conform(ConformArgs {
                node: "seg".into(),
                usb: true,
                images: s("imgs"),
                out: s("res"),
                ..Default::default()
            }),
        ),
        (
            "rollback --node seg",
            C::Rollback(RollbackArgs {
                node: "seg".into(),
                ..Default::default()
            }),
        ),
        (
            "new my-fleet",
            C::New(NewArgs {
                dir: "my-fleet".into(),
                ..Default::default()
            }),
        ),
        (
            "new my-fleet --dagon-path ../DAGON-Boot",
            C::New(NewArgs {
                dir: "my-fleet".into(),
                dagon_path: s("../DAGON-Boot"),
                ..Default::default()
            }),
        ),
        (
            "new run/p --from t.yaml --keys run/keys --dagon-path wt",
            C::New(NewArgs {
                dir: "run/p".into(),
                from: s("t.yaml"),
                keys: s("run/keys"),
                dagon_path: s("wt"),
                ..Default::default()
            }),
        ),
        (
            "new p --dagon-git https://example.org/d.git --dagon-tag cli-v1.0.0",
            C::New(NewArgs {
                dir: "p".into(),
                dagon_git: s("https://example.org/d.git"),
                dagon_tag: s("cli-v1.0.0"),
                ..Default::default()
            }),
        ),
        ("gen", C::Gen(GenArgs::default())),
        (
            "gen hub",
            C::Gen(GenArgs {
                nodes: v(&["hub"]),
                ..Default::default()
            }),
        ),
        (
            "gen --force-main",
            C::Gen(GenArgs {
                force_main: true,
                ..Default::default()
            }),
        ),
        ("build", C::Build(BuildArgs::default())),
        (
            "build seg",
            C::Build(BuildArgs {
                nodes: v(&["seg"]),
                config: None,
            }),
        ),
        (
            "build --config fail.toml",
            C::Build(BuildArgs {
                nodes: vec![],
                config: s("fail.toml"),
            }),
        ),
        ("deploy", C::Deploy(DeployArgs::default())),
        (
            "deploy seg watch",
            C::Deploy(DeployArgs {
                nodes: v(&["seg", "watch"]),
                ..Default::default()
            }),
        ),
        (
            "deploy --key k",
            C::Deploy(DeployArgs {
                key: s("k"),
                ..Default::default()
            }),
        ),
        (
            "deploy hub --bootcore --key k",
            C::Deploy(DeployArgs {
                nodes: v(&["hub"]),
                bootcore: true,
                key: s("k"),
                ..Default::default()
            }),
        ),
        (
            "deploy --version 1.2.0 --yes --port COM5",
            C::Deploy(DeployArgs {
                version: s("1.2.0"),
                yes: true,
                port: s("COM5"),
                ..Default::default()
            }),
        ),
        (
            "flash hub",
            C::Flash(FlashArgs {
                node: "hub".into(),
                ..Default::default()
            }),
        ),
        (
            "flash seg --dry-run --key k",
            C::Flash(FlashArgs {
                node: "seg".into(),
                dry_run: true,
                key: s("k"),
                ..Default::default()
            }),
        ),
        (
            "flash watch --yes --port COM7",
            C::Flash(FlashArgs {
                node: "watch".into(),
                yes: true,
                port: s("COM7"),
                ..Default::default()
            }),
        ),
        ("doctor", C::Doctor(DoctorArgs::default())),
        (
            "doctor --fix",
            C::Doctor(DoctorArgs {
                fix: true,
                ..Default::default()
            }),
        ),
        ("serve", C::Serve(ServeArgs::default())),
        (
            "serve --no-open --port 8471",
            C::Serve(ServeArgs {
                no_open: true,
                port: s("8471"),
                ..Default::default()
            }),
        ),
        (
            "serve --no-open --port not-a-number",
            C::Serve(ServeArgs {
                no_open: true,
                port: s("not-a-number"),
                ..Default::default()
            }),
        ),
        (
            "serve --config d.toml --web w --pubkey p.pub --key k",
            C::Serve(ServeArgs {
                config: s("d.toml"),
                web: s("w"),
                pubkey: s("p.pub"),
                key: s("k"),
                ..Default::default()
            }),
        ),
        ("sim run", C::Sim(SimCmd::Run)),
        ("demo", C::Demo(demo())),
        (
            "demo --sim-fail watch",
            C::Demo(DemoArgs {
                sim_fail: s("watch"),
                ..demo()
            }),
        ),
        (
            "demo --sim-speed 4",
            C::Demo(DemoArgs {
                sim_speed: "4".into(),
                ..demo()
            }),
        ),
        (
            "demo --uplink uart",
            C::Demo(DemoArgs {
                uplink: "uart".into(),
                ..demo()
            }),
        ),
        (
            "demo --port 8471 --no-open",
            C::Demo(DemoArgs {
                port: s("8471"),
                no_open: true,
                ..demo()
            }),
        ),
        ("ai link", C::Ai(AiCmd::Link(AgentWay::default()))),
        (
            "ai link --way api",
            C::Ai(AiCmd::Link(AgentWay {
                agent: None,
                way: s("api"),
            })),
        ),
        ("ai unlink", C::Ai(AiCmd::Unlink)),
        (
            "ai key set",
            C::Ai(AiCmd::Key(AiKeyCmd::Set(AgentWay::default()))),
        ),
        (
            "ai key set --agent claude --way api",
            C::Ai(AiCmd::Key(AiKeyCmd::Set(AgentWay {
                agent: s("claude"),
                way: s("api"),
            }))),
        ),
        ("ai key status", C::Ai(AiCmd::Key(AiKeyCmd::Status))),
        (
            "ai key delete --way api",
            C::Ai(AiCmd::Key(AiKeyCmd::Delete(AgentWay {
                agent: None,
                way: s("api"),
            }))),
        ),
        (
            "ai ask why does seg not answer?",
            C::Ai(AiCmd::Ask(AskArgs {
                question: v(&["why", "does", "seg", "not", "answer?"]),
            })),
        ),
        (
            "ai ask what does --bootcore do",
            C::Ai(AiCmd::Ask(AskArgs {
                question: v(&["what", "does", "--bootcore", "do"]),
            })),
        ),
        (
            "ai topology --in boards.txt --out topology.yaml",
            C::Ai(AiCmd::Topology(AiTopologyArgs {
                input: s("boards.txt"),
                out: "topology.yaml".into(),
                model: None,
            })),
        ),
        (
            "ai topology --model m2",
            C::Ai(AiCmd::Topology(AiTopologyArgs {
                input: None,
                out: "topology.yaml".into(),
                model: s("m2"),
            })),
        ),
        ("version", C::Version),
    ]
}

#[test]
fn todays_invocations_parse_to_the_same_request() {
    for (line, want) in todays_invocations() {
        assert_eq!(command(line), want, "dagon {line}");
    }
}

#[test]
fn every_command_in_the_table() {
    // Each leaf command the CLI has is covered by at least one row above.
    let rows: Vec<String> = todays_invocations()
        .iter()
        .map(|(l, _)| l.to_string())
        .collect();
    for path in leaf_paths(&Cli::command(), &[]) {
        let p = path.join(" ");
        assert!(
            rows.iter()
                .any(|r| r == &p || r.starts_with(&format!("{p} "))),
            "no invocation of dagon {p} in the table"
        );
    }
}

fn leaf_paths(cmd: &clap::Command, prefix: &[String]) -> Vec<Vec<String>> {
    let subs: Vec<&clap::Command> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .collect();
    if subs.is_empty() {
        return vec![prefix.to_vec()];
    }
    subs.iter()
        .flat_map(|c| {
            let mut p = prefix.to_vec();
            p.push(c.get_name().to_string());
            leaf_paths(c, &p)
        })
        .collect()
}

#[test]
fn the_definitions_are_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn every_command_and_option_says_what_it_is_for() {
    let mut cmd = Cli::command();
    cmd.build();
    for path in leaf_paths(&cmd, &[]) {
        let mut c = &cmd;
        for name in &path {
            c = c.find_subcommand(name).expect("subcommand");
            assert!(
                c.get_about().is_some(),
                "dagon {}: no about",
                path.join(" ")
            );
        }
        for a in c.get_arguments().filter(|a| !a.is_hide_set()) {
            assert!(
                a.get_help().is_some(),
                "dagon {} {}: no help",
                path.join(" "),
                a.get_id()
            );
        }
        let line = format!("{} --help", path.join(" "));
        let e = parse(&line).expect_err("help is shown, not parsed");
        assert_eq!(e.kind(), ErrorKind::DisplayHelp, "{line}");
        assert_eq!(e.exit_code(), 0, "{line}");
        let text = e.to_string();
        assert!(
            text.contains(&format!("Usage: dagon {}", path.join(" "))),
            "{line}: {text}"
        );
    }
}

#[test]
fn bad_command_lines_are_usage_errors() {
    for (line, kind) in [
        ("bogus", ErrorKind::InvalidSubcommand),
        ("status --bogus x", ErrorKind::UnknownArgument),
        ("status extra", ErrorKind::UnknownArgument),
        ("key gen", ErrorKind::MissingRequiredArgument),
        ("key gen --out", ErrorKind::InvalidValue),
        ("topo check", ErrorKind::MissingRequiredArgument),
        ("topo check a.yaml b.yaml", ErrorKind::UnknownArgument),
        (
            "image sign a.elf --node n --out o",
            ErrorKind::MissingRequiredArgument,
        ),
        ("release build --name r", ErrorKind::MissingRequiredArgument),
        ("provision", ErrorKind::MissingRequiredArgument),
        ("rescue node", ErrorKind::InvalidValue),
        ("rollback", ErrorKind::MissingRequiredArgument),
        ("flash", ErrorKind::MissingRequiredArgument),
        ("new", ErrorKind::MissingRequiredArgument),
        ("sim run now", ErrorKind::UnknownArgument),
        ("demo --sim-fail hub", ErrorKind::InvalidValue),
        ("demo extra", ErrorKind::UnknownArgument),
        ("ai link --way", ErrorKind::InvalidValue),
        ("demo --uplink can", ErrorKind::InvalidValue),
        ("ai ask", ErrorKind::MissingRequiredArgument),
        ("ai unlink now", ErrorKind::UnknownArgument),
        ("ai key status --way api", ErrorKind::UnknownArgument),
        ("--version status", ErrorKind::ArgumentConflict),
    ] {
        let e = parse(line).expect_err(line);
        assert_eq!(e.kind(), kind, "dagon {line}: {e}");
        assert_eq!(e.exit_code(), 2, "dagon {line}");
    }
}

#[test]
fn a_repeated_option_keeps_its_last_value() {
    assert_eq!(
        command("topo compile t.yaml --out a.bin --out b.bin"),
        Command::Topo(TopoCmd::Compile(TopoCompileArgs {
            yaml: "t.yaml".into(),
            out: s("b.bin"),
            config: None,
        }))
    );
}

#[test]
fn no_command_and_version_flags() {
    let bare = parse("").unwrap();
    assert!(bare.command.is_none() && !bare.version);
    assert!(parse("--version").unwrap().version);
    assert!(parse("-V").unwrap().version);
}

#[test]
fn the_markdown_reference_has_every_command() {
    let md = markdown();
    for path in leaf_paths(&Cli::command(), &[]) {
        assert!(
            md.contains(&format!("## dagon {}\n", path.join(" "))),
            "dagon {}",
            path.join(" ")
        );
    }
    assert!(!md.contains("help-markdown"));
}
