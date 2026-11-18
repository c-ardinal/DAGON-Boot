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
            "image out/watch-good.bin --node watch --version 1.0.0 --out out/watch-good-1.0.0.img",
            C::Image(sign(
                "out/watch-good.bin",
                "watch",
                "1.0.0",
                "out/watch-good-1.0.0.img",
            )),
        ),
        (
            "image app.elf --node seg --version 1.2.0 --slot b --out out/seg-b-1.2.0.img --key k.key",
            C::Image(ImageSignArgs {
                slot: "b".into(),
                key: s("k.key"),
                ..sign("app.elf", "seg", "1.2.0", "out/seg-b-1.2.0.img")
            }),
        ),
        (
            "image hub.elf --node hub --version 1.1.0 --slot a --out i/hub-a.img --topology t.yaml --key k.key",
            C::Image(ImageSignArgs {
                topology: s("t.yaml"),
                key: s("k.key"),
                ..sign("hub.elf", "hub", "1.1.0", "i/hub-a.img")
            }),
        ),
        (
            "image a.bin --node seg --slot b --version 1.1.0 --config d.toml --key k --out o.img",
            C::Image(ImageSignArgs {
                slot: "b".into(),
                config: s("d.toml"),
                key: s("k"),
                ..sign("a.bin", "seg", "1.1.0", "o.img")
            }),
        ),
        (
            "image a.elf --node n --version 1.0.0 --out o --kind bootcore --seq 7 --min-bootcore 1.0.0 --proto 2 --flags debug --caps sig-verifier",
            C::Image(ImageSignArgs {
                kind: "bootcore".into(),
                seq: "7".into(),
                min_bootcore: s("1.0.0"),
                proto: "2".into(),
                flags: s("debug"),
                caps: s("sig-verifier"),
                ..sign("a.elf", "n", "1.0.0", "o")
            }),
        ),
        (
            "release images/1.1 --name release-1.1 --out r.dgb --topology t.yaml --key k",
            C::Release(ReleaseBuildArgs {
                inputs: v(&["images/1.1"]),
                name: "release-1.1".into(),
                out: s("r.dgb"),
                topology: s("t.yaml"),
                key: s("k"),
                ..Default::default()
            }),
        ),
        (
            "release a.img b.img --name r --topology-bin t.bin --config d.toml",
            C::Release(ReleaseBuildArgs {
                inputs: v(&["a.img", "b.img"]),
                name: "r".into(),
                topology_bin: s("t.bin"),
                config: s("d.toml"),
                ..Default::default()
            }),
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
        ("sim", C::Sim),
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
            "image a.elf --node n --out o",
            ErrorKind::MissingRequiredArgument,
        ),
        ("release --name r", ErrorKind::MissingRequiredArgument),
        ("provision", ErrorKind::MissingRequiredArgument),
        // `provision <node>` makes any node's first-install files.
        ("rescue hub", ErrorKind::InvalidSubcommand),
        ("rollback", ErrorKind::MissingRequiredArgument),
        ("flash", ErrorKind::MissingRequiredArgument),
        ("new", ErrorKind::MissingRequiredArgument),
        ("sim now", ErrorKind::UnknownArgument),
        // The forms before the single-child groups were flattened are gone.
        ("sim run", ErrorKind::UnknownArgument),
        (
            "image sign a.elf --node n --version 1.0.0 --out o",
            ErrorKind::UnknownArgument,
        ),
        ("ai link --way", ErrorKind::InvalidValue),
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
fn a_parse_error_under_ai_key_never_shows_the_input() {
    const FAKE: &str = "FAKE-NOT-A-KEY-123";
    for line in [
        "ai key set FAKE-NOT-A-KEY-123",
        "ai key set -- FAKE-NOT-A-KEY-123",
        "ai key set --FAKE-NOT-A-KEY-123",
        "ai key FAKE-NOT-A-KEY-123",
        "help ai key FAKE-NOT-A-KEY-123",
        "help ai key set FAKE-NOT-A-KEY-123",
    ] {
        let argv: Vec<String> = std::iter::once("dagon")
            .chain(line.split_whitespace())
            .map(String::from)
            .collect();
        let e = Cli::try_parse_from(&argv).expect_err(line);
        assert_eq!(e.exit_code(), 2, "dagon {line}");
        let text = parse_error_text(&argv, &e);
        assert!(!text.contains(FAKE), "dagon {line}: {text}");
        assert!(text.contains("Usage: dagon ai key"), "dagon {line}: {text}");
    }
}

