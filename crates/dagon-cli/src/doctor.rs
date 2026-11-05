//! `dagon doctor [--fix]`: the tools each MCU of the topology needs (every
//! MCU defined without a topology). Rust itself is checked here; the rest
//! are the MCU definitions' `[[doctor]]` entries. `--fix` runs their install
//! commands (no administrator rights: `rustup target add`, `cargo install`,
//! ...); drivers and the like are left to the developer. Commands from the
//! project's own definitions run only after the user agrees to each one.
//! With a hub connected (found, or `--port`), each node's versions follow
//! (VERSION, as in `dagon status`); without one, a line says why.

use std::process::Command;

use dagon_topology::defs::{DoctorCheck, Expect, Origin, Registry};

use crate::cli::DoctorArgs;
use crate::config::{read_topology, Config};

const RUST_HINT: &str = "install Rust from https://rustup.rs";

/// A check and the MCUs that need it (none: every MCU).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    def: DoctorCheck,
    for_mcu: Vec<String>,
    /// Defined by the project: its commands run only once the user agrees.
    ask: bool,
    /// Its place (from 1) among its first MCU's `[[doctor]]` entries.
    index: usize,
}

fn rust(what: &str) -> Check {
    Check {
        def: DoctorCheck {
            what: what.into(),
            probe: vec![what.into(), "--version".into()],
            expect: Expect::Runs,
            fix: None,
            hint: RUST_HINT.into(),
            slow: false,
        },
        for_mcu: Vec::new(),
        ask: false,
        index: 0,
    }
}

/// Rust, then each MCU's `[[doctor]]` entries; an entry several MCUs define
/// alike is one check naming them all. MCUs go by image header id, so the
/// order is the same whether the definitions are builtin or a source tree's.
fn checks(reg: &Registry, mcus: &[String]) -> Vec<Check> {
    let mut out = vec![rust("rustup"), rust("cargo")];
    let mut wanted: Vec<_> = reg.mcus().iter().filter(|m| mcus.contains(&m.id)).collect();
    wanted.sort_by_key(|m| m.wire_id);
    for m in wanted {
        let ask = reg.mcu_origin(&m.id) == Some(Origin::Project);
        for (i, d) in m.doctor.iter().enumerate() {
            match out.iter_mut().find(|c| c.def == *d) {
                Some(c) => {
                    c.for_mcu.push(m.id.clone());
                    c.ask |= ask;
                }
                None => out.push(Check {
                    def: d.clone(),
                    for_mcu: vec![m.id.clone()],
                    ask,
                    index: i + 1,
                }),
            }
        }
    }
    out
}

/// Every MCU `reg` defines.
pub(crate) fn all_mcus(reg: &Registry) -> Vec<String> {
    reg.mcus().iter().map(|m| m.id.clone()).collect()
}

/// Runs a program: its stdout when it ran and succeeded.
pub(crate) type Runner<'a> = dyn FnMut(&[&str]) -> Option<String> + 'a;

/// Asks whether a project-defined command may run (shown in the question).
pub(crate) type Consent<'a> = dyn FnMut(&str) -> bool + 'a;

fn args(cmd: &[String]) -> Vec<&str> {
    cmd.iter().map(String::as_str).collect()
}

fn passes(c: &DoctorCheck, run: &mut Runner<'_>) -> bool {
    let Some(out) = run(&args(&c.probe)) else {
        return false;
    };
    let mut lines = out.lines().map(str::trim);
    match &c.expect {
        Expect::Runs => true,
        Expect::Line(l) => lines.any(|x| x == l),
        Expect::Prefix(p) => lines.any(|x| x.starts_with(p.as_str())),
    }
}

/// Whether a command of a check may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Run,
    /// The user said no.
    Declined,
    /// Nobody to ask (`dagon serve`): only the terminal's `dagon doctor` may.
    Unattended,
}

/// Whether `cmd` of check `c` may run: always for builtin and DAGON source
/// definitions, after the user agrees for the project's.
fn gate(c: &Check, cmd: &[String], consent: &mut Option<&mut Consent<'_>>) -> Gate {
    if !c.ask {
        return Gate::Run;
    }
    let Some(ask) = consent else {
        return Gate::Unattended;
    };
    let question = format!(
        "the project's definition of {} ({}) runs: {}\n",
        c.def.what,
        c.for_mcu.join(", "),
        cmd.join(" ")
    );
    if ask(&question) {
        Gate::Run
    } else {
        Gate::Declined
    }
}

