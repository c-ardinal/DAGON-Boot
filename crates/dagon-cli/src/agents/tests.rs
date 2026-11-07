use std::time::Duration;

use super::*;

const T: Duration = Duration::from_secs(30);

fn tmp_home(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dagon-agents-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".dagon").join(USER_DIR)).unwrap();
    d
}

fn prompt(system: &str, user: &str) -> Prompt {
    Prompt {
        system: system.into(),
        user: user.into(),
        model: None,
    }
}

/// A program that prints a file, and one that copies stdin to stdout.
fn printer(file: &Path) -> (String, Vec<String>) {
    let f = file.display().to_string();
    if cfg!(windows) {
        ("cmd".into(), vec!["/C".into(), "type".into(), f])
    } else {
        ("cat".into(), vec![f])
    }
}

fn echo() -> command::Command {
    let (program, args) = if cfg!(windows) {
        // sort copies stdin to stdout (in line order; the test text sorts as sent).
        ("sort", Vec::new())
    } else {
        ("cat", Vec::new())
    };
    command::Command {
        program: program.into(),
        args,
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    }
}

fn toml_list(args: &[String]) -> String {
    // TOML literal strings: a Windows path keeps its backslashes.
    let quoted: Vec<String> = args.iter().map(|a| format!("'{a}'")).collect();
    format!("[{}]", quoted.join(", "))
}

#[test]
fn the_builtin_agent_is_claude_with_two_ways_claude_code_first() {
    let a = Agents::load(None);
    assert!(a.errors.is_empty(), "{:?}", a.errors);
    let (agent, way) = a.default_way();
    assert_eq!(
        (agent.id.as_str(), way.id.as_str()),
        ("claude", "claude-code")
    );
    assert_eq!(agent.label(), "Claude (Anthropic)");
    let Transport::Command(c) = &way.transport else {
        panic!("claude-code is a command")
    };
    assert_eq!(c.program, "claude");
    for flag in [
        "-p",
        "--no-session-persistence",
        "--safe-mode",
        "--strict-mcp-config",
        "--disable-slash-commands",
    ] {
        assert!(c.args.iter().any(|x| x == flag), "{flag}");
    }
    let after = |f: &str| c.args[c.args.iter().position(|x| x == f).unwrap() + 1].clone();
    assert_eq!(after("--tools"), "", "every tool disabled");
    assert_eq!(after("--permission-mode"), "dontAsk");
    assert_eq!(after("--permission-prompts"), "none");
    assert_eq!(after("--system-prompt-file"), "{system_file}");
    let (_, api) = a.way("claude", "api").unwrap();
    let Transport::MessagesHttp(h) = &api.transport else {
        panic!("api is messages-http")
    };
    assert!(h.url.starts_with("https://") && api.transport.takes_key());
    assert!(
        !way.transport.takes_key(),
        "Claude Code uses the user's own login"
    );
    assert!(secret_env(None).contains(&"ANTHROPIC_API_KEY".to_string()));
    assert!(secret_env(None).contains(&crate::config::KEY_ENV.to_string()));
}

