//! `dagon flash <id>`: the first write of a board, as its MCU definition
//! says. Builds the definition's parts (the boot core, stage0, a board's
//! recovery agent) and the app with the developer's key, makes the
//! first-install files as `provision` / `rescue hub` do, then hands them to
//! the definition's writer: a UF2 drive, probe-rs, or espflash (the bundled
//! 2nd stage, the partition table, the agent, slot A and otadata).
//! The plan is printed before anything is built; the device written to is
//! one the writer reports, chosen by the user when there are several, and
//! nothing touches it until the user confirms (`--yes`) or ever with
//! `--dry-run`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use dagon_topology::defs::{McuDef, Registry, Writer};
use dagon_topology::Topology;
use ed25519_compact::KeyPair;

use crate::commands::{command_lines, node_mcu, provision_files, FirstWrite};
use crate::config::{load_key, read_topology, Config, Opts, ProvisionInputs};
use crate::deploy::{confirm, firmware_key, YES};
use crate::esp::Esp;
use crate::flashdev::{self, Pick, DRIVE_WAIT};
use crate::nodebuild;
use crate::scaffold::Want;

const DRY_RUN: &str = "--dry-run";
/// Version of the first install when dagon.toml gives none.
const FIRST_VERSION: &str = "1.0.0";
/// The stage0 part (the layout's `stage0` region); every other part is
/// the boot image of the first install.
const STAGE0: &str = "stage0";

/// A DAGON firmware part built from the project's DAGON source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub name: String,
    /// The MCU or board package the part belongs to.
    pub owner: Want,
    /// The part's own package folder inside that package ("" for itself).
    pub sub: String,
    /// Binary target; `None`: the part package's own name.
    pub bin: Option<String>,
}

/// The parts `mcu`'s first install needs besides the app, for a node on
/// `board`: the MCU definition's, then the board definition's.
pub(crate) fn parts(reg: &Registry, mcu: &str, board: Option<&str>) -> Vec<Part> {
    let of = |owner: Want, p: &dagon_topology::defs::Part| Part {
        name: p.name.clone(),
        owner,
        sub: p.sub.clone(),
        bin: p.bin.clone(),
    };
    let from_mcu = reg
        .mcu(mcu)
        .into_iter()
        .flat_map(|m| &m.parts)
        .map(|p| of(Want::Mcu(mcu.into()), p));
    let from_board = board
        .and_then(|b| reg.board(b))
        .into_iter()
        .flat_map(|b| b.parts.iter().map(|p| of(Want::Board(b.id.clone()), p)));
    from_mcu.chain(from_board).collect()
}

/// The root of the project's DAGON source: its clone (path mode), or
/// cargo's checkout of the `[dagon]` release tag (fetched when missing).
pub(crate) fn source_root(cfg: &Config) -> Result<PathBuf, String> {
    let r = crate::dagonsrc::root(&cfg.dagon, &cfg.base)?;
    Ok(std::fs::canonicalize(&r).map_or(r, crate::config::plain_path))
}

/// Folder of the package `owner` under `root`.
fn package_dir(root: &Path, owner: &Want) -> Result<PathBuf, String> {
    crate::scaffold::find_for(root, owner)
        .map(|f| crate::config::plain_path(f.dir))
        .ok_or_else(|| {
            format!(
                "no package {} in {}",
                crate::scaffold::conventional_name(owner),
                root.display()
            )
        })
}

/// Package folder of `part` under `root`.
pub(crate) fn part_dir(root: &Path, part: &Part) -> Result<PathBuf, String> {
    let at = package_dir(root, &part.owner)?;
    Ok(if part.sub.is_empty() {
        at
    } else {
        at.join(&part.sub)
    })
}

/// Refuses a node crate whose DAGON packages come from another source than
/// `[dagon]`: its app and the boot images built here would be of different
/// DAGON versions.
pub(crate) fn same_source(cfg: &Config, node: &str) -> Result<(), String> {
    let b = cfg
        .build
        .get(node)
        .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
    let manifest = b.package.join("Cargo.toml");
    match crate::scaffold::source_mismatch(&cfg.base, &manifest, &cfg.dagon)? {
        None => Ok(()),
        Some(why) => Err(format!(
            "{}: {why}; run `dagon gen {node}` so the app and the boot images come from the \
             same DAGON. Nothing was built.",
            manifest.display()
        )),
    }
}