/// A project check's name where nobody can confirm it (the page): only its
/// place and MCU ids (a project's ids are plain, checked on load).
fn unattended_name(c: &Check) -> String {
    format!("project check {} for {}", c.index, c.for_mcu.join(", "))
}

/// Everything [`doctor`] does outside: run probes and installs, ask, print.
/// `consent: None` asks nobody: project-defined commands do not run.
pub(crate) struct Io<'a> {
    pub probe: &'a mut Runner<'a>,
    pub install: &'a mut Runner<'a>,
    pub consent: Option<&'a mut Consent<'a>>,
    pub out: &'a mut dyn FnMut(&str),
}

/// One line per check for `mcus`; `false` when something is still missing
/// or was not checked. With `fix`, missing tools that have an install
/// command are installed (through `install`) and checked again.
pub(crate) fn doctor(reg: &Registry, mcus: &[String], fix: bool, io: &mut Io<'_>) -> bool {
    let mut ok = true;
    for c in checks(reg, mcus) {
        ok &= one(&c, fix, io);
    }
    ok
}

/// Runs check `c`, and its fix when asked to; prints its line.
fn one(c: &Check, fix: bool, io: &mut Io<'_>) -> bool {
    let d = &c.def;
    let used_by = if c.for_mcu.is_empty() {
        String::new()
    } else {
        format!(" ({})", c.for_mcu.join(", "))
    };
    match gate(c, &d.probe, &mut io.consent) {
        Gate::Run => {}
        Gate::Declined => {
            (io.out)(&format!(
                "SKIP  {}{used_by}  not run; to check it yourself: {}",
                d.what,
                d.probe.join(" ")
            ));
            return false;
        }
        Gate::Unattended => {
            (io.out)(&format!(
                "SKIP  {}  needs confirmation in the terminal (dagon doctor)",
                unattended_name(c)
            ));
            return false;
        }
    }
    let mut good = passes(d, io.probe);
    let mut fixed = false;
    if !good && fix {
        let fix_cmd = d.fix.as_deref();
        if let Some(cmd) = fix_cmd.filter(|cmd| gate(c, cmd, &mut io.consent) == Gate::Run) {
            if d.slow {
                (io.out)(&format!("installing the {}; this takes a while", d.what));
            }
            (io.out)(&format!("fix   {}", cmd.join(" ")));
            good = (io.install)(&args(cmd)).is_some() && passes(d, io.probe);
            fixed = true;
        }
    }
    let advice = match (good, &d.fix) {
        (true, _) => String::new(),
        (false, Some(_)) if fixed => "  the install did not help; see its output".into(),
        (false, Some(cmd)) if !fix => {
            format!("  fix: {} (or dagon doctor --fix)", cmd.join(" "))
        }
        (false, Some(cmd)) => format!("  fix: {}", cmd.join(" ")),
        (false, None) => format!("  {}", d.hint),
    };
    (io.out)(&format!(
        "{}  {}{used_by}{advice}",
        if good { "ok  " } else { "MISS" },
        d.what
    ));
    good
}

/// `reg`'s checks for `mcus` (no installs, nobody to ask), one line each to
/// `out`: a project-defined check is reported as needing the terminal's
/// `dagon doctor`, never run.
pub(crate) fn check(reg: &Registry, mcus: &[String], out: &mut dyn FnMut(&str)) -> bool {
    let mut io = Io {
        probe: &mut probe_cmd,
        install: &mut |_| None,
        consent: None,
        out,
    };
    doctor(reg, mcus, false, &mut io)
}