#[test]
fn a_parse_error_under_ai_key_shows_no_part_of_a_multiline_input() {
    let argv = v(&["dagon", "ai", "key", "set", "FAKE-HEAD\n\nFAKE-TAIL-123"]);
    let e = Cli::try_parse_from(&argv).expect_err("multiline input");
    assert_eq!(e.exit_code(), 2);
    let text = parse_error_text(&argv, &e);
    assert!(!text.contains("FAKE-"), "{text}");
    assert!(text.contains("Usage: dagon ai key"), "{text}");
}

#[test]
fn a_parse_error_elsewhere_keeps_clap_text() {
    let argv = v(&["dagon", "status", "extra-word"]);
    let e = Cli::try_parse_from(&argv).expect_err("status extra-word");
    let text = parse_error_text(&argv, &e);
    assert_eq!(text, e.to_string());
    assert!(text.contains("extra-word"), "{text}");
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

/// `dagon <word>` in backticks or at the start of a code line of the repo's
/// documents is a command this CLI has.
#[test]
fn documents_name_only_commands_the_cli_has() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let known: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    let mut files = vec![
        root.join("CHANGELOG.md"),
        root.join("README.md"),
        root.join("README.en.md"),
    ];
    let mut dirs = vec![root.join("site/src/content/docs"), root.join("samples")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                files.push(p);
            }
        }
    }
    let mut bad = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for line in text.lines() {
            let from_tick = line.split("`dagon ").skip(1).map(str::to_string);
            let from_start = line.strip_prefix("dagon ").map(str::to_string);
            for rest in from_tick.chain(from_start) {
                let word: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                if word.starts_with(|c: char| c.is_ascii_alphabetic()) && !known.contains(&word) {
                    bad.push(format!("{}: dagon {word}", f.display()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
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

/// The `--long` option names written in `text`.
fn flags_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    let mut prev = ' ';
    while let Some(i) = rest.find("--") {
        if let Some(c) = rest[..i].chars().last() {
            prev = c;
        }
        let name: String = rest[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        let standalone = !(prev.is_ascii_alphanumeric() || prev == '-');
        if standalone && name.starts_with(|c: char| c.is_ascii_lowercase()) {
            found.push(name.trim_end_matches('-').to_string());
        }
        rest = &rest[i + 2 + name.len()..];
        prev = '-';
    }
    found
}

fn leaf<'a>(root: &'a clap::Command, path: &[String]) -> &'a clap::Command {
    path.iter()
        .fold(root, |c, n| c.find_subcommand(n).expect("subcommand"))
}

fn has_long(c: &clap::Command, flag: &str) -> bool {
    flag == "help" || flag == "version" || c.get_arguments().any(|a| a.get_long() == Some(flag))
}

#[test]
fn every_flag_a_help_text_names_is_one_the_command_has() {
    let mut root = Cli::command();
    root.build();
    for path in leaf_paths(&root, &[]) {
        let c = leaf(&root, &path);
        let text = c.clone().render_long_help().to_string();
        for f in flags_in(&text) {
            assert!(
                has_long(c, &f),
                "dagon {}: help names --{f}, which the command lacks",
                path.join(" ")
            );
        }
    }
}

#[test]
fn a_no_topology_error_names_only_flags_its_commands_have() {
    let cfg = crate::config::Config::default();
    let mut root = Cli::command();
    root.build();
    // `dagon gen` takes its topology from dagon.toml alone.
    let err = cfg
        .configured_topology()
        .expect_err("no topology configured");
    let gen = leaf(&root, &["gen".to_string()]);
    for f in flags_in(&err) {
        assert!(has_long(gen, &f), "{err:?} names --{f}, which gen lacks");
    }
    // Every command with --topology is told it.
    let err = cfg.topology_path(None).expect_err("no topology given");
    assert!(err.contains("--topology"), "{err:?}");
    let with_flag: Vec<_> = leaf_paths(&root, &[])
        .into_iter()
        .filter(|p| has_long(leaf(&root, p), "topology"))
        .collect();
    assert!(!with_flag.is_empty());
    for p in &with_flag {
        for f in flags_in(&err) {
            assert!(
                has_long(leaf(&root, p), &f),
                "dagon {}: {err:?} names --{f}",
                p.join(" ")
            );
        }
    }
}

#[test]
fn conform_help_counts_the_tests_there_are() {
    let mut root = Cli::command();
    root.build();
    let about = leaf(&root, &["conform".to_string()])
        .get_about()
        .unwrap()
        .to_string();
    assert!(
        about.contains(&format!("{} conformance", dagon_conform::CTS.len())),
        "{about}"
    );
}

#[test]
fn slot_and_kind_help_list_what_the_parser_takes() {
    let mut root = Cli::command();
    root.build();
    let sign = leaf(&root, &["image".to_string()]);
    let help_of = |id: &str| {
        sign.get_arguments()
            .find(|a| a.get_long() == Some(id))
            .and_then(|a| a.get_help())
            .expect("help")
            .to_string()
    };
    for l in crate::commands::slot_letters() {
        assert!(help_of("slot").contains(l), "--slot help lacks {l}");
    }
    for (name, _) in dagon_sign::image::IMAGE_KIND_NAMES {
        assert!(help_of("kind").contains(name), "--kind help lacks {name}");
    }
}

/// Headings and the commands under them, from the text `dagon --help` prints.
fn help_sections(help: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for l in help.lines() {
        if let Some(name) = l.strip_prefix("  ") {
            if let Some(last) = out.last_mut() {
                let first = name.split_whitespace().next().unwrap_or("");
                last.1.push(first.to_string());
            }
        } else if let Some(h) = l.strip_suffix(':') {
            out.push((h.to_string(), Vec::new()));
        }
    }
    out
}

#[test]
fn top_help_lists_the_commands_under_task_headings() {
    let help = super::command().render_long_help().to_string();
    let want: Vec<(String, Vec<String>)> = [
        ("Set up a project", vec!["new", "gen", "key", "doctor"]),
        (
            "Build a release",
            vec!["topo", "build", "image", "release", "provision"],
        ),
        (
            "Write to a node",
            vec!["flash", "deploy", "apply", "rollback"],
        ),
        ("Check a node", vec!["status", "conform"]),
        ("Tools", vec!["serve", "sim", "ai"]),
        ("Help", vec!["help"]),
    ]
    .into_iter()
    .map(|(h, c)| (h.to_string(), c.into_iter().map(String::from).collect()))
    .collect();
    let mut got = help_sections(&help);
    let last = got.pop().map(|(h, _)| h);
    assert_eq!(last.as_deref(), Some("Options"), "{help}");
    assert_eq!(got, want, "{help}");
    assert!(help.contains("Usage: dagon"), "{help}");
    assert!(
        help.contains("`dagon <command> --help` describes"),
        "{help}"
    );
}

#[test]
fn headings_cover_every_visible_command_once_in_markdown_order() {
    let mut root = super::command();
    root.build();
    let visible: Vec<String> = root
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .map(|c| c.get_name().to_string())
        .collect();
    let listed: Vec<String> = super::GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter())
        .filter(|n| **n != "help")
        .map(|n| n.to_string())
        .collect();
    assert_eq!(listed, visible);
    let md = super::markdown();
    let at = |n: &str| md.find(&format!("## dagon {n}\n")).expect(n);
    for w in visible.windows(2) {
        assert!(at(&w[0]) < at(&w[1]), "{} before {}", w[0], w[1]);
    }
}