/// Whether `node`'s package lies inside the DAGON source at `root` (a
/// sample): built from that source, it is on the boot images' DAGON.
fn package_in_source(cfg: &Config, node: &str, root: &Path) -> bool {
    cfg.build
        .get(node)
        .is_some_and(|b| crate::paths::contained(root, &cfg.base.join(&b.package)).is_ok())
}

/// How the plan names a package or a part folder of the DAGON source.
fn origin(cfg: &Config, owner: &Want, sub: &str) -> String {
    let name = crate::scaffold::conventional_name(owner);
    let rel = if sub.is_empty() {
        name
    } else {
        format!("{name}/{sub}")
    };
    match crate::dagonsrc::clone_root(&cfg.dagon, &cfg.base) {
        Some(root) => package_dir(&root, owner).map_or(rel, |d| {
            (if sub.is_empty() { d } else { d.join(sub) })
                .display()
                .to_string()
        }),
        None => format!(
            "{rel} of DAGON {} ({})",
            cfg.dagon.tag.as_deref().unwrap_or("?"),
            cfg.dagon.git.as_deref().unwrap_or("?")
        ),
    }
}

/// `owner`'s package as a path from the DAGON source root (its package
/// name when there is no local clone).
fn source_relative(cfg: &Config, owner: &Want) -> String {
    let name = crate::scaffold::conventional_name(owner);
    let Some(root) = crate::dagonsrc::clone_root(&cfg.dagon, &cfg.base) else {
        return name;
    };
    let root = std::fs::canonicalize(&root).map_or(root, crate::config::plain_path);
    package_dir(&root, owner)
        .ok()
        .and_then(|d| d.strip_prefix(&root).ok().map(crate::config::forward_path))
        .unwrap_or(name)
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
    let reg = crate::defs::registry(cfg)?;
    let mcu = node_mcu(&reg, t, node)?;
    let parts = parts(&reg, &mcu.id, n.board.as_deref());
    if parts.is_empty() {
        return Err(match &n.board {
            None => format!(
                "'{node}' names no board and the {} definition names no first-write part \
                 (its boot image comes from a board package)",
                mcu.id
            ),
            Some(b) => format!(
                "neither the {} nor the {b} definition names a first-write part",
                mcu.id
            ),
        });
    }
    let mut lines = vec![format!("flash {node} ({}):", n.mcu)];
    if cfg.provision.contains_key(node) {
        lines.push(format!(
            "  ignore   [provision.{node}]: flash always builds with the signing key's public half"
        ));
    }
    for part in &parts {
        lines.push(format!(
            "  build    {:<8} {} (tag {})",
            part.name,
            origin(cfg, &part.owner, &part.sub),
            n.tag
        ));
    }
    let b = cfg
        .build
        .get(node)
        .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
    lines.push(format!("  build    app      {}", b.package.display()));
    lines.extend(write_plan(cfg, mcu, out)?);
    Ok(lines)
}

/// The plan's lines for the files made and the write, by writer kind.
fn write_plan(cfg: &Config, mcu: &McuDef, out: &Path) -> Result<Vec<String>, String> {
    let via = &mcu.connection.label;
    Ok(match &mcu.writer {
        Writer::Uf2Drive { board_id, .. } => vec![
            format!("  make     {}", out.join("rescue.uf2").display()),
            format!(
                "  write    copy it to the UF2 drive chosen after the build (Board-ID {board_id} \
                 is the {}; waits {} s for one)",
                mcu.name,
                DRIVE_WAIT.as_secs()
            ),
        ],
        Writer::ProbeRs { .. } => vec![
            format!("  make     {}", out.join("flash.bin").display()),
            format!("  write    probe-rs erase, download, reset through the {via} chosen after the build"),
        ],
        Writer::Espflash { usb_hint, .. } => {
            let esp = Esp::of(mcu).expect("espflash writer");
            let sums = Path::new(esp.checksums)
                .file_name()
                .map_or(esp.checksums.into(), |n| n.to_string_lossy());
            let mut l = vec![format!(
                "  check    {}/{} of the DAGON source against {sums} (the unmodified ESP-IDF 2nd stage)",
                source_relative(cfg, &Want::Mcu(mcu.id.clone())),
                esp.bootloader
            )];
            l.push(format!("  make     in {}:", out.display()));
            l.extend(
                esp_files_plan(mcu, &esp)?
                    .into_iter()
                    .map(|f| format!("             {f}")),
            );
            let hint = usb_hint.map_or(String::new(), |h| {
                format!(" (USB {:04x}:{:04x} is the {})", h.vid, h.pid, mcu.name)
            });
            l.push(format!(
                "  write    espflash over {via} to the serial port chosen after the build{hint}"
            ));
            l
        }
    })
}

