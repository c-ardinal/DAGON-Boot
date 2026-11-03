//! `dagon doctor [--fix]`: the tools each MCU of the topology needs (every
//! MCU defined without a topology). Rust itself is checked here; the rest
//! are the MCU definitions' `[[doctor]]` entries. `--fix` runs their install
//! commands (no administrator rights: `rustup target add`, `cargo install`,
//! ...); drivers and the like are left to the developer. Commands from the
//! project's own definitions run only after the user agrees to each one.

use std::process::Command;

use dagon_topology::defs::{DoctorCheck, Expect, Origin, Registry};

use crate::config::{read_topology, Config, Opts};

const FIX: &str = "--fix";

/// The MCU ids `dagon serve`'s doctor job checks without a topology.
// ponytail: serve/devjobs.rs still names this list; once it passes the
// project's registry ([`all_mcus`]) this goes (a test pins it to builtin).
pub(crate) const FAMILIES: [&str; 3] = ["rp2040", "stm32f401re", "esp32s3"];

const RUST_HINT: &str = "install Rust from https://rustup.rs";

/// A check and the MCUs that need it (none: every MCU).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    def: DoctorCheck,
    for_mcu: Vec<String>,
    /// Defined by the project: its commands run only once the user agrees.
    ask: bool,
}

fn rust(what: &str) -> Check {
    Check {
        def: DoctorCheck {
            what: what.into(),
            probe: vec![what.into(), "--version".into()],
            expect: Expect::Runs,
            fix: None,
            hint: RUST_HINT.into(),
        },
        for_mcu: Vec::new(),
        ask: false,
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
        for d in &m.doctor {
            match out.iter_mut().find(|c| c.def == *d) {
                Some(c) => {
                    c.for_mcu.push(m.id.clone());
                    c.ask |= ask;
                }
                None => out.push(Check {
                    def: d.clone(),
                    for_mcu: vec![m.id.clone()],
                    ask,
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

/// Whether `cmd` of check `c` may run: always for builtin and DAGON source
/// definitions, after the user agrees for the project's.
fn allowed(c: &Check, cmd: &[String], consent: &mut Consent<'_>) -> bool {
    !c.ask
        || consent(&format!(
            "the project's definition of {} ({}) runs: {}\n",
            c.def.what,
            c.for_mcu.join(", "),
            cmd.join(" ")
        ))
}

/// Everything [`doctor`] does outside: run probes and installs, ask, print.
pub(crate) struct Io<'a> {
    pub probe: &'a mut Runner<'a>,
    pub install: &'a mut Runner<'a>,
    pub consent: &'a mut Consent<'a>,
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
    if !allowed(c, &d.probe, io.consent) {
        (io.out)(&format!(
            "SKIP  {}{used_by}  not run; to check it yourself: {}",
            d.what,
            d.probe.join(" ")
        ));
        return false;
    }
    let mut good = passes(d, io.probe);
    let mut fixed = false;
    if !good && fix {
        if let Some(cmd) = d.fix.as_deref().filter(|cmd| allowed(c, cmd, io.consent)) {
            // ponytail: the one slow install is named by its tool; a
            // definition field would carry it once a second one appears.
            if cmd[0] == "espup" {
                (io.out)("installing the esp toolchain; this takes a while");
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

/// The builtin MCUs' checks for `mcus` (no installs), one line each to
/// `out`; nothing asks, so no project-defined command runs.
pub(crate) fn check(mcus: &[String], out: &mut dyn FnMut(&str)) -> bool {
    let mut io = Io {
        probe: &mut probe_cmd,
        install: &mut |_| None,
        consent: &mut |_| false,
        out,
    };
    doctor(Registry::builtin(), mcus, false, &mut io)
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

/// `dagon doctor [--fix] [--topology <yaml>] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let fix = args.contains(&FIX);
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != FIX).collect();
    let o = Opts::parse(&args, &["topology", "config"])?;
    let cfg = Config::load(o.get("config"))?;
    let reg = crate::defs::registry(&cfg)?;
    let mcus: Vec<String> = match o.get("topology").map(Into::into).or(cfg.topology.clone()) {
        Some(p) => {
            let t = read_topology(&p)?;
            println!("checking for {}", p.display());
            t.nodes.iter().map(|n| n.mcu.clone()).collect()
        }
        None => {
            println!("no topology: checking every family");
            all_mcus(&reg)
        }
    };
    let mut io = Io {
        probe: &mut probe_cmd,
        install: &mut install_cmd,
        consent: &mut |q| crate::deploy::confirm(q, &mut std::io::stdin().lock()),
        out: &mut |l| println!("{l}"),
    };
    if doctor(&reg, &mcus, fix, &mut io) {
        Ok(())
    } else {
        Err("some prerequisites are missing; see above".into())
    }
}

#[cfg(test)]
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
                consent: &mut |q| {
                    asked.push(q.to_string());
                    agree
                },
                out: &mut |l| lines.push(l.to_string()),
            },
        );
        (ok, lines, asked)
    }

    /// The builtin definitions as the project's (JSON is never trusted),
    /// plus `rp2040b`: an RP2040 copy under another id.
    fn project_registry() -> Registry {
        let mut v: serde_json::Value =
            serde_json::from_str(&Registry::builtin().to_json()).unwrap();
        let toml = include_str!("../../../targets/mcu/rp2040/dagon-mcu.toml")
            .replace("id = \"rp2040\"", "id = \"rp2040b\"")
            .replace("wire_id = 0x0001", "wire_id = 0x00F1")
            .replace("name = \"rp2040-4m\"", "name = \"rp2040b-4m\"");
        let mcus = v["mcus"].as_array_mut().unwrap();
        mcus.push(serde_json::json!({ "label": "project", "toml": toml }));
        Registry::from_json(&v.to_string()).unwrap()
    }

    #[test]
    fn every_builtin_mcu_is_checked_without_a_topology() {
        assert_eq!(all_mcus(Registry::builtin()), FAMILIES.map(String::from));
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
        let reg = project_registry();
        let fake = Fake::new(&[RUST[0], RUST[1]]);
        let adds = [(
            "rustup target add thumbv6m-none-eabi",
            "rustup target list --installed",
            "thumbv6m-none-eabi\n",
        )];
        let (ok, lines, asked) = run_in(&reg, &fake, &["rp2040"], true, &adds, false);
        assert!(!ok, "a check not run is not a pass: {lines:?}");
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!(asked[0].contains("rustup target list --installed"));
        let skip = lines.iter().find(|l| l.starts_with("SKIP")).unwrap();
        assert!(
            skip.contains("target thumbv6m-none-eabi (rp2040)"),
            "{skip}"
        );
        assert!(fake.installed.borrow().is_empty());

        let (ok, lines, asked) = run_in(&reg, &fake, &["rp2040"], true, &adds, true);
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
            &project_registry(),
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
        assert!(lines.iter().any(|l| l.contains("takes a while")));
    }
}
