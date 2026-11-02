//! `dagon flash <id>`: the first write of a board. Builds the boot core (and
//! the RP2040's stage0) and the app with the developer's key, makes the
//! first-install files as `provision` / `rescue hub` do, then writes them:
//! RP2040 by copying `rescue.uf2` to the RPI-RP2 drive, F401 with probe-rs.
//! The plan is printed before anything is built, and nothing touches a
//! device until the user confirms (`--yes`) or ever with `--dry-run`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use dagon_topology::Topology;
use ed25519_compact::KeyPair;

use crate::commands::{command_lines, provision_files, FirstWrite};
use crate::config::{load_key, read_topology, Config, Opts, ProvisionInputs};
use crate::deploy::{confirm, firmware_key, YES};
use crate::nodebuild;

const DRY_RUN: &str = "--dry-run";
/// How long the RP2040's boot ROM drive is waited for.
const DRIVE_WAIT: Duration = Duration::from_secs(60);
const DRIVE_POLL: Duration = Duration::from_millis(500);
/// File the RP2040 boot ROM drive always holds, naming the board.
const UF2_INFO: &str = "INFO_UF2.TXT";
const UF2_BOARD: &str = "RPI-RP2";
/// Version of the first install when dagon.toml gives none.
const FIRST_VERSION: &str = "1.0.0";

/// A DAGON firmware part built from the DAGON checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Part {
    pub name: &'static str,
    /// Cargo package directory, relative to the DAGON checkout.
    pub package: &'static str,
}

/// The parts each MCU family's first install needs besides the app. The
/// one place that knows where they live: the board packages move them.
pub(crate) fn parts(mcu: &str) -> Option<&'static [Part]> {
    const RP2040: &[Part] = &[
        Part {
            name: "stage0",
            package: "targets/mcu/rp2040/stage0",
        },
        Part {
            name: "bootcore",
            package: "targets/mcu/rp2040/bootcore",
        },
    ];
    const F401: &[Part] = &[Part {
        name: "bootcore",
        package: "targets/mcu/stm32f401re/bootcore",
    }];
    match mcu {
        "rp2040" => Some(RP2040),
        "stm32f401re" => Some(F401),
        _ => None,
    }
}