/// The first-install inputs, all built now with the signing key's public
/// half embedded. Prebuilt `[provision.<id>]` files are never used: a boot
/// core with another key would refuse every image signed later.
fn inputs(
    cfg: &Config,
    reg: &Registry,
    t: &Topology,
    node: &str,
    key: &KeyPair,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<ProvisionInputs, String> {
    let n = t.node(node).ok_or("checked in plan")?;
    let root = source_root(cfg)?;
    let in_source = package_in_source(cfg, node, &root);
    if !in_source {
        same_source(cfg, node)?;
    }
    let fw_key = firmware_key(cfg, key)?;
    let hex = dagon_sign::to_hex(&fw_key);
    log(&format!("{node}: DAGON source {}", root.display()));
    let mut built = Vec::new();
    for part in parts(reg, &n.mcu, n.board.as_deref()) {
        let dir = part_dir(&root, &part)?;
        let bin = match &part.bin {
            Some(b) => b.clone(),
            None => nodebuild::package_name(&dir)?,
        };
        let target = cfg.base.join("out/flash/target").join(&part.name);
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
    let source = in_source.then_some(root.as_path());
    nodebuild::build_nodes_keyed(cfg, &[node.to_string()], Some(fw_key), source, log, cancel)?;
    let stage0 = built.iter().find(|p| p.0 == STAGE0).map(|p| p.1.clone());
    let version = cfg.build.get(node).and_then(|b| b.version.clone());
    Ok(ProvisionInputs {
        stage0,
        // The boot core, or the agent that stands where it is.
        bootcore: built
            .iter()
            .find(|p| p.0 != STAGE0)
            .map(|p| p.1.clone())
            .ok_or("no boot image part")?,
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
    let reg = crate::defs::registry(cfg)?;
    let i = inputs(cfg, &reg, t, node, key, log, cancel)?;
    let mcu = node_mcu(&reg, t, node)?;
    match Esp::of(mcu) {
        Some(esp) => {
            let app = crate::esp::save_image(&esp, &i.app, &out.join("app.bin"))?;
            esp_files(&reg, t, node, &i, &app, &source_root(cfg)?, out, key)
        }
        None => provision_files(&reg, t, node, &i, out, key),
    }
}

/// The espflash first-install files, as the plan names them.
fn esp_files_plan(mcu: &McuDef, esp: &Esp<'_>) -> Result<Vec<String>, String> {
    let l = crate::esp::layout(mcu)?;
    let row = |file: &str, at: String, what: &str| format!("{file:<16}{at:<13}({what})");
    Ok(vec![
        row(
            "bootloader.bin",
            format!("at {:#x}", crate::esp::BOOTLOADER_AT),
            "2nd stage",
        ),
        row(
            "partitions.csv",
            format!("at {:#x}", esp.table_at),
            "partition table",
        ),
        row(
            "factory.elf",
            format!("in {}", l.factory.0),
            "recovery agent",
        ),
        row("body.bin", format!("in {}", l.ota_0.0), "app, slot A"),
        row(
            "header.bin",
            format!("in {}", l.dagon.0),
            "its signed header",
        ),
        row(
            "otadata.bin",
            format!("in {}", crate::esp::OTADATA),
            &format!("{} selected, confirmed", l.ota_0.0),
        ),
    ])
}

/// Writes the espflash first-install files into `out` and returns the
/// espflash commands (without the port, chosen when writing): the checked
/// 2nd stage, the partition table and the recovery agent, then slot A's
/// body and header and the otadata entry that selects it.
#[allow(clippy::too_many_arguments)]
fn esp_files(
    reg: &Registry,
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
    let mcu = node_mcu(reg, t, node)?;
    let e = Esp::of(mcu).ok_or("not an espflash MCU")?;
    let pkg = package_dir(home, &Want::Mcu(mcu.id.clone()))?;
    let (bl_path, sums, csv_path) = esp::vendor_files(mcu, &e, &pkg)?;
    let boot = esp::checked_bootloader(&bl_path, &sums)?;
    let csv =
        std::fs::read_to_string(&csv_path).map_err(|e| format!("{}: {e}", csv_path.display()))?;
    let l = esp::layout(mcu)?;
    let version = crate::commands::version(i.version.as_deref().unwrap_or(FIRST_VERSION))?;
    let opts = crate::commands::SignOpts {
        version,
        release_seq: i.release_seq.unwrap_or(1),
        min_bootcore: dagon_format::Version::default(),
        proto: i.proto_version.unwrap_or(1),
        bits: None,
    };
    let img = crate::commands::image_for_node_with(
        reg,
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
    let common = [s("--chip"), s(e.chip)];
    let mut first = vec![s("flash")];
    first.extend(common.clone());
    first.extend([
        s("--flash-size"),
        s(e.flash_size),
        s("--partition-table-offset"),
        format!("{:#x}", e.table_at),
        s("--bootloader"),
        p(&bootloader),
        s("--partition-table"),
        p(&table),
        s("--erase-parts"),
        format!("{},{}", esp::OTADATA, l.dagon.0),
        s("--target-app-partition"),
        l.factory.0.clone(),
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
        write_bin(l.ota_0.1, &body),
        write_bin(l.dagon.1, &header),
        write_bin(l.otadata, &otadata),
    ];
    let files = vec![
        (esp::BOOTLOADER_AT, bootloader),
        (e.table_at, table),
        (l.factory.1, factory),
        (l.ota_0.1, body),
        (l.dagon.1, header),
        (l.otadata, otadata),
    ];
    crate::config::write_file(
        &out.join("commands.txt"),
        command_lines("espflash", &commands).as_bytes(),
    )?;
    Ok(FirstWrite::Espflash { files, commands })
}

/// The MCU families `flash` can write (the page offers it for these).
pub(crate) fn flashable(mcu: &str) -> bool {
    Registry::builtin().mcu(mcu).is_some()
}

/// Every device `node`'s writer reports now (a UF2 writer waits for its
/// drive), with the writer.
pub(crate) fn devices(
    cfg: &Config,
    t: &Topology,
    node: &str,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<(Writer, Vec<flashdev::Candidate>), String> {
    let reg = crate::defs::registry(cfg)?;
    let mcu = node_mcu(&reg, t, node)?;
    Ok((mcu.writer.clone(), flashdev::find(mcu, log, cancel)?))
}

/// Writes the first-install files to `device` (after the confirmation).
pub(crate) fn write(
    w: &FirstWrite,
    c: &flashdev::Candidate,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let device = c.id.as_str();
    match w {
        FirstWrite::ProbeRs { commands, .. } => run_probe_rs(&with_probe(commands, device), log),
        FirstWrite::Uf2 { file, .. } => {
            copy_uf2(file, c.path.as_deref().unwrap_or(Path::new(device)), log)
        }
        FirstWrite::Espflash { commands, .. } => {
            run_espflash(&with_port(commands, device), log, cancel)
        }
    }
}

/// `commands` with `extra` after each verb.
fn after_verb(commands: &[Vec<String>], extra: &[&str]) -> Vec<Vec<String>> {
    commands
        .iter()
        .map(|c| {
            let mut a = vec![c[0].clone()];
            a.extend(extra.iter().map(|s| s.to_string()));
            a.extend(c[1..].iter().cloned());
            a
        })
        .collect()
}

/// `commands` aimed at `port`, never asking which port to use.
fn with_port(commands: &[Vec<String>], port: &str) -> Vec<Vec<String>> {
    after_verb(commands, &["--port", port, "--non-interactive"])
}

/// `commands` aimed at `probe` (`vid:pid[:serial]`).
fn with_probe(commands: &[Vec<String>], probe: &str) -> Vec<Vec<String>> {
    after_verb(commands, &["--probe", probe])
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

fn copy_uf2(file: &Path, drive: &Path, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let dest = drive.join("rescue.uf2");
    std::fs::copy(file, &dest).map_err(|e| format!("cannot copy to {}: {e}", dest.display()))?;
    log(&format!(
        "copied to {}; the board restarts on its own",
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

/// What the dry run prints instead of writing.
fn dry_run(w: &FirstWrite) {
    match w {
        FirstWrite::ProbeRs { commands, .. } => print!(
            "dry run; would run, on the probe chosen:\n{}",
            command_lines("probe-rs", &with_probe(commands, "<probe>"))
        ),
        FirstWrite::Uf2 { file, .. } => println!(
            "dry run; would copy {} to the UF2 drive chosen",
            file.display()
        ),
        FirstWrite::Espflash { files, commands } => {
            println!("dry run; the flash would hold:");
            for (at, f) in files {
                println!("  {at:#09x}  {}", f.display());
            }
            print!(
                "and would run, on the port chosen:\n{}",
                command_lines("espflash", &with_port(commands, "<port>"))
            )
        }
    }
}

/// `dagon flash <id> [--dry-run] [--yes] [--probe <vid:pid[:serial]> |
/// --port <port> | --drive <path>] [--out <dir>] [--topology <yaml>]
/// [--key <file>] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let dry = args.contains(&DRY_RUN);
    let yes = args.contains(&YES);
    let args: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != DRY_RUN && *a != YES)
        .collect();
    let o = Opts::parse(
        &args,
        &["out", "topology", "key", "config", "probe", "port", "drive"],
    )?;
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
    let selector = selector(&o, &cfg, &t, node)?;
    let key = load_key(o.get("key"))?;
    let mut log = |l: &str| println!("{l}");
    let cancel = AtomicBool::new(false);
    let w = prepare(&cfg, &t, node, &key, &out, &mut log, &cancel)?;
    if dry {
        dry_run(&w);
        return Ok(());
    }
    let (writer, found) = devices(&cfg, &t, node, &mut log, &cancel)?;
    let device = match flashdev::pick(found, selector, &writer)? {
        Pick::Use(c) => c,
        Pick::Choose(list) if !yes && std::io::stdin().is_terminal() => {
            flashdev::choose(&list, &writer, &mut std::io::stdin().lock())?
        }
        Pick::Choose(list) => return Err(flashdev::several(&list, &writer)),
    };
    println!(
        "  device   --{} {}  {}",
        flashdev::flag(&writer),
        device.id,
        device.label
    );
    if !yes && !confirm("", &mut std::io::stdin().lock()) {
        return Err("nothing written to the board".into());
    }
    write(&w, &device, &mut log, &cancel)
}

/// The device option given, which must be the one `node`'s writer takes.
fn selector<'a>(
    o: &'a Opts<'_>,
    cfg: &Config,
    t: &Topology,
    node: &str,
) -> Result<Option<&'a str>, String> {
    let given: Vec<(&str, &str)> = ["probe", "port", "drive"]
        .into_iter()
        .filter_map(|f| o.get(f).map(|v| (f, v)))
        .collect();
    let [(f, v)] = given[..] else {
        return match given.len() {
            0 => Ok(None),
            _ => Err("name the device once (--probe, --port or --drive)".into()),
        };
    };
    let reg = crate::defs::registry(cfg)?;
    let want = flashdev::flag(&node_mcu(&reg, t, node)?.writer);
    if f != want {
        return Err(format!(
            "'{node}' is written to a device named with --{want}, not --{f}"
        ));
    }
    Ok(Some(v))
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

    /// The DAGON checkout this test runs in.
    fn here() -> PathBuf {
        std::fs::canonicalize(crate::dagonsrc::BUILT_FROM)
            .map(crate::config::plain_path)
            .unwrap()
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
        match provision_files(
            Registry::builtin(),
            &t,
            "hub",
            &built(&cfg, true),
            &out,
            &key(),
        )
        .unwrap()
        {
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
        match provision_files(
            Registry::builtin(),
            &t,
            "seg",
            &built(&cfg, false),
            &out,
            &key(),
        )
        .unwrap()
        {
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
            parts(Registry::builtin(), "esp32s3", None).is_empty(),
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
    fn in_release_mode_the_boot_images_come_from_cargos_checkout_of_the_tag() {
        let (mut cfg, t) = area("tag");
        let pkg = crate::dagonsrc::tests::package;
        let url = crate::dagonsrc::tests::repo(
            &cfg.base.join("release"),
            "v3.1.4",
            &[
                ("crates/dagon-format/Cargo.toml", &pkg("dagon-format")),
                ("crates/dagon-format/lib.rs", ""),
                ("targets/mcu/rp2040/Cargo.toml", &pkg("dagon-mcu-rp2040")),
                ("targets/mcu/rp2040/lib.rs", ""),
                (
                    "targets/mcu/rp2040/stage0/Cargo.toml",
                    &pkg("dagon-rp2040-stage0"),
                ),
                ("targets/mcu/rp2040/stage0/lib.rs", ""),
                (
                    "targets/mcu/rp2040/bootcore/Cargo.toml",
                    &pkg("dagon-rp2040-bootcore"),
                ),
                ("targets/mcu/rp2040/bootcore/lib.rs", ""),
            ],
        );
        cfg.dagon = crate::scaffold::DagonSource {
            git: Some(url.clone()),
            tag: Some("v3.1.4".into()),
            path: None,
        };
        let plan = plan(&cfg, &t, "hub", &cfg.base.join("out"))
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("dagon-mcu-rp2040/bootcore of DAGON v3.1.4"),
            "{plan}"
        );

        let root = source_root(&cfg).unwrap();
        assert!(
            !root.starts_with(&cfg.base),
            "cargo's checkout, not the repository"
        );
        for p in parts(Registry::builtin(), "rp2040", None) {
            let dir = part_dir(&root, &p).unwrap();
            assert!(dir.starts_with(&root) && dir.join("Cargo.toml").is_file());
            assert!(dir.ends_with(&p.sub), "{}", dir.display());
        }

        cfg.dagon.tag = Some("v0.0.0-missing".into());
        let e = source_root(&cfg).unwrap_err();
        assert!(
            e.contains("cannot get the DAGON source v0.0.0-missing"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn a_node_crate_on_another_dagon_source_is_refused_before_building() {
        let (mut cfg, _) = area("mix");
        cfg.build.get_mut("seg").unwrap().package = PathBuf::from("nodes/seg");
        let manifest = cfg.base.join("nodes/seg/Cargo.toml");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        let write = |dep: &str| {
            std::fs::write(
                &manifest,
                format!("[package]\nname = \"seg\"\n\n[dependencies]\n{dep}\nheapless = \"0.8\"\n"),
            )
            .unwrap()
        };
        cfg.dagon = crate::scaffold::DagonSource {
            git: Some("file:///dagon".into()),
            tag: Some("v1.1.0".into()),
            path: None,
        };

        write(r#"dagon-board-nucleo-f401re = { git = "file:///dagon", tag = "v1.1.0" }"#);
        assert!(same_source(&cfg, "seg").is_ok());

        // gen ran for v1.0.0; dagon.toml now says v1.1.0.
        write(r#"dagon-board-nucleo-f401re = { git = "file:///dagon", tag = "v1.0.0" }"#);
        let e = same_source(&cfg, "seg").unwrap_err();
        assert!(e.contains("v1.0.0") && e.contains("v1.1.0"), "{e}");
        assert!(e.contains("run `dagon gen seg`"), "{e}");

        // A crate still on a local clone while dagon.toml pins a release.
        write(r#"dagon-board-nucleo-f401re = { path = "C:/somewhere/else" }"#);
        assert!(same_source(&cfg, "seg").is_err());

        // Path mode: a path inside the clone matches, a release does not.
        let clone = cfg.base.join("clone");
        std::fs::create_dir_all(clone.join("boards/nucleo")).unwrap();
        cfg.dagon = crate::scaffold::DagonSource {
            git: None,
            tag: None,
            path: Some(PathBuf::from("clone")),
        };
        let inside = crate::config::forward_path(&clone.join("boards/nucleo"));
        write(&format!(
            "dagon-board-nucleo-f401re = {{ path = {inside:?} }}"
        ));
        assert!(same_source(&cfg, "seg").is_ok());
        write(r#"dagon-board-nucleo-f401re = { git = "file:///dagon", tag = "v1.1.0" }"#);
        assert!(same_source(&cfg, "seg").is_err());
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn the_parts_are_in_this_checkout() {
        let reg = &crate::defs::registry_at(&Config::default(), Some(&here())).unwrap();
        for mcu in reg.mcus().iter().map(|m| m.id.as_str()) {
            let board = reg.default_board(mcu).map(|b| b.id.as_str());
            for p in parts(reg, mcu, board) {
                let toml = part_dir(&here(), &p).unwrap().join("Cargo.toml");
                assert!(toml.is_file(), "{} is not in the checkout", toml.display());
            }
        }
        let names: Vec<String> = parts(reg, "rp2040", None)
            .into_iter()
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
            "m5stopwatch (tag watch-s3)",
            "SHA256SUMS",
            "bootloader.bin  at 0x0",
            "otadata.bin     in otadata",
            "USB 303a:1001",
        ] {
            assert!(plan.contains(want), "{want}: {plan}");
        }

        let i = built(&cfg, false);
        let app = vec![0xE9; 7000];
        let reg = Registry::builtin();
        let w = esp_files(reg, &t, "watch", &i, &app, &here(), &out, &key()).unwrap();
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

    /// An MCU the project registers (existing kinds only): flash plans it,
    /// makes its files and aims probe-rs at it with no DAGON code for it.
    #[test]
    fn a_project_mcu_is_flashed_from_its_definition_alone() {
        let (mut cfg, _) = area("fake");
        let mcu = include_str!("../../dagon-topology/src/defs/tests.rs");
        let def = |name: &str| {
            let at = mcu.find(&format!("const {name}: &str = r#\"")).unwrap();
            let rest = &mcu[at..];
            let start = rest.find("r#\"").unwrap() + 3;
            rest[start..start + rest[start..].find("\"#;").unwrap()].to_string()
        };
        let pkg = cfg.base.join("m");
        std::fs::create_dir_all(pkg.join("bootcore")).unwrap();
        std::fs::write(pkg.join("dagon-mcu.toml"), def("FAKE_MCU")).unwrap();
        let board = def("FAKE_BOARD").replace("product", "[board.fake-board]\nproduct");
        let toml = cfg.base.join(crate::config::CONFIG_FILE);
        std::fs::write(
            &toml,
            format!("mcu_files = [\"m/dagon-mcu.toml\"]\n{board}"),
        )
        .unwrap();
        let yaml = YAML
            .replace("mcu: stm32f401re", "mcu: fake-m0")
            .replace("board: nucleo-f401re", "board: fake-board")
            .replace("flash_map: f401-default", "flash_map: fake-1m")
            .replace("{ sda: PB4, scl: PA8 }", "{ sda: GPIO2, scl: GPIO3 }");
        let t = dagon_topology::parse_str(&yaml).unwrap();
        cfg.build.get_mut("seg").unwrap().package = PathBuf::from("nodes/seg");

        let out = cfg.base.join("out/seg");
        let plan = plan(&cfg, &t, "seg", &out).unwrap().join("\n");
        assert!(plan.contains("flash seg (fake-m0):"), "{plan}");
        assert!(
            plan.contains("build    bootcore dagon-mcu-fake-m0/bootcore"),
            "{plan}"
        );
        assert!(plan.contains("through the SWD"), "{plan}");

        let reg = crate::defs::registry(&cfg).unwrap();
        let FirstWrite::ProbeRs { bin, commands, .. } =
            provision_files(&reg, &t, "seg", &built(&cfg, false), &out, &key()).unwrap()
        else {
            panic!("a probe-rs writer")
        };
        assert_eq!(
            commands[1][..7],
            [
                "download",
                "--chip",
                "FAKE0",
                "--binary-format",
                "bin",
                "--base-address",
                "0x00100000"
            ]
        );
        let flash = std::fs::read(&bin).unwrap();
        // The app's inline header at its slot (0x11_0000 from the base).
        assert_eq!(&flash[0x1_0000..0x1_0004], &dagon_format::header::MAGIC);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn a_package_in_the_dagon_source_needs_no_source_check() {
        let (mut cfg, _) = area("insrc");
        let root = here();
        cfg.build.get_mut("seg").unwrap().package = root.join("samples/f401-app");
        assert!(package_in_source(&cfg, "seg", &root));
        assert!(
            !package_in_source(&cfg, "hub", &root),
            "targets/rp2040-hub is in the project"
        );
        let _ = std::fs::remove_dir_all(&cfg.base);
    }
}
