//! `dagon flash <id>`: the first write of a board. Builds the boot core (and
//! the RP2040's stage0) and the app with the developer's key, makes the
//! first-install files as `provision` / `rescue hub` do, then writes them:
//! RP2040 by copying `rescue.uf2` to the RPI-RP2 drive, F401 with probe-rs,
//! ESP32-S3 with espflash (the bundled 2nd stage, the partition table, the
//! factory agent, slot A and otadata) to the one ESP32-S3 on USB.
//! The plan is printed before anything is built, and nothing touches a
//! device until the user confirms (`--yes`) or ever with `--dry-run`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub name: &'static str,
    /// Cargo package directory, relative to the DAGON checkout.
    pub package: String,
    /// Binary target; `None`: the package's own name.
    pub bin: Option<&'static str>,
}

/// The parts each MCU family's first install needs besides the app, for a
/// node on `board`. The one place that knows where they live.
pub(crate) fn parts(mcu: &str, board: Option<&str>) -> Option<Vec<Part>> {
    let part = |name, package: &str, bin| Part {
        name,
        package: package.into(),
        bin,
    };
    match mcu {
        "rp2040" => Some(vec![
            part("stage0", "targets/mcu/rp2040/stage0", None),
            part("bootcore", "targets/mcu/rp2040/bootcore", None),
        ]),
        "stm32f401re" => Some(vec![part(
            "bootcore",
            "targets/mcu/stm32f401re/bootcore",
            None,
        )]),
        // The recovery agent for the factory partition ships with the
        // board package: a node without one has none to write.
        "esp32s3" => board.map(|b| {
            vec![part(
                "factory",
                &format!("targets/boards/{b}"),
                Some("factory"),
            )]
        }),
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
    let parts = parts(&n.mcu, n.board.as_deref()).ok_or_else(|| match n.mcu.as_str() {
        "esp32s3" => {
            format!("'{node}' names no board: its factory agent comes from the board package")
        }
        mcu => {
            format!("flash supports rp2040, stm32f401re and esp32s3 nodes, not {mcu} ('{node}')")
        }
    })?;
    let mut lines = vec![format!("flash {node} ({}):", n.mcu)];
    if cfg.provision.contains_key(node) {
        lines.push(format!(
            "  ignore   [provision.{node}]: flash always builds with the signing key's public half"
        ));
    }
    let home = dagon_home();
    for part in &parts {
        lines.push(format!(
            "  build    {:<8} {} (tag {})",
            part.name,
            home.join(&part.package).display(),
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
        "esp32s3" => {
            let mut l = vec![format!(
                "  check    {} against SHA256SUMS (the unmodified ESP-IDF 2nd stage)",
                home.join(crate::esp::VENDOR_DIR)
                    .join("bootloader.bin")
                    .display()
            )];
            l.push(format!("  make     in {}:", out.display()));
            l.extend(
                esp_files_plan()
                    .into_iter()
                    .map(|f| format!("             {f}")),
            );
            l.push(format!(
                "  write    espflash over USB-C to the one ESP32-S3 connected (USB {:04x}:{:04x})",
                crate::esp::USB_JTAG.0,
                crate::esp::USB_JTAG.1
            ));
            l
        }
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
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<ProvisionInputs, String> {
    let n = t.node(node).ok_or("checked in plan")?;
    let fw_key = firmware_key(cfg, key)?;
    let hex = dagon_sign::to_hex(&fw_key);
    let mut built = Vec::new();
    for part in parts(&n.mcu, n.board.as_deref()).ok_or("checked in plan")? {
        let dir = dagon_home().join(&part.package);
        let bin = match part.bin {
            Some(b) => b.to_string(),
            None => nodebuild::package_name(&dir)?,
        };
        let target = cfg.base.join("out/flash/target").join(part.name);
        log(&format!(
            "{node}: building {} in {}",
            part.name,
            dir.display()
        ));
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
            log,
            cancel,
        )?;
        built.push((part.name, exe));
    }
    nodebuild::build_nodes_keyed(cfg, &[node.to_string()], Some(fw_key), log, cancel)?;
    let take = |name: &str| built.iter().find(|p| p.0 == name).map(|p| p.1.clone());
    let version = cfg.build.get(node).and_then(|b| b.version.clone());
    Ok(ProvisionInputs {
        stage0: take("stage0"),
        // ESP32-S3: the factory agent stands where the boot core is.
        bootcore: take("bootcore")
            .or_else(|| take("factory"))
            .ok_or("no boot core part")?,
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
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<FirstWrite, String> {
    let i = inputs(cfg, t, node, key, log, cancel)?;
    match t.node(node).map(|n| n.mcu.as_str()) {
        Some("esp32s3") => {
            let app = crate::esp::save_image(&i.app, &out.join("app.bin"))?;
            esp_files(t, node, &i, &app, &dagon_home(), out, key)
        }
        _ => provision_files(t, node, &i, out, key),
    }
}

/// The ESP32-S3 first-install files, as the plan names them.
fn esp_files_plan() -> Vec<String> {
    [
        "bootloader.bin  at 0x0       (2nd stage)",
        "partitions.csv  at 0x8000    (partition table)",
        "factory.elf     in factory   (recovery agent)",
        "body.bin        in ota_0     (app, slot A)",
        "header.bin      in dagon     (its signed header)",
        "otadata.bin     in otadata   (ota_0 selected, confirmed)",
    ]
    .map(String::from)
    .to_vec()
}

/// Writes the ESP32-S3 first-install files into `out` and returns the
/// espflash commands (without the port, chosen when writing): the checked
/// 2nd stage, the partition table and the factory agent, then slot A's body
/// and header and the otadata entry that selects it.
fn esp_files(
    t: &Topology,
    node: &str,
    i: &ProvisionInputs,
    app_image: &[u8],
    home: &Path,
    out: &Path,
    key: &KeyPair,
) -> Result<FirstWrite, String> {
    use crate::esp;
    use dagon_format::header::{image_kind, slot_binding, HEADER_SIZE};
    let (vendor, csv_path) = esp::vendor_files(home);
    let boot = esp::checked_bootloader(&vendor)?;
    let csv =
        std::fs::read_to_string(&csv_path).map_err(|e| format!("{}: {e}", csv_path.display()))?;
    let l = esp::layout(&csv)?;
    let version = crate::commands::version(i.version.as_deref().unwrap_or(FIRST_VERSION))?;
    let opts = crate::commands::SignOpts {
        version,
        release_seq: i.release_seq.unwrap_or(1),
        min_bootcore: dagon_format::Version::default(),
        proto: i.proto_version.unwrap_or(1),
        bits: None,
    };
    let img = crate::commands::image_for_node(
        t,
        node,
        image_kind::APP,
        slot_binding::A,
        app_image,
        Some(&opts),
        key,
    )?;
    let (header, body) = img.split_at(HEADER_SIZE);
    let file = |name: &str, bytes: &[u8]| -> Result<PathBuf, String> {
        let p = out.join(name);
        crate::config::write_file(&p, bytes)?;
        Ok(p)
    };
    let bootloader = file("bootloader.bin", &boot)?;
    let table = file("partitions.csv", csv.as_bytes())?;
    let factory = file("factory.elf", &crate::config::read_file(&i.bootcore)?)?;
    file("app-a.img", &img)?;
    let body = file("body.bin", body)?;
    let header = file("header.bin", header)?;
    let otadata = file("otadata.bin", &esp::otadata_slot_a())?;

    let s = |v: &str| v.to_string();
    let p = |p: &PathBuf| p.display().to_string();
    let common = [s("--chip"), s(esp::CHIP)];
    let mut first = vec![s("flash")];
    first.extend(common.clone());
    first.extend([
        s("--flash-size"),
        s(esp::FLASH_SIZE),
        s("--partition-table-offset"),
        format!("{:#x}", esp::TABLE_AT),
        s("--bootloader"),
        p(&bootloader),
        s("--partition-table"),
        p(&table),
        s("--erase-parts"),
        s("otadata,dagon"),
        s("--target-app-partition"),
        s("factory"),
        p(&factory),
    ]);
    let write_bin = |at: u32, f: &PathBuf| {
        let mut c = vec![s("write-bin")];
        c.extend(common.clone());
        c.extend([format!("{at:#x}"), p(f)]);
        c
    };
    let commands = vec![
        first,
        write_bin(l.ota_0, &body),
        write_bin(l.dagon, &header),
        write_bin(l.otadata, &otadata),
    ];
    let files = vec![
        (0, bootloader),
        (esp::TABLE_AT, table),
        (l.factory, factory),
        (l.ota_0, body),
        (l.dagon, header),
        (l.otadata, otadata),
    ];
    crate::config::write_file(
        &out.join("commands.txt"),
        command_lines("espflash", &commands).as_bytes(),
    )?;
    Ok(FirstWrite::Espflash { files, commands })
}

/// The RP2040 boot ROM drive among `roots` (its INFO_UF2.TXT names
/// RPI-RP2): `Ok(None)` while there is none, an error when there are
/// several, since the file must reach exactly the board being flashed.
pub(crate) fn find_uf2_drive(roots: &[PathBuf]) -> Result<Option<PathBuf>, String> {
    let found: Vec<&PathBuf> = roots
        .iter()
        .filter(|r| std::fs::read_to_string(r.join(UF2_INFO)).is_ok_and(|s| is_rp2040_rom(&s)))
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

/// Whether INFO_UF2.TXT is the RP2040 boot ROM's: a `UF2 Bootloader` first
/// line and the exact line `Board-ID: RPI-RP2` (not a part of another ID).
fn is_rp2040_rom(info: &str) -> bool {
    // The literal first line (lines() drops only the line break).
    let mut lines = info.lines();
    let header = lines
        .next()
        .is_some_and(|l| l.starts_with("UF2 Bootloader"));
    let board = format!("Board-ID: {UF2_BOARD}");
    header && lines.any(|l| l == board)
}

/// The MCU families `flash` can write (the page offers it for these).
pub(crate) fn flashable(mcu: &str) -> bool {
    parts(mcu, Some("any")).is_some()
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

/// Writes the first-install files to the board (after the confirmation).
pub(crate) fn write(
    w: &FirstWrite,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<(), String> {
    match w {
        FirstWrite::ProbeRs { commands, .. } => run_probe_rs(commands, log),
        FirstWrite::Uf2 { file, .. } => copy_uf2(file, log, cancel),
        FirstWrite::Espflash { commands, .. } => {
            let port = crate::esp::pick_port(&crate::esp::usb_ports()?)?;
            run_espflash(&with_port(commands, &port), log, cancel)
        }
    }
}

/// `commands` aimed at `port`, never asking which port to use.
fn with_port(commands: &[Vec<String>], port: &str) -> Vec<Vec<String>> {
    commands
        .iter()
        .map(|c| {
            let mut a = vec![c[0].clone()];
            a.extend(["--port".into(), port.into(), "--non-interactive".into()]);
            a.extend(c[1..].iter().cloned());
            a
        })
        .collect()
}

fn run_espflash(
    commands: &[Vec<String>],
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<(), String> {
    for args in commands {
        if cancel.load(Ordering::Relaxed) {
            return Err(format!(
                "cancelled before espflash {}; the rest was not written",
                args[0]
            ));
        }
        log(&format!("+ espflash {}", args.join(" ")));
        let o = Command::new("espflash")
            .args(args)
            .output()
            .map_err(|e| format!("cannot run espflash ({e}); see `dagon doctor`"))?;
        for l in String::from_utf8_lossy(&o.stdout)
            .lines()
            .chain(String::from_utf8_lossy(&o.stderr).lines())
        {
            log(l);
        }
        if !o.status.success() {
            return Err(format!("espflash {} failed", args[0]));
        }
    }
    Ok(())
}

fn copy_uf2(file: &Path, log: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<(), String> {
    log(&format!(
        "waiting for the {UF2_BOARD} drive: hold BOOT while plugging the hub's USB in"
    ));
    let until = Instant::now() + DRIVE_WAIT;
    let drive = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled; nothing was copied".into());
        }
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
    log(&format!(
        "copied to {}; the hub restarts on its own",
        dest.display()
    ));
    Ok(())
}

fn run_probe_rs(commands: &[Vec<String>], log: &mut dyn FnMut(&str)) -> Result<(), String> {
    for args in commands {
        log(&format!("+ probe-rs {}", args.join(" ")));
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
    let mut log = |l: &str| println!("{l}");
    let cancel = AtomicBool::new(false);
    let w = prepare(&cfg, &t, node, &key, &out, &mut log, &cancel)?;
    if dry {
        match &w {
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
            FirstWrite::Espflash { files, commands } => {
                println!("dry run; the flash would hold:");
                for (at, f) in files {
                    println!("  {at:#09x}  {}", f.display());
                }
                print!(
                    "and would run, on the one ESP32-S3 connected:\n{}",
                    command_lines("espflash", &with_port(commands, "<port>"))
                )
            }
        }
        return Ok(());
    }
    if !yes && !confirm("", &mut std::io::stdin().lock()) {
        return Err("nothing written to the board".into());
    }
    write(&w, &mut log, &cancel)
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
        assert!(
            parts("esp32s3", None).is_none(),
            "no board, no factory agent"
        );
        assert!(plan(&cfg, &t, "nope", &out).is_err());
        cfg.build.clear();
        assert!(plan(&cfg, &t, "seg", &out)
            .unwrap_err()
            .contains("[build.seg]"));
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn the_parts_are_in_this_checkout() {
        for mcu in ["rp2040", "stm32f401re", "esp32s3"] {
            for p in parts(mcu, Some("m5stopwatch")).unwrap() {
                let toml = dagon_home().join(p.package).join("Cargo.toml");
                assert!(toml.is_file(), "{} is not in the checkout", toml.display());
            }
        }
        let names: Vec<&str> = parts("rp2040", None)
            .unwrap()
            .iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["stage0", "bootcore"]);
    }

    #[test]
    fn esp32s3_dry_run_makes_the_full_file_set_at_the_table_offsets() {
        let (mut cfg, t) = area("watch");
        cfg.build.insert(
            "watch".into(),
            BuildInputs {
                package: PathBuf::from("nodes/watch"),
                bin: None,
                features: vec![],
                slot_b_feature: None,
                version: Some("1.0.0".into()),
            },
        );
        let out = cfg.base.join("out/watch");
        let plan = plan(&cfg, &t, "watch", &out).unwrap().join("\n");
        for want in [
            "targets/boards/m5stopwatch (tag watch-s3)",
            "SHA256SUMS",
            "bootloader.bin  at 0x0",
            "otadata.bin     in otadata",
            "USB 303a:1001",
        ] {
            assert!(plan.contains(want), "{want}: {plan}");
        }

        let i = built(&cfg, false);
        let app = vec![0xE9; 7000];
        let w = esp_files(&t, "watch", &i, &app, &dagon_home(), &out, &key()).unwrap();
        let FirstWrite::Espflash { files, commands } = w else {
            panic!("{w:?}")
        };
        let at: Vec<u32> = files.iter().map(|f| f.0).collect();
        assert_eq!(at, [0, 0x8000, 0x1_0000, 0x11_0000, 0x71_0000, 0xD000]);
        let read = |n: &str| std::fs::read(out.join(n)).unwrap();
        assert_eq!(
            dagon_sign::to_hex(&dagon_format::sha256(&read("bootloader.bin"))),
            "75e529456be566edb74e7e1436a45b614133d8d41896e747e9d6204329e8968f"
        );
        let (header, body) = (read("header.bin"), read("body.bin"));
        assert_eq!(header.len(), 512);
        assert_eq!(body, app, "the ESP image itself, after its header");
        assert_eq!([header, body].concat(), read("app-a.img"));
        assert_eq!(read("otadata.bin"), crate::esp::otadata_slot_a());
        assert_eq!(read("factory.elf"), std::fs::read(&i.bootcore).unwrap());

        let verbs: Vec<&str> = commands.iter().map(|c| c[0].as_str()).collect();
        assert_eq!(verbs, ["flash", "write-bin", "write-bin", "write-bin"]);
        let first = commands[0].join(" ");
        for want in [
            "--chip esp32s3",
            "--flash-size 16mb",
            "--partition-table-offset 0x8000",
            "--erase-parts otadata,dagon",
            "--target-app-partition factory",
        ] {
            assert!(first.contains(want), "{want}: {first}");
        }
        assert_eq!(commands[1][3], "0x110000");
        assert_eq!(commands[2][3], "0x710000");
        assert_eq!(commands[3][3], "0xd000");
        assert!(commands.iter().all(|c| !c.contains(&"--port".to_string())));
        let aimed = with_port(&commands, "COM7");
        assert!(aimed
            .iter()
            .all(|c| c[1..4] == ["--port", "COM7", "--non-interactive"]));
        assert!(out.join("commands.txt").exists());
        let _ = std::fs::remove_dir_all(&cfg.base);
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
    fn only_the_rp2040_boot_rom_is_an_rp2040_drive() {
        let rom = "UF2 Bootloader v3.0\r\nModel: Raspberry Pi RP2\r\nBoard-ID: RPI-RP2\r\n";
        assert!(is_rp2040_rom(rom));
        for not in [
            "UF2 Bootloader v3.0\nBoard-ID: NOT-RPI-RP2\n",
            "UF2 Bootloader v3.0\nBoard-ID: RPI-RP2-CLONE\n",
            "UF2 Bootloader v3.0\nBoard-ID: RPI-RP2350\n",
            "UF2 Bootloader v3.0\nModel: RPI-RP2\n",
            "UF2 Bootloader v3.0\nNote: Board-ID: RPI-RP2\n",
            "Board-ID: RPI-RP2\n",
            "My stick\nUF2 Bootloader v3.0\nBoard-ID: RPI-RP2\n",
            "\nUF2 Bootloader v3.0\nBoard-ID: RPI-RP2\n",
            "\r\nUF2 Bootloader v3.0\r\nBoard-ID: RPI-RP2\r\n",
            "  UF2 Bootloader v3.0\nBoard-ID: RPI-RP2\n",
            "UF2 Bootloader v3.0\n Board-ID: RPI-RP2\n",
            "UF2 Bootloader v3.0\nBoard-ID: RPI-RP2 \n",
            "UF2 Bootloader v3.0\nBoard-ID : RPI-RP2\n",
            "UF2 Bootloader v3.0\nBoard-ID:RPI-RP2\n",
            "UF2 Bootloader v3.0\nBoard-ID:  RPI-RP2\n",
            "UF2 Bootloader v3.0\r\n Board-ID : RPI-RP2 \r\n",
            "",
        ] {
            assert!(!is_rp2040_rom(not), "{not:?}");
        }
    }

    #[test]
    fn a_second_board_under_another_mount_name_is_seen() {
        let base = std::env::temp_dir().join(format!("dagon-mnt-{}", std::process::id()));
        let (volumes, media) = (base.join("Volumes"), base.join("media/user"));
        let info = "UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n";
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