#[test]
fn user_definitions_are_checked_and_reported_by_file_name_only() {
    let h = tmp_home("defs");
    let dir = h.join(".dagon").join(USER_DIR);
    let ok = "dagon_agent = 1\nid = \"local\"\nname = \"Local\"\nprovider = \"Me\"\n\
              [[ways]]\nid = \"run\"\nname = \"Run\"\n\
              [ways.transport]\nkind = \"command\"\nprogram = \"local-agent\"\n";
    std::fs::write(dir.join("a-local.toml"), ok).unwrap();
    for (name, text) in [
        ("b-unknown-field.toml", ok.replace("provider = \"Me\"", "provider = \"Me\"\nshell = true")),
        ("c-unknown-kind.toml", ok.replace("\"command\"", "\"shell\"")),
        ("d-bad-id.toml", ok.replace("id = \"local\"", "id = \"Local Agent\"")),
        ("e-again.toml", ok.to_string()),
        ("f-claude.toml", ok.replace("id = \"local\"", "id = \"claude\"")),
        ("g-http.toml", ok.replace("kind = \"command\"\nprogram = \"local-agent\"", "kind = \"messages-http\"\nurl = \"http://x\"\nversion = \"1\"\nmodel = \"m\"")),
        ("m-key-env.toml", ok.replace("kind = \"command\"\nprogram = \"local-agent\"", "kind = \"messages-http\"\nurl = \"https://x\"\nversion = \"1\"\nkey_env = \"K\"\nmodel = \"m\"")),
        ("h-placeholder.toml", ok.replace("program = \"local-agent\"", "program = \"x\"\nargs = [\"--in={user}\"]")),
        ("i-no-ways.toml", "dagon_agent = 1\nid = \"x\"\nname = \"X\"\nprovider = \"P\"\nways = []\n".into()),
        ("k-newer-format.toml", ok.replace("dagon_agent = 1", "dagon_agent = 2")),
        ("l-no-format.toml", ok.replace("dagon_agent = 1\n", "")),
        ("j-transport-field.toml", ok.replace("program = \"local-agent\"", "program = \"x\"\nenv = \"A=1\"")),
    ] {
        std::fs::write(dir.join(name), text).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let a = Agents::load(Some(&h));
    assert_eq!(a.list.len(), 2, "{:?}", a.errors);
    assert!(a.way("local", "run").is_some());
    assert_eq!(a.errors.len(), 12, "{:?}", a.errors);
    // A key is never named by an environment variable: `key_env` is refused.
    let key_env = a
        .errors
        .iter()
        .find(|e| e.starts_with("m-key-env.toml"))
        .unwrap();
    assert!(key_env.contains("key_env"), "{key_env}");
    let newer = a
        .errors
        .iter()
        .find(|e| e.starts_with("k-newer-format.toml"))
        .unwrap();
    assert!(
        newer.contains("dagon_agent = 2 is not a format this dagon reads (it reads 1)"),
        "{newer}"
    );
    for e in &a.errors {
        assert!(
            !e.contains(&*h.to_string_lossy()) && !e.contains("Users"),
            "{e}"
        );
    }
    assert!(a.errors[3].starts_with("e-again.toml: agent local is defined already"));
    std::fs::remove_dir_all(&h).unwrap();
}

#[test]
fn replies_and_conversations() {
    assert_eq!(
        reply_json("{\"a\":1}").unwrap(),
        serde_json::json!({ "a": 1 })
    );
    assert_eq!(
        reply_json("Here:\n```json\n{\"a\":{\"b\":2}}\n```").unwrap()["a"]["b"],
        2
    );
    for bad in ["", "no", "[1]", "{not json}", "} {"] {
        assert_eq!(reply_json(bad).unwrap_err(), UNEXPECTED, "{bad}");
    }
    assert_eq!(conversation(&["one".into()]), "one");
    let c = conversation(&["q".into(), "r".into(), "fix".into()]);
    assert_eq!(
        c,
        "<turn from=\"dagon\">\nq\n</turn>\n<turn from=\"you\">\nr\n</turn>\n<turn from=\"dagon\">\nfix\n</turn>"
    );
}

#[test]
fn every_call_is_checked_for_consent_and_size_first() {
    let h = tmp_home("gate");
    let small = prompt("s", "u");
    assert_eq!(
        before_call(&h, &small).err().unwrap(),
        crate::consent::NOT_LINKED
    );
    assert_eq!(
        call(&h, &crate::keystore::Keys::double(), &small, T).unwrap_err(),
        crate::consent::NOT_LINKED
    );
    crate::consent::link(&h, "claude", "api").unwrap();
    assert!(before_call(&h, &small).is_ok());
    let big = prompt(&"s".repeat(MAX_REQUEST), "u");
    assert_eq!(before_call(&h, &big).err().unwrap(), TOO_LARGE);
    assert_eq!(
        call(&h, &crate::keystore::Keys::double(), &big, T).unwrap_err(),
        TOO_LARGE
    );
    std::fs::remove_dir_all(&h).unwrap();
}

#[test]
fn a_command_gets_the_prompt_on_stdin_and_a_fresh_working_folder() {
    let out = command::run_unchecked(&echo(), &prompt("SYSTEM-TEXT", "USER-TEXT"), T).unwrap();
    let (s, u) = (
        out.find("SYSTEM-TEXT").unwrap(),
        out.find("USER-TEXT").unwrap(),
    );
    assert!(s < u, "{out}");

    let (program, mut args) = if cfg!(windows) {
        (
            "cmd".to_string(),
            vec!["/C".to_string(), "type".to_string()],
        )
    } else {
        ("cat".to_string(), Vec::new())
    };
    args.push("{system_file}".into());
    let file = command::Command {
        program,
        args,
        system: command::SystemVia::File,
        pass_env: Vec::new(),
    };
    let out = command::run_unchecked(&file, &prompt("ONLY-SYSTEM", "USER"), T).unwrap();
    assert_eq!(out.trim(), "ONLY-SYSTEM");

    // Its working folder is a fresh temporary one, removed afterwards
    // (not the project's; this is hygiene, not a sandbox).
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C".to_string(), "cd".to_string()])
    } else {
        ("pwd", Vec::new())
    };
    let pwd = command::Command {
        program: program.into(),
        args,
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    };
    let dir = command::run_unchecked(&pwd, &prompt("", ""), T).unwrap();
    let dir = PathBuf::from(dir.trim());
    assert!(
        dir.starts_with(std::env::temp_dir()) || dir.to_string_lossy().contains("dagon-agent-")
    );
    assert!(!dir.exists(), "{}", dir.display());

    // Only the allow-listed environment reaches it (cargo sets
    // CARGO_MANIFEST_DIR for this test process).
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C".to_string(), "set".to_string()])
    } else {
        ("env", Vec::new())
    };
    let env = command::Command {
        program: program.into(),
        args,
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    };
    let seen = command::run_unchecked(&env, &prompt("", ""), T)
        .unwrap()
        .to_ascii_uppercase();
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(
        !seen.contains("CARGO_MANIFEST_DIR") && seen.contains("PATH="),
        "{seen}"
    );
}

