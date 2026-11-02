//! `dagon doctor [--fix]`: the tools each MCU family of the topology needs
//! (all families without a topology). `--fix` runs the installs that need
//! no administrator rights (`rustup target add`, `cargo install`, `espup
//! install`); drivers and the like are left to the developer.

use std::process::Command;

use crate::config::{read_topology, Config, Opts};

const FIX: &str = "--fix";
const FAMILIES: [&str; 3] = ["rp2040", "stm32f401re", "esp32s3"];

/// What a check finds in a command's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// The command runs and succeeds.
    Runs,
    /// Its output has this line (trimmed), e.g. an installed target.
    Line(&'static str),
    /// Its output has a line starting with this.
    Prefix(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Check {
    what: &'static str,
    /// Families that need it; empty: every family.
    for_mcu: &'static [&'static str],
    probe: &'static [&'static str],
    expect: Expect,
    /// Install command; `None`: the developer installs it.
    fix: Option<&'static [&'static str]>,
    /// Shown when it is missing.
    hint: &'static str,
}

const CHECKS: &[Check] = &[
    Check {
        what: "rustup",
        for_mcu: &[],
        probe: &["rustup", "--version"],
        expect: Expect::Runs,
        fix: None,
        hint: "install Rust from https://rustup.rs",
    },
    Check {
        what: "cargo",
        for_mcu: &[],
        probe: &["cargo", "--version"],
        expect: Expect::Runs,
        fix: None,
        hint: "install Rust from https://rustup.rs",
    },
    Check {
        what: "target thumbv6m-none-eabi",
        for_mcu: &["rp2040"],
        probe: &["rustup", "target", "list", "--installed"],
        expect: Expect::Line("thumbv6m-none-eabi"),
        fix: Some(&["rustup", "target", "add", "thumbv6m-none-eabi"]),
        hint: "",
    },
    Check {
        what: "target thumbv7em-none-eabihf",
        for_mcu: &["stm32f401re"],
        probe: &["rustup", "target", "list", "--installed"],
        expect: Expect::Line("thumbv7em-none-eabihf"),
        fix: Some(&["rustup", "target", "add", "thumbv7em-none-eabihf"]),
        hint: "",
    },
    Check {
        what: "probe-rs",
        for_mcu: &["stm32f401re"],
        probe: &["probe-rs", "--version"],
        expect: Expect::Runs,
        fix: Some(&["cargo", "install", "probe-rs-tools", "--locked"]),
        hint: "",
    },
    Check {
        what: "ST-LINK probe connected",
        for_mcu: &["stm32f401re"],
        probe: &["probe-rs", "list"],
        expect: Expect::Prefix("["),
        fix: None,
        hint: "plug in the ST-LINK; on Windows install the ST-LINK USB driver from st.com",
    },
    Check {
        what: "espup",
        for_mcu: &["esp32s3"],
        probe: &["espup", "--version"],
        expect: Expect::Runs,
        fix: Some(&["cargo", "install", "espup", "--locked"]),
        hint: "",
    },
    Check {
        what: "esp toolchain",
        for_mcu: &["esp32s3"],
        probe: &["rustup", "toolchain", "list"],
        expect: Expect::Prefix("esp"),
        fix: Some(&["espup", "install"]),
        hint: "",
    },
    Check {
        what: "espflash",
        for_mcu: &["esp32s3"],
        probe: &["espflash", "--version"],
        expect: Expect::Runs,
        fix: Some(&["cargo", "install", "espflash", "--locked"]),
        hint: "",
    },
];

/// Runs a program: its stdout when it ran and succeeded.
pub(crate) type Runner<'a> = dyn FnMut(&[&str]) -> Option<String> + 'a;

fn passes(c: &Check, run: &mut Runner<'_>) -> bool {
    let Some(out) = run(c.probe) else {
        return false;
    };
    let mut lines = out.lines().map(str::trim);
    match c.expect {
        Expect::Runs => true,
        Expect::Line(l) => lines.any(|x| x == l),
        Expect::Prefix(p) => lines.any(|x| x.starts_with(p)),
    }
}

/// One line per check for `mcus`; `false` when something is still missing.
/// With `fix`, missing tools that have an install command are installed
/// (through `install`) and checked again.
pub(crate) fn doctor(
    mcus: &[String],
    fix: bool,
    probe: &mut Runner<'_>,
    install: &mut Runner<'_>,
    out: &mut dyn FnMut(&str),
) -> bool {
    let mut ok = true;
    let wanted = CHECKS
        .iter()
        .filter(|c| c.for_mcu.is_empty() || c.for_mcu.iter().any(|m| mcus.iter().any(|x| x == m)));
    for c in wanted {
        let mut good = passes(c, probe);
        if !good && fix {
            if let Some(cmd) = c.fix {
                if cmd[0] == "espup" {
                    out("installing the esp toolchain; this takes a while");
                }
                out(&format!("fix   {}", cmd.join(" ")));
                good = install(cmd).is_some() && passes(c, probe);
            }
        }
        let used_by = if c.for_mcu.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.for_mcu.join(", "))
        };
        let advice = match (good, c.fix, c.hint) {
            (true, ..) => String::new(),
            (false, Some(cmd), _) if !fix => {
                format!("  fix: {} (or dagon doctor --fix)", cmd.join(" "))
            }
            (false, Some(_), _) => "  the install did not help; see its output".into(),
            (false, None, hint) => format!("  {hint}"),
        };
        out(&format!(
            "{}  {}{used_by}{advice}",
            if good { "ok  " } else { "MISS" },
            c.what
        ));
        ok &= good;
    }
    ok
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
    let mcus: Vec<String> = match o.get("topology").map(Into::into).or(cfg.topology.clone()) {
        Some(p) => {
            let t = read_topology(&p)?;
            println!("checking for {}", p.display());
            t.nodes.iter().map(|n| n.mcu.clone()).collect()
        }
        None => {
            println!("no topology: checking every family");
            FAMILIES.map(String::from).to_vec()
        }
    };
    let ok = doctor(&mcus, fix, &mut probe_cmd, &mut install_cmd, &mut |l| {
        println!("{l}")
    });
    if ok {
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
        let mcus: Vec<String> = mcus.iter().map(|s| s.to_string()).collect();
        let mut lines = Vec::new();
        let ok = doctor(
            &mcus,
            fix,
            &mut |a| fake.probe(a),
            &mut |a| fake.install(a, adds),
            &mut |l| lines.push(l.to_string()),
        );
        (ok, lines)
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