/// The DAGON checkout the parts are built from: `DAGON_HOME`, else the one
/// this CLI was built from.
pub(crate) fn dagon_home() -> PathBuf {
    let home = std::env::var_os("DAGON_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    std::fs::canonicalize(&home).map_or(home, crate::config::plain_path)
}

/// What `flash` will do, one line each, printed before it starts.
pub(crate) fn plan(
    cfg: &Config,
    t: &Topology,
    node: &str,
    out: &Path,
) -> Result<Vec<String>, String> {
    let n = t
        .node(node)
        .ok_or_else(|| format!("no node '{node}' in the topology"))?;
    let parts = parts(&n.mcu).ok_or_else(|| {
        format!(
            "flash supports rp2040 and stm32f401re nodes, not {} ('{node}')",
            n.mcu
        )
    })?;
    let mut lines = vec![format!("flash {node} ({}):", n.mcu)];
    if cfg.provision.contains_key(node) {
        lines.push(format!(
            "  ignore   [provision.{node}]: flash always builds with the signing key's public half"
        ));
    }
    let home = dagon_home();
    for part in parts {
        lines.push(format!(
            "  build    {:<8} {} (tag {})",
            part.name,
            home.join(part.package).display(),
            n.tag
        ));
    }
    let b = cfg
        .build
        .get(node)
        .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
    lines.push(format!("  build    app      {}", b.package.display()));
    let write = match n.mcu.as_str() {
        "rp2040" => vec![
            format!("  make     {}", out.join("rescue.uf2").display()),
            format!(
                "  write    copy it to the {UF2_BOARD} drive (hold BOOT while plugging in USB; \
                 waits {} s)",
                DRIVE_WAIT.as_secs()
            ),
        ],
        _ => vec![
            format!("  make     {}", out.join("flash.bin").display()),
            "  write    probe-rs erase, download, reset through the ST-LINK".into(),
        ],
    };
    lines.extend(write);
    Ok(lines)
}

/// The first-install inputs, all built now with the signing key's public
/// half embedded. Prebuilt `[provision.<id>]` files are never used: a boot
/// core with another key would refuse every image signed later.
fn inputs(
    cfg: &Config,
    t: &Topology,
    node: &str,
    key: &KeyPair,
) -> Result<ProvisionInputs, String> {
    let n = t.node(node).ok_or("checked in plan")?;
    let fw_key = firmware_key(cfg, key)?;
    let hex = dagon_sign::to_hex(&fw_key);
    let cancel = AtomicBool::new(false);
    let mut log = |l: &str| println!("{l}");
    let mut built = Vec::new();
    for part in parts(&n.mcu).ok_or("checked in plan")? {
        let dir = dagon_home().join(part.package);
        let bin = nodebuild::package_name(&dir)?;
        let target = cfg.base.join("out/flash/target").join(part.name);
        println!("{node}: building {} in {}", part.name, dir.display());
        let exe = nodebuild::cargo_build(
            &dir,
            &bin,
            &[],
            Some(&target),
            nodebuild::FirmwareEnv {
                pubkey_hex: Some(&hex),
                // The boot core accepts only images carrying this tag.
                target_tag: Some(&n.tag),
            },
            &mut log,
            &cancel,
        )?;
        built.push((part.name, exe));
    }
    nodebuild::build_nodes_keyed(cfg, &[node.to_string()], Some(fw_key), &mut log, &cancel)?;
    let take = |name: &str| built.iter().find(|p| p.0 == name).map(|p| p.1.clone());
    let version = cfg.build.get(node).and_then(|b| b.version.clone());
    Ok(ProvisionInputs {
        stage0: take("stage0"),
        bootcore: take("bootcore").ok_or("no boot core part")?,
        app: nodebuild::elf_path(cfg, node, 'a'),
        version: Some(version.unwrap_or_else(|| FIRST_VERSION.into())),
        bootcore_version: None,
        release_seq: None,
        proto_version: None,
    })
}

/// Builds the parts and the app and writes the first-install files into
/// `out`.
pub(crate) fn prepare(
    cfg: &Config,
    t: &Topology,
    node: &str,
    key: &KeyPair,
    out: &Path,
) -> Result<FirstWrite, String> {
    let i = inputs(cfg, t, node, key)?;
    provision_files(t, node, &i, out, key)
}

/// The RP2040 boot ROM drive among `roots` (its INFO_UF2.TXT names
/// RPI-RP2): `Ok(None)` while there is none, an error when there are
/// several, since the file must reach exactly the board being flashed.
pub(crate) fn find_uf2_drive(roots: &[PathBuf]) -> Result<Option<PathBuf>, String> {
    let found: Vec<&PathBuf> = roots
        .iter()
        .filter(|r| std::fs::read_to_string(r.join(UF2_INFO)).is_ok_and(|s| s.contains(UF2_BOARD)))
        .collect();
    match found[..] {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        _ => Err(format!(
            "{} {UF2_BOARD} drives are connected ({}); unplug every board but the one to \
             flash, then run flash again. Nothing was copied.",
            found.len(),
            found
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Where a removable drive shows up on this system.
fn drive_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        return (b'D'..=b'Z')
            .map(|c| PathBuf::from(format!("{}:\\", c as char)))
            .collect();
    }
    let user = std::env::var("USER").unwrap_or_default();
    let parents = [
        PathBuf::from("/Volumes"),
        Path::new("/media").join(&user),
        Path::new("/run/media").join(&user),
        PathBuf::from("/media"),
        PathBuf::from("/mnt"),
    ];
    mounts_under(&parents)
}

/// Every directory directly under `parents`: a second board mounts under
/// another name ("RPI-RP2 1"), so names are not guessed.
fn mounts_under(parents: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = parents
        .iter()
        .filter_map(|p| std::fs::read_dir(p).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn copy_uf2(file: &Path) -> Result<(), String> {
    println!("waiting for the {UF2_BOARD} drive: hold BOOT while plugging the hub's USB in");
    let until = Instant::now() + DRIVE_WAIT;
    let drive = loop {
        if let Some(d) = find_uf2_drive(&drive_roots())? {
            break d;
        }
        if Instant::now() >= until {
            return Err(format!(
                "no {UF2_BOARD} drive within {} s; hold BOOT while plugging in USB, then copy {} \
                 to the drive by hand",
                DRIVE_WAIT.as_secs(),
                file.display()
            ));
        }
        std::thread::sleep(DRIVE_POLL);
    };
    let dest = drive.join("rescue.uf2");
    std::fs::copy(file, &dest).map_err(|e| format!("cannot copy to {}: {e}", dest.display()))?;
    println!("copied to {}; the hub restarts on its own", dest.display());
    Ok(())
}

fn run_probe_rs(commands: &[Vec<String>]) -> Result<(), String> {
    for args in commands {
        println!("+ probe-rs {}", args.join(" "));
        let ok = Command::new("probe-rs")
            .args(args)
            .status()
            .map_err(|e| format!("cannot run probe-rs ({e}); see `dagon doctor`"))?
            .success();
        if !ok {
            return Err(format!("probe-rs {} failed", args[0]));
        }
    }
    Ok(())
}

/// `dagon flash <id> [--dry-run] [--yes] [--out <dir>] [--topology <yaml>]
/// [--key <file>] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let dry = args.contains(&DRY_RUN);
    let yes = args.contains(&YES);
    let args: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != DRY_RUN && *a != YES)
        .collect();
    let o = Opts::parse(&args, &["out", "topology", "key", "config"])?;
    let [node] = o.pos[..] else {
        return Err("flash needs <id>".into());
    };
    let cfg = Config::load(o.get("config"))?;
    let t = read_topology(&cfg.topology_path(o.get("topology"))?)?;
    let out = o
        .get("out")
        .map_or_else(|| cfg.provision_dir().join(node), PathBuf::from);
    let steps = plan(&cfg, &t, node, &out)?.join("\n");
    println!("{steps}");
    let key = load_key(o.get("key"))?;
    let write = prepare(&cfg, &t, node, &key, &out)?;
    if dry {
        match &write {
            FirstWrite::ProbeRs { commands, .. } => {
                print!(
                    "dry run; would run:\n{}",
                    command_lines("probe-rs", commands)
                )
            }
            FirstWrite::Uf2 { file, .. } => println!(
                "dry run; would copy {} to the {UF2_BOARD} drive",
                file.display()
            ),
        }
        return Ok(());
    }
    if !yes && !confirm("", &mut std::io::stdin().lock()) {
        return Err("nothing written to the board".into());
    }
    match &write {
        FirstWrite::ProbeRs { commands, .. } => run_probe_rs(commands),
        FirstWrite::Uf2 { file, .. } => copy_uf2(file),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BuildInputs;
    use ed25519_compact::Seed;

    const YAML: &str = include_str!("../../../topology/desk-demo.yaml");

    /// A work area with `[build.<id>]` for hub and seg, and the topology.
    fn area(name: &str) -> (Config, Topology) {
        let base = std::env::temp_dir().join(format!("dagon-flash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let mut cfg = Config {
            base,
            ..Config::default()
        };
        for (id, package) in [("hub", "targets/rp2040-hub"), ("seg", "samples/f401-app")] {
            cfg.build.insert(
                id.into(),
                BuildInputs {
                    package: PathBuf::from(package),
                    bin: None,
                    features: vec![],
                    slot_b_feature: Some("slot-b".into()),
                    version: None,
                },
            );
        }
        (cfg, dagon_topology::parse_str(YAML).unwrap())
    }

    /// What the builds hand to the file step, as raw stand-in payloads.
    fn built(cfg: &Config, with_stage0: bool) -> ProvisionInputs {
        let file = |n: &str, len: usize| {
            let p = cfg.base.join(n);
            std::fs::write(&p, (0..len).map(|i| i as u8).collect::<Vec<_>>()).unwrap();
            p
        };
        ProvisionInputs {
            stage0: with_stage0.then(|| file("stage0.bin", 2000)),
            bootcore: file("boot.bin", 4000),
            app: file("app.bin", 8000),
            version: Some(FIRST_VERSION.into()),
            bootcore_version: None,
            release_seq: None,
            proto_version: None,
        }
    }

    fn key() -> KeyPair {
        KeyPair::from_seed(Seed::new([9; 32]))
    }

    #[test]
    fn rp2040_dry_run_makes_the_uf2_and_plans_the_drive_copy() {
        let (cfg, t) = area("hub");
        let out = cfg.base.join("out/hub");
        let plan = plan(&cfg, &t, "hub", &out).unwrap().join("\n");
        assert!(
            plan.contains("rescue.uf2") && plan.contains("RPI-RP2"),
            "{plan}"
        );
        assert!(
            plan.contains("stage0") && plan.contains("targets/rp2040-hub"),
            "{plan}"
        );
        assert!(plan.contains("(tag hub-rp2040-lcd)"), "{plan}");
        match provision_files(&t, "hub", &built(&cfg, true), &out, &key()).unwrap() {
            FirstWrite::Uf2 { file, blocks } => {
                assert_eq!(file, out.join("rescue.uf2"));
                let len = std::fs::metadata(&file).unwrap().len() as usize;
                assert_eq!(len, blocks * 512);
                assert!(blocks > 0);
            }
            w => panic!("{w:?}"),
        }
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn f401_dry_run_makes_the_image_and_the_probe_rs_commands() {
        let (cfg, t) = area("seg");
        let out = cfg.base.join("out/seg");
        let plan = plan(&cfg, &t, "seg", &out).unwrap().join("\n");
        assert!(
            plan.contains("flash.bin") && plan.contains("probe-rs"),
            "{plan}"
        );
        assert!(
            plan.contains("stm32f401re") && plan.contains("samples/f401-app"),
            "{plan}"
        );
        assert!(plan.contains("(tag seg-f401-7seg)"), "{plan}");
        match provision_files(&t, "seg", &built(&cfg, false), &out, &key()).unwrap() {
            FirstWrite::ProbeRs {
                bin,
                bytes,
                commands,
            } => {
                assert_eq!(std::fs::metadata(&bin).unwrap().len() as usize, bytes);
                let verbs: Vec<&str> = commands.iter().map(|c| c[0].as_str()).collect();
                assert_eq!(verbs, ["erase", "download", "reset"]);
                assert!(commands.iter().all(|c| c.contains(&"STM32F401RETx".into())));
                assert_eq!(commands[1].last().unwrap(), &bin.display().to_string());
                assert!(out.join("commands.txt").exists());
            }
            w => panic!("{w:?}"),
        }
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn prebuilt_provision_files_are_never_used() {
        let (mut cfg, t) = area("prov");
        let prebuilt = built(&cfg, false);
        cfg.provision.insert("seg".into(), prebuilt.clone());
        let plan = plan(&cfg, &t, "seg", &cfg.base.join("out"))
            .unwrap()
            .join("\n");
        assert!(plan.contains("ignore   [provision.seg]"), "{plan}");
        assert!(plan.contains("build    bootcore"), "{plan}");
        assert!(
            !plan.contains(&prebuilt.app.display().to_string()),
            "{plan}"
        );
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn other_families_and_unknown_nodes_are_refused_before_anything_runs() {
        let (mut cfg, t) = area("esp");
        let out = cfg.base.join("out");
        assert!(plan(&cfg, &t, "watch", &out)
            .unwrap_err()
            .contains("esp32s3"));
        assert!(plan(&cfg, &t, "nope", &out).is_err());
        cfg.build.clear();
        assert!(plan(&cfg, &t, "seg", &out)
            .unwrap_err()
            .contains("[build.seg]"));
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn the_parts_are_in_this_checkout() {
        for mcu in ["rp2040", "stm32f401re"] {
            for p in parts(mcu).unwrap() {
                let toml = dagon_home().join(p.package).join("Cargo.toml");
                assert!(toml.is_file(), "{} is not in the checkout", toml.display());
            }
        }
        let names: Vec<&str> = parts("rp2040").unwrap().iter().map(|p| p.name).collect();
        assert_eq!(names, ["stage0", "bootcore"]);
    }

    #[test]
    fn copies_only_to_a_single_rp2040_boot_drive() {
        let base = std::env::temp_dir().join(format!("dagon-uf2-{}", std::process::id()));
        let (other, rp, rp2) = (base.join("usb"), base.join("rp"), base.join("rp2"));
        for d in [&other, &rp, &rp2] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(other.join(UF2_INFO), "Board-ID: SOMETHING-ELSE").unwrap();
        let info = "UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n";
        std::fs::write(rp.join(UF2_INFO), info).unwrap();
        std::fs::write(rp2.join(UF2_INFO), info).unwrap();
        let none = base.join("missing");
        assert_eq!(
            find_uf2_drive(&[none.clone(), other.clone(), rp.clone()]),
            Ok(Some(rp.clone()))
        );
        assert_eq!(find_uf2_drive(&[none.clone(), other.clone()]), Ok(None));
        let two = find_uf2_drive(&[rp, other, rp2]).unwrap_err();
        assert!(
            two.contains("2 RPI-RP2 drives") && two.contains("unplug"),
            "{two}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_second_board_under_another_mount_name_is_seen() {
        let base = std::env::temp_dir().join(format!("dagon-mnt-{}", std::process::id()));
        let (volumes, media) = (base.join("Volumes"), base.join("media/user"));
        let info = "Model: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n";
        for d in [volumes.join("RPI-RP2"), volumes.join("RPI-RP2 1")] {
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(UF2_INFO), info).unwrap();
        }
        std::fs::create_dir_all(media.join("USB STICK")).unwrap();
        std::fs::write(volumes.join("not-a-dir"), "x").unwrap();
        let parents = [volumes.clone(), media.clone(), base.join("missing")];
        let roots = mounts_under(&parents);
        assert_eq!(roots.len(), 3, "{roots:?}");
        let err = find_uf2_drive(&roots).unwrap_err();
        assert!(err.contains("2 RPI-RP2 drives"), "{err}");

        std::fs::remove_dir_all(volumes.join("RPI-RP2")).unwrap();
        let one = find_uf2_drive(&mounts_under(&parents)).unwrap();
        assert_eq!(one, Some(volumes.join("RPI-RP2 1")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