#[test]
fn a_command_that_hangs_fails_or_is_missing_gives_a_plain_error() {
    let (program, args) = if cfg!(windows) {
        (
            "ping",
            vec!["-n".to_string(), "20".to_string(), "127.0.0.1".to_string()],
        )
    } else {
        ("sleep", vec!["20".to_string()])
    };
    let slow = command::Command {
        program: program.into(),
        args,
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    };
    let t0 = std::time::Instant::now();
    let e = command::run_unchecked(&slow, &prompt("", ""), Duration::from_millis(300)).unwrap_err();
    assert_eq!(e, TIMED_OUT);
    assert!(t0.elapsed() < Duration::from_secs(10));

    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C".to_string(), "exit 3".to_string()])
    } else {
        ("sh", vec!["-c".to_string(), "exit 3".to_string()])
    };
    let fails = command::Command {
        program: program.into(),
        args,
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    };
    let e = command::run_unchecked(&fails, &prompt("", ""), T).unwrap_err();
    assert_eq!(e, "the agent program ended with exit code 3");

    let missing = command::Command {
        program: "dagon-no-such-agent-program".into(),
        args: Vec::new(),
        system: command::SystemVia::Stdin,
        pass_env: Vec::new(),
    };
    assert!(missing.program_path().is_none());
}