fn probe_cmd(args: &[&str]) -> Option<String> {
    let o = Command::new(args[0]).args(&args[1..]).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Installs show their output; only success matters.
fn install_cmd(args: &[&str]) -> Option<String> {
    let s = Command::new(args[0]).args(&args[1..]).status().ok()?;
    s.success().then(String::new)
}

/// `dagon doctor`: the tool checks, then each node's versions when a hub is connected.
pub fn command(a: &DoctorArgs) -> Result<(), String> {
    let fix = a.fix;
    let cfg = Config::load(a.config.as_deref())?;
    let reg = crate::defs::registry(&cfg)?;
    println!("{}", crate::versions::known_text());
    let d = &cfg.dagon;
    match (&d.tag, &d.path) {
        (Some(tag), _) => println!("this project's DAGON source: release {tag}"),
        (None, Some(p)) => println!("this project's DAGON source: local clone {}", p.display()),
        (None, None) => {}
    }
    let topology = match a.topology.as_ref().map(Into::into).or(cfg.topology.clone()) {
        Some(p) => {
            let t = read_topology(&cfg, &p)?;
            println!("checking for {}", p.display());
            Some(t)
        }
        None => {
            println!("no topology: checking every family");
            None
        }
    };
    let mcus: Vec<String> = match &topology {
        Some(t) => t.nodes.iter().map(|n| n.mcu.clone()).collect(),
        None => all_mcus(&reg),
    };
    let mut io = Io {
        probe: &mut probe_cmd,
        install: &mut install_cmd,
        consent: Some(&mut |q| crate::deploy::confirm(q, &mut std::io::stdin().lock())),
        out: &mut |l| println!("{l}"),
    };
    let ok = doctor(&reg, &mcus, fix, &mut io);
    for l in crate::device::doctor_versions(a.port.as_deref(), &cfg, topology.as_ref()) {
        println!("{l}");
    }
    if ok {
        Ok(())
    } else {
        Err("some prerequisites are missing; see above".into())
    }
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    /// Programs that exist (by first word) and what they print.
    struct Fake {
        tools: RefCell<BTreeMap<String, String>>,
        installed: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(tools: &[(&str, &str)]) -> Fake {
            Fake {
                tools: RefCell::new(
                    tools
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                installed: RefCell::default(),
            }
        }

        fn probe(&self, args: &[&str]) -> Option<String> {
            self.tools.borrow().get(&args.join(" ")).cloned()
        }

        /// Install `args`: the probes `adds` lists for it start answering.
        fn install(&self, args: &[&str], adds: &[Adds]) -> Option<String> {
            let cmd = args.join(" ");
            let mut t = self.tools.borrow_mut();
            for (_, k, v) in adds.iter().filter(|a| a.0 == cmd) {
                t.entry(k.to_string()).or_default().push_str(v);
            }
            self.installed.borrow_mut().push(cmd);
            Some(String::new())
        }
    }

    /// (install command, probe it fixes, text the probe then prints).
    type Adds = (&'static str, &'static str, &'static str);

    fn run(fake: &Fake, mcus: &[&str], fix: bool, adds: &[Adds]) -> (bool, Vec<String>) {
        let (ok, lines, asked) = run_in(Registry::builtin(), fake, mcus, fix, adds, true);
        assert!(asked.is_empty(), "builtin checks never ask: {asked:?}");
        (ok, lines)
    }

    /// [`doctor`] over `reg`, answering every question with `agree`; the
    /// questions asked come back too.
    fn run_in(
        reg: &Registry,
        fake: &Fake,
        mcus: &[&str],
        fix: bool,
        adds: &[Adds],
        agree: bool,
    ) -> (bool, Vec<String>, Vec<String>) {
        let mcus: Vec<String> = mcus.iter().map(|s| s.to_string()).collect();
        let mut lines = Vec::new();
        let mut asked = Vec::new();
        let ok = doctor(
            reg,
            &mcus,
            fix,
            &mut Io {
                probe: &mut |a| fake.probe(a),
                install: &mut |a| fake.install(a, adds),
                consent: Some(&mut |q| {
                    asked.push(q.to_string());
                    agree
                }),
                out: &mut |l| lines.push(l.to_string()),
            },
        );
        (ok, lines, asked)
    }

    /// The builtin definitions plus a project's `rp2040b`: an RP2040 copy
    /// under another id, registered from its dagon.toml.
    fn project_registry(name: &str, extra: &str) -> Registry {
        let dir = std::env::temp_dir().join(format!("dagon-doctor-{name}-{}", std::process::id()));
        for sub in ["m/stage0", "m/bootcore"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let toml = include_str!("../../../targets/mcu/rp2040/dagon-mcu.toml")
            .replace("id = \"rp2040\"", "id = \"rp2040b\"")
            .replace("wire_id = 0x0001", "wire_id = 0x00F1")
            .replace("name = \"rp2040-4m\"", "name = \"rp2040b-4m\"");
        std::fs::write(dir.join("m/dagon-mcu.toml"), format!("{toml}{extra}")).unwrap();
        std::fs::write(
            dir.join("dagon.toml"),
            "mcu_files = [\"m/dagon-mcu.toml\"]
",
        )
        .unwrap();
        let reg = Registry::load(&[dagon_topology::defs::Source::Project(
            &dir.join("dagon.toml"),
        )]);
        std::fs::remove_dir_all(&dir).unwrap();
        reg.unwrap()
    }

    #[test]
    fn with_nobody_to_ask_project_checks_point_to_the_terminal() {
        let secret = "C:/Users/me/sk-ant-FAKE-TOKEN";
        let reg = project_registry(
            "unattended",
            &format!(
                "
[[doctor]]
what = \"{secret}\"
probe = [\"{secret}\"]
expect = \"runs\"
hint = \"{secret}\"
"
            ),
        );
        let fake = Fake::new(&[RUST[0], RUST[1]]);
        let probed = RefCell::new(Vec::new());
        let mut lines = Vec::new();
        let ok = doctor(
            &reg,
            &["rp2040b".to_string()],
            true,
            &mut Io {
                probe: &mut |a| {
                    probed.borrow_mut().push(a.join(" "));
                    fake.probe(a)
                },
                install: &mut |a| fake.install(a, &[]),
                consent: None,
                out: &mut |l| lines.push(l.to_string()),
            },
        );
        assert!(!ok);
        assert_eq!(*probed.borrow(), ["rustup --version", "cargo --version"]);
        assert!(fake.installed.borrow().is_empty());
        assert_eq!(
            lines[2..],
            [
                "SKIP  project check 1 for rp2040b  needs confirmation in the terminal (dagon doctor)",
                "SKIP  project check 2 for rp2040b  needs confirmation in the terminal (dagon doctor)",
            ]
        );
        assert!(lines
            .iter()
            .all(|l| !l.contains("sk-ant") && !l.contains("thumbv6m")));
    }

    #[test]
    fn a_source_tree_gives_the_builtin_order() {
        use dagon_topology::defs::Source;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tree = Registry::load(&[Source::Tree(&root)]).unwrap();
        let all = all_mcus(&tree);
        let what = |r: &Registry| -> Vec<String> {
            checks(r, &all).into_iter().map(|c| c.def.what).collect()
        };
        assert_eq!(what(&tree), what(Registry::builtin()));
    }

    #[test]
    fn project_commands_run_only_after_the_user_agrees() {
        let reg = project_registry("consent", "");
        let fake = Fake::new(&[RUST[0], RUST[1]]);
        let adds = [(
            "rustup target add thumbv6m-none-eabi",
            "rustup target list --installed",
            "thumbv6m-none-eabi\n",
        )];
        let (ok, lines, asked) = run_in(&reg, &fake, &["rp2040b"], true, &adds, false);
        assert!(!ok, "a check not run is not a pass: {lines:?}");
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!(asked[0].contains("rustup target list --installed"));
        let skip = lines.iter().find(|l| l.starts_with("SKIP")).unwrap();
        assert!(
            skip.contains("target thumbv6m-none-eabi (rp2040b)"),
            "{skip}"
        );
        assert!(fake.installed.borrow().is_empty());

        let (ok, lines, asked) = run_in(&reg, &fake, &["rp2040b"], true, &adds, true);
        assert!(ok, "{lines:?}");
        assert_eq!(asked.len(), 2, "the probe, then the fix: {asked:?}");
        assert!(asked[1].contains("rustup target add thumbv6m-none-eabi"));
        assert_eq!(
            *fake.installed.borrow(),
            ["rustup target add thumbv6m-none-eabi"]
        );
    }

    #[test]
    fn a_check_several_mcus_define_alike_is_one_line_naming_them() {
        let fake = Fake::new(&[
            RUST[0],
            RUST[1],
            ("rustup target list --installed", "thumbv6m-none-eabi\n"),
        ]);
        let (ok, lines, _) = run_in(
            &project_registry("merge", ""),
            &fake,
            &["rp2040", "rp2040b"],
            false,
            &[],
            true,
        );
        assert!(ok, "{lines:?}");
        assert_eq!(
            lines,
            [
                "ok    rustup",
                "ok    cargo",
                "ok    target thumbv6m-none-eabi (rp2040, rp2040b)"
            ]
        );
    }

    const RUST: [(&str, &str); 2] = [
        ("rustup --version", "rustup 1.28"),
        ("cargo --version", "cargo 1.92"),
    ];

    #[test]
    fn checks_only_the_families_in_the_topology() {
        let fake = Fake::new(&[
            RUST[0],
            RUST[1],
            ("rustup target list --installed", "thumbv6m-none-eabi\n"),
        ]);
        let (ok, lines) = run(&fake, &["rp2040"], false, &[]);
        assert!(ok, "{lines:?}");
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines
            .iter()
            .all(|l| !l.contains("probe-rs") && !l.contains("esp")));
    }

    #[test]
    fn reports_missing_tools_with_their_install_command() {
        let fake = Fake::new(&[RUST[0], RUST[1]]);
        let (ok, lines) = run(&fake, &["stm32f401re"], false, &[]);
        assert!(!ok);
        let probe = lines.iter().find(|l| l.contains("probe-rs (")).unwrap();
        assert!(probe.starts_with("MISS") && probe.contains("cargo install probe-rs-tools"));
        let link = lines.iter().find(|l| l.contains("ST-LINK")).unwrap();
        assert!(
            link.contains("driver"),
            "no automatic fix for the driver: {link}"
        );
        assert!(
            fake.installed.borrow().is_empty(),
            "nothing installed without --fix"
        );
    }

    #[test]
    fn fix_installs_what_it_can_and_checks_again() {
        let fake = Fake::new(&[RUST[0], RUST[1]]);
        let adds = [
            (
                "rustup target add thumbv7em-none-eabihf",
                "rustup target list --installed",
                "thumbv7em-none-eabihf\n",
            ),
            (
                "cargo install probe-rs-tools --locked",
                "probe-rs --version",
                "probe-rs 0.30",
            ),
        ];
        let (ok, lines) = run(&fake, &["stm32f401re"], true, &adds);
        assert_eq!(
            *fake.installed.borrow(),
            [
                "rustup target add thumbv7em-none-eabihf",
                "cargo install probe-rs-tools --locked"
            ]
        );
        assert!(!ok, "the probe is still not connected: {lines:?}");
        assert!(lines
            .iter()
            .any(|l| l.starts_with("ok") && l.contains("probe-rs (")));

        fake.tools.borrow_mut().insert(
            "probe-rs list".into(),
            "The following debug probes were found:\n[0]: STLink V2-1\n".into(),
        );
        assert!(run(&fake, &["stm32f401re"], false, &[]).0);
    }

    #[test]
    fn esp_toolchain_needs_espup_and_says_it_is_slow() {
        let fake = Fake::new(&[
            RUST[0],
            RUST[1],
            (
                "rustup toolchain list",
                "stable-x86_64-pc-windows-msvc (default)\n",
            ),
        ]);
        let adds = [
            (
                "cargo install espup --locked",
                "espup --version",
                "espup 0.15",
            ),
            ("espup install", "rustup toolchain list", "esp\n"),
            (
                "cargo install espflash --locked",
                "espflash --version",
                "espflash 4",
            ),
        ];
        let (ok, lines) = run(&fake, &["esp32s3"], true, &adds);
        assert!(ok, "{lines:?}");
        // Only the entry its definition marks `slow` says so, before its fix.
        let slow: Vec<_> = lines
            .iter()
            .filter(|l| l.contains("takes a while"))
            .collect();
        assert_eq!(slow, ["installing the esp toolchain; this takes a while"]);
    }
}