/// An agent added as data only: one TOML file in ~/.dagon/agents naming a
/// program of kind `command`, linked, then asked through [`call`] and the
/// whole question path.
#[test]
fn an_agent_defined_only_in_a_toml_file_answers_through_command() {
    let h = tmp_home("fake");
    let answer = h.join("answer.json");
    std::fs::write(&answer, r#"{"answer":"偽のエージェントです。"}"#).unwrap();
    let (program, args) = printer(&answer);
    let def = format!(
        "dagon_agent = 1\nid = \"fake\"\nname = \"Fake\"\nprovider = \"Test\"\n\
         [[ways]]\nid = \"print\"\nname = \"Print\"\n\
         [ways.transport]\nkind = \"command\"\nprogram = {program:?}\nargs = {}\n",
        toml_list(&args)
    );
    std::fs::write(h.join(".dagon").join(USER_DIR).join("fake.toml"), def).unwrap();
    let agents = Agents::load(Some(&h));
    assert!(agents.errors.is_empty(), "{:?}", agents.errors);
    let (_, way) = agents.way("fake", "print").unwrap();
    assert!(way.available("fake", &crate::keystore::Keys::double()));
    crate::consent::link(&h, "fake", "print").unwrap();
    let keys = crate::keystore::Keys::double();
    assert!(ready(&h, &keys).is_ok());
    let text = call(&h, &keys, &prompt("s", "u"), T).unwrap();
    assert_eq!(
        reply_json(&text).unwrap()["answer"],
        "偽のエージェントです。"
    );

    let q = crate::ask::parse(&serde_json::json!({ "question": "次は？" })).unwrap();
    let facts = crate::ask::Facts {
        reg: dagon_topology::defs::Registry::builtin(),
        topology: None,
        jobs: Vec::new(),
    };
    let mut api = |p: &Prompt| call(&h, &keys, p, T);
    let r = crate::ask::ask(&q, &facts, &mut api).unwrap();
    assert_eq!(r.answer, "偽のエージェントです。");
    // Unlinked: the same agent is not called again.
    crate::consent::unlink(&h).unwrap();
    assert_eq!(
        call(&h, &keys, &prompt("s", "u"), T).unwrap_err(),
        crate::consent::NOT_LINKED
    );
    std::fs::remove_dir_all(&h).unwrap();
}

/// Manual (network, the user's Claude Code login): the builtin claude-code
/// way through DAGON's own spawn path (allow-listed environment, fresh
/// folder, system file). One line in, no project data.
/// `cargo test -p dagon-cli claude_code_answers -- --ignored`
#[test]
#[ignore]
fn claude_code_answers_through_the_command_path() {
    let a = Agents::load(None);
    let (_, way) = a.way("claude", "claude-code").unwrap();
    let Transport::Command(c) = &way.transport else {
        panic!("command")
    };
    let p = prompt(
        "Reply with one JSON object and nothing else.",
        "Reply with exactly {\"answer\":\"ok\"}.",
    );
    let out = command::run_unchecked(c, &p, Duration::from_secs(180)).unwrap();
    assert_eq!(reply_json(&out).unwrap()["answer"], "ok", "{out}");
}

fn doc() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../site/src/content/docs/en/reference/agent-api.md"),
    )
    .unwrap()
}

/// Every file in agents/ is builtin, and nothing else is: adding a builtin
/// agent is a new file plus its line in BUILTIN, which this test demands.
#[test]
fn the_builtin_list_is_the_agents_folder() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("agents");
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    files.sort();
    let mut listed: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    listed.sort();
    assert_eq!(files, listed);
}

/// The top-level bullets of the section under `heading` (up to the next
/// heading of the same or a higher level).
fn bullets(doc: &str, heading: &str) -> Vec<String> {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut lines = doc.lines().skip_while(|l| *l != heading);
    assert!(lines.next().is_some(), "no section {heading}");
    lines
        .take_while(|l| {
            let h = l.chars().take_while(|c| *c == '#').count();
            h == 0 || h > level
        })
        .filter_map(|l| l.strip_prefix("- "))
        .map(str::to_string)
        .collect()
}

/// The backticked words of a bullet, in order.
fn ticked(line: &str) -> Vec<String> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Every transport kind in KINDS parses as itself; the builtin
/// definitions use only these kinds.
#[test]
fn transport_kind_names_round_trip() {
    for kind in KINDS {
        let text = match kind {
            "command" => "kind = \"command\"\nprogram = \"x\"\n".to_string(),
            "messages-http" => {
                "kind = \"messages-http\"\nurl = \"https://x\"\nversion = \"1\"\nmodel = \"m\"\n"
                    .to_string()
            }
            other => panic!("give the test a minimal {other} transport"),
        };
        let t: Transport = toml::from_str(&text).unwrap();
        assert_eq!(t.kind(), kind);
    }
}

/// The authoritative sections of site/src/content/docs/en/reference/agent-api.md say exactly what the code
/// has: an extra, missing or stale entry fails.
#[test]
fn the_contract_document_follows_the_code() {
    let d = doc();
    // Error codes: every AGENT_* word in the document, against the one list.
    let mut in_doc: Vec<&str> = d
        .split(|c: char| !(c.is_ascii_uppercase() || c == '_'))
        .filter(|w| w.starts_with("AGENT_") && w.len() > 6)
        .collect();
    in_doc.sort();
    in_doc.dedup();
    let mut all = codes::ALL.to_vec();
    all.sort();
    assert_eq!(in_doc, all, "error codes");

    let kinds: Vec<String> = bullets(&d, "## Transport kinds")
        .iter()
        .map(|b| ticked(b)[0].clone())
        .collect();
    assert_eq!(kinds, KINDS, "transport kinds");

    let sends = bullets(&d, "### Sends");
    assert_eq!(sends, crate::consent::SENDS, "sends");
    let never = bullets(&d, "### Never sends");
    assert_eq!(never, crate::consent::NEVER_SENDS, "never sends");

    let formats: Vec<String> = bullets(&d, "## Format numbers")
        .iter()
        .map(|b| ticked(b)[0].clone())
        .collect();
    assert_eq!(
        formats,
        [
            format!("dagon_agent = {FORMAT}"),
            format!("\"version\": {}", crate::consent::FILE_VERSION),
        ],
        "format numbers"
    );

    let env = d.split("Environment: only").nth(1).unwrap();
    let env = env.split("plus the names").next().unwrap();
    let mut listed: Vec<String> = ticked(env).into_iter().filter(|w| w != "LC_*").collect();
    listed.sort();
    let mut base: Vec<String> = command::BASE_ENV.iter().map(|s| s.to_string()).collect();
    base.sort();
    assert_eq!(listed, base, "environment allow-list");
    assert!(env.contains("`LC_*`"));
}

/// Product code (not its tests) of dagon-cli and the builtin definitions:
/// (file, line number, line) of every line before a file's test module.
fn product_lines() -> Vec<(String, usize, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs" || x == "toml") {
                out.push(p);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("src"), &mut files);
    walk(&root.join("agents"), &mut files);
    let mut out = Vec::new();
    for f in files
        .iter()
        .filter(|f| f.file_name().is_some_and(|n| n != "tests.rs"))
    {
        let name = f.strip_prefix(root).unwrap().display().to_string();
        let text = std::fs::read_to_string(f).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // A file's test module comes last: what follows is test code.
            let test_mod = line.trim() == "#[cfg(test)]"
                && lines[i + 1..]
                    .iter()
                    .take(3)
                    .any(|n| n.trim_start().starts_with("mod tests"));
            if test_mod {
                break;
            }
            out.push((name.clone(), i + 1, line.to_string()));
        }
    }
    out
}

/// An agent key is read only from the OS credential store: no definition
/// names a key variable (`key_env`), and no product code reads
/// ANTHROPIC_API_KEY or any `*_API_KEY` variable. The one line that may
/// name ANTHROPIC_API_KEY is [`NEVER_PASSED`], which keeps it from children.
#[test]
fn no_code_reads_an_agent_key_from_the_environment() {
    let mut found = Vec::new();
    for (file, n, line) in product_lines() {
        let named = line.contains("key_env") || line.contains("ANTHROPIC_API_KEY");
        let reads =
            (line.contains("env::var") || line.contains("var_os(")) && line.contains("API_KEY");
        let never_passed = line.contains("NEVER_PASSED") && line.contains("ANTHROPIC_API_KEY");
        if (named || reads) && !never_passed {
            found.push(format!("{file}:{n}: {}", line.trim()));
        }
    }
    assert!(
        found.is_empty(),
        "an agent key read from the environment:\n{}",
        found.join("\n")
    );
}
