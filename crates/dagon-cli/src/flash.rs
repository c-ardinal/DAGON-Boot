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

use crate::cli::FlashArgs;
use crate::commands::{command_lines, node_mcu, provision_files, FirstWrite};
use crate::config::{load_key, read_topology, Config, ProvisionInputs};
use crate::deploy::{confirm, firmware_key};
use crate::esp::Esp;
use crate::flashdev::{self, Pick, DRIVE_WAIT};
use crate::nodebuild;
use crate::scaffold::Want;

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
    /// The owner's package folder when a project defines it (else found
    /// in the DAGON source).
    pub package: Option<PathBuf>,
}

/// The parts `mcu`'s first install needs besides the app, for a node on
/// `board`: the MCU definition's, then the board definition's.
pub(crate) fn parts(reg: &Registry, mcu: &str, board: Option<&str>) -> Vec<Part> {
    let of = |owner: Want, package: &Option<PathBuf>, p: &dagon_topology::defs::Part| Part {
        name: p.name.clone(),
        owner,
        sub: p.sub.clone(),
        bin: p.bin.clone(),
        package: package.clone(),
    };
    let from_mcu = reg.mcu(mcu).into_iter().flat_map(|m| {
        m.parts
            .iter()
            .map(|p| of(Want::Mcu(mcu.into()), &m.package, p))
    });
    let from_board = board.and_then(|b| reg.board(b)).into_iter().flat_map(|b| {
        b.parts
            .iter()
            .map(|p| of(Want::Board(b.id.clone()), &b.package, p))
    });
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
    let at = match &part.package {
        Some(p) => p.clone(),
        None => package_dir(root, &part.owner)?,
    };
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

/// How plan lines name local folders and files: in full (the terminal),
/// or relative to the project or the DAGON source (the page: no path of
/// this machine leaves it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Paths {
    Full,
    Relative,
}

/// `p` as plan lines show it: in full, or relative to the project (its
/// file name when outside it).
fn shown(cfg: &Config, p: &Path, paths: Paths) -> String {
    if paths == Paths::Full {
        return p.display().to_string();
    }
    let base = std::fs::canonicalize(&cfg.base).map_or(cfg.base.clone(), crate::config::plain_path);
    [&cfg.base, &base]
        .iter()
        .find_map(|b| p.strip_prefix(b).ok())
        .map(crate::config::forward_path)
        .or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Every spelling of `dir` a line may carry: as given and canonical.
pub(crate) fn spellings(dir: &Path) -> Vec<String> {
    let canon = std::fs::canonicalize(dir).map(crate::config::plain_path);
    let mut v: Vec<String> = [Some(dir.to_path_buf()), canon.ok()]
        .into_iter()
        .flatten()
        .map(|d| d.display().to_string())
        .collect();
    v.dedup();
    v
}

/// Every folder of this machine a serve job's lines may name for project
/// `cfg`, and what the page sees instead, most specific first: each app
/// (its package name), each project-defined MCU/board package, the DAGON
/// source, the project, the home folder (cargo's registry paths).
pub(crate) fn page_swaps(cfg: &Config, reg: &Registry) -> Vec<(String, String)> {
    let mut swaps = Vec::new();
    for (node, b) in &cfg.build {
        let label = app_label(cfg, node);
        for f in spellings(&cfg.base.join(&b.package)) {
            swaps.push((f, label.clone()));
        }
        // As dagon.toml writes it, either slash (a bare one-word folder
        // would match ordinary words in the lines; its absolute form is
        // replaced above).
        let written = b.package.display().to_string();
        if written.contains(['/', '\\']) {
            swaps.push((written.replace('\\', "/"), label.clone()));
            swaps.push((written.replace('/', "\\"), label));
        }
    }
    let owned = reg
        .mcus()
        .iter()
        .map(|m| (Want::Mcu(m.id.clone()), &m.package))
        .chain(
            reg.boards()
                .iter()
                .map(|b| (Want::Board(b.id.clone()), &b.package)),
        );
    for (owner, package) in owned {
        if let Some(p) = package {
            let label = project_package(&owner);
            swaps.extend(spellings(p).into_iter().map(|f| (f, label.clone())));
        }
    }
    if let Some(root) = crate::dagonsrc::clone_root(&cfg.dagon, &cfg.base) {
        swaps.extend(
            spellings(&root)
                .into_iter()
                .map(|f| (f, "<DAGON source>".into())),
        );
    }
    swaps.extend(base_swaps(Some(&cfg.base)));
    most_specific_first(swaps)
}

/// What every page-bound line hides even before a project loads: the
/// project's folder (when known) as <project>, the home folder as ~.
pub(crate) fn base_swaps(project: Option<&Path>) -> Vec<(String, String)> {
    let mut swaps = Vec::new();
    if let Some(p) = project {
        swaps.extend(spellings(p).into_iter().map(|f| (f, "<project>".into())));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    if let Some(h) = home.filter(|h| !h.is_empty()) {
        swaps.extend(
            spellings(Path::new(&h))
                .into_iter()
                .map(|f| (f, "~".into())),
        );
    }
    most_specific_first(swaps)
}

/// Longest folder first, so a folder inside another is replaced before it
/// (stable: equal lengths keep their order).
pub(crate) fn most_specific_first(mut swaps: Vec<(String, String)>) -> Vec<(String, String)> {
    swaps.sort_by_key(|s| std::cmp::Reverse(s.0.len()));
    swaps
}

/// `line` with each folder of `swaps` replaced by its name.
pub(crate) fn hidden(line: &str, swaps: &[(String, String)]) -> String {
    swaps
        .iter()
        .filter(|(f, _)| !f.is_empty())
        .fold(line.to_string(), |l, (f, to)| l.replace(f.as_str(), to))
}

/// How the page names `node`'s app: its Cargo package name, never its folder.
fn app_label(cfg: &Config, node: &str) -> String {
    cfg.build
        .get(node)
        .and_then(|b| nodebuild::package_name(&cfg.base.join(&b.package)).ok())
        .map_or_else(
            || format!("the {node} app"),
            |n| format!("{n} (the {node} app)"),
        )
}

/// How the page's plan names a project's own package (no folder of it).
fn project_package(owner: &Want) -> String {
    format!(
        "{} (the project's own package)",
        crate::scaffold::conventional_name(owner)
    )
}

/// How the plan names a part's folder: in its project package, or in the
/// DAGON source.
fn part_origin(cfg: &Config, part: &Part, paths: Paths) -> String {
    match (&part.package, paths) {
        (Some(p), Paths::Full) => {
            let dir = if part.sub.is_empty() {
                p.clone()
            } else {
                p.join(&part.sub)
            };
            dir.display().to_string()
        }
        // The page never gets a project's own folder names, only the package.
        (Some(_), Paths::Relative) => project_package(&part.owner),
        (None, Paths::Full) => origin(cfg, &part.owner, &part.sub),
        (None, Paths::Relative) => {
            let rel = source_relative(cfg, &part.owner);
            let rel = if part.sub.is_empty() {
                rel
            } else {
                format!("{rel}/{}", part.sub)
            };
            format!("{rel} of the DAGON source")
        }
    }
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
    paths: Paths,
) -> Result<Vec<String>, String> {
    let n = t
        .node(node)
        .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?;
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
            part_origin(cfg, part, paths),
            n.tag
        ));
    }
    let b = cfg
        .build
        .get(node)
        .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
    lines.push(format!(
        "  build    app      {}",
        match paths {
            Paths::Full => shown(cfg, &b.package, paths),
            Paths::Relative => app_label(cfg, node),
        }
    ));
    lines.extend(write_plan(cfg, mcu, out, paths)?);
    Ok(lines)
}

/// The plan's lines for the files made and the write, by writer kind.
fn write_plan(cfg: &Config, mcu: &McuDef, out: &Path, paths: Paths) -> Result<Vec<String>, String> {
    let via = &mcu.connection.label;
    Ok(match &mcu.writer {
        Writer::Uf2Drive { board_id, .. } => vec![
            format!("  make     {}", shown(cfg, &out.join("rescue.uf2"), paths)),
            format!(
                "  write    copy it to the UF2 drive chosen after the build (Board-ID {board_id} \
                 is the {}; waits {} s for one)",
                mcu.name,
                DRIVE_WAIT.as_secs()
            ),
        ],
        Writer::ProbeRs { .. } => vec![
            format!("  make     {}", shown(cfg, &out.join("flash.bin"), paths)),
            format!("  write    probe-rs erase, download, reset through the {via} chosen after the build"),
        ],
        Writer::Espflash { usb_hint, .. } => {
            let esp = Esp::of(mcu).expect("espflash writer");
            let sums = Path::new(esp.checksums)
                .file_name()
                .map_or(esp.checksums.into(), |n| n.to_string_lossy());
            let check = match (&mcu.package, paths) {
                // A project's own MCU package.
                (Some(p), Paths::Full) => format!(
                    "{} against {sums}",
                    p.join(esp.bootloader).display()
                ),
                (Some(_), Paths::Relative) => format!(
                    "the 2nd stage of {} against its checksums",
                    project_package(&Want::Mcu(mcu.id.clone()))
                ),
                (None, _) => format!(
                    "{}/{} of the DAGON source against {sums}",
                    source_relative(cfg, &Want::Mcu(mcu.id.clone())),
                    esp.bootloader
                ),
            };
            let mut l = vec![format!(
                "  check    {check} (the unmodified ESP-IDF 2nd stage)"
            )];
            l.push(format!("  make     in {}:", shown(cfg, out, paths)));
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
#[allow(clippy::too_many_arguments)]
fn inputs(
    cfg: &Config,
    reg: &Registry,
    t: &Topology,
    node: &str,
    key: &KeyPair,
    paths: Paths,
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
    log(&match paths {
        Paths::Full => format!("{node}: DAGON source {}", root.display()),
        Paths::Relative => format!("{node}: building with the project's DAGON source"),
    });
    let built = build_parts(cfg, reg, n, &root, &hex, paths, log, cancel)?;
    let source = in_source.then_some(root.as_path());
    build_app(cfg, node, fw_key, source, paths, log, cancel)?;
    let stage0 = built.iter().find(|p| p.0 == STAGE0).map(|p| p.1.clone());
    let (version, bootcore_version) = first_versions(cfg, node)?;
    Ok(ProvisionInputs {
        stage0,
        // The boot core, or the agent that stands where it is.
        bootcore: built
            .iter()
            .find(|p| p.0 != STAGE0)
            .map(|p| p.1.clone())
            .ok_or("no boot image part")?,
        app: nodebuild::elf_path(cfg, node, 'a'),
        version: Some(version),
        bootcore_version,
        release_seq: None,
        proto_version: None,
    })
}

/// The versions a first write signs node `node` with: its crate's Cargo.toml
/// `version`, and its boot crate's for its boot core (none without a boot
/// crate: an MCU without one has no boot core of its own).
fn first_versions(cfg: &Config, node: &str) -> Result<(String, Option<String>), String> {
    let app = crate::deploy::app_version(cfg, node, None)?;
    let boot = cfg.base.join(crate::extgen::BOOT_CRATES).join(node);
    let core = boot
        .join("Cargo.toml")
        .is_file()
        .then(|| crate::srcver::crate_version(&boot))
        .transpose()?;
    Ok((app.to_string(), core.map(|v| v.to_string())))
}

/// Builds `node`'s app with the firmware key (`source`: the DAGON source
/// when the app is one of its samples). For the page, the app's folder is
/// its package name and the project's folder is <project>, in DAGON's
/// lines and cargo's alike (errors included).
pub(crate) fn build_app(
    cfg: &Config,
    node: &str,
    fw_key: [u8; 32],
    source: Option<&Path>,
    paths: Paths,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut swaps = Vec::new();
    if paths == Paths::Relative {
        if let Some(b) = cfg.build.get(node) {
            let label = app_label(cfg, node);
            for f in spellings(&cfg.base.join(&b.package)) {
                swaps.push((f, label.clone()));
            }
            swaps.push((b.package.display().to_string(), label));
        }
        for f in spellings(&cfg.base) {
            swaps.push((f, "<project>".to_string()));
        }
    }
    let hide = |l: &str| hidden(l, &swaps);
    nodebuild::build_nodes_keyed(
        cfg,
        &[node.to_string()],
        Some(fw_key),
        source,
        &mut |l| log(&hide(l)),
        cancel,
    )
    .map_err(|e| hide(&e))?;
    Ok(())
}

/// Builds the first-write parts of node `n` with the firmware key `hex`:
/// a project-defined package's in that package, the others found in the
/// DAGON source at `root`. Returns (part name, built executable).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_parts(
    cfg: &Config,
    reg: &Registry,
    n: &dagon_topology::schema::Node,
    root: &Path,
    hex: &str,
    paths: Paths,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<(String, PathBuf)>, String> {
    let node = &n.id;
    let mut built = Vec::new();
    // The node's boot stages (stage0, boot core) are built for its boot crate.
    let boot = boot_crate(cfg, n).ok();
    let link_baud = nodebuild::link_baud(cfg, node)?;
    for part in parts(reg, &n.mcu, n.board.as_deref()) {
        let mut dir = part_dir(root, &part)?;
        let boot_folder = boot_folder_of(&part, &dir, boot.as_deref())?;
        let mut bin = match &part.bin {
            Some(b) => b.clone(),
            None => nodebuild::package_name(&dir)?,
        };
        // A boot core library is linked into the node's boot crate, which
        // is what builds (slot A, as for a first write).
        if boot_library(&dir)? {
            dir = boot_crate(cfg, n)?;
            bin = BOOT_BIN.to_string();
        }
        let target = cfg.base.join("out/flash/target").join(&part.name);
        log(&format!(
            "{node}: building {} in {}",
            part.name,
            match paths {
                Paths::Full => dir.display().to_string(),
                Paths::Relative => part_origin(cfg, &part, paths),
            }
        ));
        // cargo's own lines and errors name the folder: for the page, a
        // project package's folder becomes its package name.
        let swaps: Vec<(String, String)> = match (&part.package, paths) {
            (Some(_), Paths::Relative) => {
                let label = project_package(&part.owner);
                spellings(&dir)
                    .into_iter()
                    .map(|f| (f, label.clone()))
                    .collect()
            }
            _ => Vec::new(),
        };
        let hide = |l: &str| hidden(l, &swaps);
        let exe = nodebuild::cargo_build(
            &dir,
            &bin,
            &[],
            Some(&target),
            nodebuild::FirmwareEnv {
                pubkey_hex: Some(hex),
                // The boot core accepts only images carrying this tag.
                target_tag: Some(&n.tag),
                boot_crate: boot_folder,
                // A recovery agent talks over the same link as the app.
                link_baud,
            },
            &mut |l| log(&hide(l)),
            cancel,
        )
        .map_err(|e| hide(&e))?;
        built.push((part.name, exe));
    }
    Ok(built)
}

/// Node `n`'s boot core (its boot crate `dagon gen` writes into
/// `boot/<id>/`), built once per boot core slot its MCU definition
/// declares (slot B links with the `slot-b` feature), for a release:
/// `(slot, ELF bytes)`. Empty when the MCU keeps no boot core slots.
pub(crate) fn build_core_slots(
    cfg: &Config,
    reg: &Registry,
    n: &dagon_topology::schema::Node,
    hex: &str,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<nodebuild::SlotElfs, String> {
    use dagon_format::header::{image_kind, slot_binding};
    let slots: Vec<(char, Vec<String>)> = [(slot_binding::A, 'a'), (slot_binding::B, 'b')]
        .into_iter()
        .filter(|(b, _)| {
            dagon_sign::image::slot_with(reg, &n.mcu, image_kind::BOOTCORE, *b).is_some()
        })
        .map(|(b, c)| {
            let features = if b == slot_binding::B {
                vec![SLOT_B_FEATURE.to_string()]
            } else {
                Vec::new()
            };
            (c, features)
        })
        .collect();
    if slots.is_empty() {
        return Ok(Vec::new());
    }
    let dir = boot_crate(cfg, n)?;
    let mut out = Vec::new();
    for (slot, features) in slots {
        log(&format!("{}: building the boot core for slot {slot}", n.id));
        let target = cfg
            .base
            .join("out/deploy/target")
            .join(format!("{}-boot-{slot}", n.id));
        let exe = nodebuild::cargo_build(
            &dir,
            BOOT_BIN,
            &features,
            Some(&target),
            nodebuild::FirmwareEnv {
                pubkey_hex: Some(hex),
                target_tag: Some(&n.tag),
                boot_crate: Some(&dir),
                link_baud: None,
            },
            log,
            cancel,
        )?;
        let elf = std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))?;
        out.push((slot, elf));
    }
    Ok(out)
}

/// Node `n`'s boot crate, `boot/<id>/` of the project (`dagon gen` writes it).
fn boot_crate(cfg: &Config, n: &dagon_topology::schema::Node) -> Result<PathBuf, String> {
    let dir = cfg.base.join(crate::extgen::BOOT_CRATES).join(&n.id);
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "{}: no boot crate in {}; run `dagon gen` first",
            n.id,
            dir.display()
        ));
    }
    Ok(dir)
}

/// The boot folder part `part` (its package in `dir`) is built for:
/// `boot` for a boot stage (stage0, or a boot core library), `None` for
/// any other part, so a part that is not a boot stage never reads a
/// node's boot crate.
fn boot_folder_of<'a>(
    part: &Part,
    dir: &Path,
    boot: Option<&'a Path>,
) -> Result<Option<&'a Path>, String> {
    let stage = part.name == STAGE0 || boot_library(dir)?;
    Ok(boot.filter(|_| stage))
}

/// Whether the package in `dir` is a boot core library (it declares
/// `[package.metadata.dagon] bootcore = "<mcu>"`, as `dagon gen` looks for).
fn boot_library(dir: &Path) -> Result<bool, String> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(v.get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("dagon"))
        .and_then(|d| d.get("bootcore"))
        .is_some())
}

/// The binary a node's boot crate builds (its template's `[[bin]]`).
const BOOT_BIN: &str = "boot";
/// The feature that links a boot crate for boot core slot B.
const SLOT_B_FEATURE: &str = "slot-b";

/// Builds the parts and the app and writes the first-install files into
/// `out`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    cfg: &Config,
    t: &Topology,
    node: &str,
    key: &KeyPair,
    out: &Path,
    paths: Paths,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<FirstWrite, String> {
    let reg = crate::defs::registry(cfg)?;
    let i = inputs(cfg, &reg, t, node, key, paths, log, cancel)?;
    let mcu = node_mcu(&reg, t, node)?;
    match Esp::of(mcu) {
        Some(esp) => {
            let app_out = crate::genfs::Out::of(&cfg.base, out)?.join("app.bin")?;
            let app_bin = crate::genfs::check_out(&cfg.base, &app_out)?;
            let app = crate::esp::save_image(&esp, &i.app, &app_bin)?;
            esp_files(
                &reg,
                t,
                node,
                &i,
                &app,
                &source_root(cfg)?,
                out,
                key,
                &cfg.base,
            )
        }
        None => provision_files(&reg, t, node, &i, out, key, &cfg.base),
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
    base: &Path,
) -> Result<FirstWrite, String> {
    use crate::esp;
    use dagon_format::header::{image_kind, slot_binding, HEADER_SIZE};
    let mcu = node_mcu(reg, t, node)?;
    let e = Esp::of(mcu).ok_or("not an espflash MCU")?;
    let pkg = match &mcu.package {
        Some(p) => p.clone(),
        None => package_dir(home, &Want::Mcu(mcu.id.clone()))?,
    };
    let (bl_path, sums, csv_path) = esp::vendor_files(mcu, &e, &pkg)?;
    let boot = esp::checked_bootloader(&bl_path, &sums)?;
    let csv =
        std::fs::read_to_string(&csv_path).map_err(|e| format!("{}: {e}", csv_path.display()))?;
    let l = esp::layout(mcu)?;
    let version = crate::commands::version(i.version.as_deref().ok_or("no app version")?)?;
    let opts = crate::commands::SignOpts {
        version,
        release_seq: i.release_seq.unwrap_or(1),
        min_bootcore: dagon_format::Version::default(),
        proto: i.proto_version.unwrap_or(1),
        bits: None,
    };
    let img = crate::commands::image_for_node(
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
    let dest = crate::genfs::Out::of(base, out)?;
    let file = |name: &str, bytes: &[u8]| -> Result<PathBuf, String> {
        crate::genfs::write_out(base, &dest.join(name)?, bytes)?;
        Ok(out.join(name))
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
    crate::genfs::write_out(
        base,
        &dest.join("commands.txt")?,
        command_lines("espflash", &commands).as_bytes(),
    )?;
    Ok(FirstWrite::Espflash { files, commands })
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

// Copies the UF2 onto the device's boot drive.
#[allow(clippy::disallowed_methods)]
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

/// `dagon flash <id>`: writes to the device `--probe`, `--port` or
/// `--drive` names (the one the node's writer takes), else the one found.
pub fn command(a: &FlashArgs) -> Result<(), String> {
    let dry = a.dry_run;
    let yes = a.yes;
    let node = a.node.as_str();
    let cfg = Config::load(a.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(a.topology.as_deref())?)?;
    let out = a
        .out
        .as_ref()
        .map_or_else(|| cfg.provision_dir().join(node), PathBuf::from);
    let steps = plan(&cfg, &t, node, &out, Paths::Full)?.join("\n");
    println!("{steps}");
    let selector = selector(a, &cfg, &t, node)?;
    let key = load_key(a.key.as_deref())?;
    let mut log = |l: &str| println!("{l}");
    let cancel = AtomicBool::new(false);
    let w = prepare(&cfg, &t, node, &key, &out, Paths::Full, &mut log, &cancel)?;
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
    a: &'a FlashArgs,
    cfg: &Config,
    t: &Topology,
    node: &str,
) -> Result<Option<&'a str>, String> {
    let given: Vec<(&str, &str)> = [("probe", &a.probe), ("port", &a.port), ("drive", &a.drive)]
        .into_iter()
        .filter_map(|(f, v)| v.as_deref().map(|v| (f, v)))
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
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::config::BuildInputs;
    use ed25519_compact::Seed;

    const YAML: &str = include_str!("../../../topology/sample.yaml");

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
                },
            );
        }
        (cfg, dagon_topology::parse_str(YAML).unwrap())
    }

    #[test]
    fn a_first_write_signs_with_the_versions_in_the_crates() {
        let base =
            std::env::temp_dir().join(format!("dagon-first-versions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let manifest = |rel: &str, v: &str| {
            let dir = base.join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"x\"\nversion = \"{v}\"\n"),
            )
            .unwrap();
        };
        manifest("nodes/seg", "0.1.0");
        manifest("boot/seg", "0.2.0");
        manifest("nodes/watch", "0.3.0");
        let mut cfg = Config {
            base: base.clone(),
            ..Config::default()
        };
        for id in ["seg", "watch"] {
            cfg.build.insert(
                id.into(),
                crate::config::BuildInputs {
                    package: PathBuf::from(format!("nodes/{id}")),
                    bin: None,
                    features: vec![],
                    slot_b_feature: None,
                },
            );
        }
        assert_eq!(
            first_versions(&cfg, "seg").unwrap(),
            ("0.1.0".to_string(), Some("0.2.0".to_string()))
        );
        assert_eq!(
            first_versions(&cfg, "watch").unwrap(),
            ("0.3.0".to_string(), None)
        );
        assert!(first_versions(&cfg, "nope").is_err());
        let _ = std::fs::remove_dir_all(&base);
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
            version: Some("0.1.0".into()),
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
        let plan = plan(&cfg, &t, "hub", &out, Paths::Full).unwrap().join("\n");
        assert!(
            plan.contains("rescue.uf2") && plan.contains("RPI-RP2"),
            "{plan}"
        );
        assert!(
            plan.contains("stage0") && plan.contains("targets/rp2040-hub"),
            "{plan}"
        );
        assert!(plan.contains("(tag hub-rp2040-lcd)"), "{plan}");
        // The page's plan: the same steps with no path of this machine.
        let page = super::plan(&cfg, &t, "hub", &out, Paths::Relative)
            .unwrap()
            .join(
                "
",
            );
        assert_eq!(page.lines().count(), plan.lines().count());
        assert!(page.contains("  make     out/hub/rescue.uf2"), "{page}");
        assert!(page.contains("/stage0 of the DAGON source"), "{page}");
        for local in [cfg.base.display().to_string(), here().display().to_string()] {
            assert!(!page.contains(&local), "{local} in {page}");
        }
        match provision_files(
            Registry::builtin(),
            &t,
            "hub",
            &built(&cfg, true),
            &out,
            &key(),
            &cfg.base,
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
        let plan = plan(&cfg, &t, "seg", &out, Paths::Full).unwrap().join("\n");
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
            &built(&cfg, true),
            &out,
            &key(),
            &cfg.base,
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
    fn provision_files_go_to_a_provision_folder_outside_the_work_area() {
        let (mut cfg, t) = area("outside");
        let shared = cfg
            .base
            .with_file_name(format!("dagon-flash-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&shared);
        // As Config::load leaves `provision_dir = "../dagon-flash-shared-<pid>"`.
        cfg.provision_dir = Some(crate::config::lexical(
            &cfg.base.join("..").join(shared.file_name().unwrap()),
        ));
        let out = cfg.provision_dir().join("seg");
        provision_files(
            Registry::builtin(),
            &t,
            "seg",
            &built(&cfg, true),
            &out,
            &key(),
            &cfg.base,
        )
        .unwrap();
        assert!(shared.join("seg/flash.bin").is_file());
        assert!(shared.join("seg/commands.txt").is_file());
        let _ = std::fs::remove_dir_all(&shared);
        let _ = std::fs::remove_dir_all(&cfg.base);
    }

    #[test]
    fn prebuilt_provision_files_are_never_used() {
        let (mut cfg, t) = area("prov");
        let prebuilt = built(&cfg, false);
        cfg.provision.insert("seg".into(), prebuilt.clone());
        let plan = plan(&cfg, &t, "seg", &cfg.base.join("out"), Paths::Full)
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
        assert!(plan(&cfg, &t, "nope", &out, Paths::Full).is_err());
        cfg.build.clear();
        assert!(plan(&cfg, &t, "seg", &out, Paths::Full)
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
        let plan = plan(&cfg, &t, "hub", &cfg.base.join("out"), Paths::Full)
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

    /// Only the boot stages (stage0 and a boot core library) are built for
    /// the node's boot folder (`DAGON_BOOT_CRATE`); every other first-write
    /// part (the ESP32-S3 factory agent, a board's own part) is built
    /// without it.
    #[test]
    fn only_boot_stages_get_the_boot_folder() {
        let reg = &crate::defs::registry_at(&Config::default(), Some(&here())).unwrap();
        let mut seen = Vec::new();
        for mcu in reg.mcus().iter().map(|m| m.id.as_str()) {
            let board = reg.default_board(mcu).map(|b| b.id.as_str());
            for p in parts(reg, mcu, board) {
                let dir = part_dir(&here(), &p).unwrap();
                let folder = boot_folder_of(&p, &dir, Some(Path::new("boot/n"))).unwrap();
                seen.push((mcu.to_string(), p.name.clone(), folder.is_some()));
            }
        }
        let boot_stage = |name: &str| ["stage0", "bootcore"].contains(&name);
        assert!(seen.iter().any(|(_, n, _)| !boot_stage(n)), "{seen:?}");
        for (mcu, name, has) in &seen {
            assert_eq!(*has, boot_stage(name), "{mcu} {name}");
        }
        let app = Part {
            name: "app".into(),
            owner: Want::Mcu("rp2040".into()),
            sub: String::new(),
            bin: None,
            package: None,
        };
        let dir = here().join("targets/mcu/rp2040");
        assert_eq!(
            boot_folder_of(&app, &dir, Some(Path::new("boot/n"))).unwrap(),
            None
        );
    }

    #[test]
    fn a_boot_core_library_part_builds_from_the_node_boot_crate() {
        let reg = &crate::defs::registry_at(&Config::default(), Some(&here())).unwrap();
        for (mcu, lib) in [("rp2040", [false, true]), ("stm32f401re", [false, true])] {
            let found: Vec<bool> = parts(reg, mcu, None)
                .iter()
                .map(|p| boot_library(&part_dir(&here(), p).unwrap()).unwrap())
                .collect();
            assert_eq!(
                found, lib,
                "{mcu}: stage0 builds itself, the boot core is a library"
            );
        }
        let (cfg, t) = area("seg-boot");
        let err = boot_crate(&cfg, t.node("seg").unwrap()).unwrap_err();
        assert!(err.contains("run `dagon gen` first"), "{err}");
        let _ = std::fs::remove_dir_all(&cfg.base);
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
            },
        );
        let out = cfg.base.join("out/watch");
        let plan = plan(&cfg, &t, "watch", &out, Paths::Full)
            .unwrap()
            .join("\n");
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
        let w = esp_files(reg, &t, "watch", &i, &app, &here(), &out, &key(), &here()).unwrap();
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
        let plan = plan(&cfg, &t, "seg", &out, Paths::Full).unwrap().join("\n");
        assert!(plan.contains("flash seg (fake-m0):"), "{plan}");
        // The part is in the project's MCU package, not in a DAGON source.
        assert!(
            plan.contains(&format!(
                "build    bootcore {} (tag",
                pkg.join("bootcore").display()
            )),
            "{plan}"
        );
        assert!(plan.contains("through the SWD"), "{plan}");

        let reg = crate::defs::registry(&cfg).unwrap();
        let FirstWrite::ProbeRs { bin, commands, .. } = provision_files(
            &reg,
            &t,
            "seg",
            &built(&cfg, false),
            &out,
            &key(),
            &cfg.base,
        )
        .unwrap() else {
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
    fn a_folder_inside_another_is_named_before_it() {
        let (mut cfg, _) = area("nested");
        let template = cfg.build["seg"].clone();
        cfg.build.clear();
        for (node, package) in [("a", "private"), ("z", "private/token/app")] {
            let dir = cfg.base.join(package);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"app-{node}\"\nversion = \"0.0.0\"\n"),
            )
            .unwrap();
            let mut b = template.clone();
            b.package = PathBuf::from(package);
            cfg.build.insert(node.into(), b);
        }
        let swaps = page_swaps(&cfg, Registry::builtin());
        let lens: Vec<usize> = swaps.iter().map(|(f, _)| f.len()).collect();
        assert!(lens.windows(2).all(|w| w[0] >= w[1]), "{swaps:?}");
        let inner = cfg.base.join("private/token/app/src/main.rs");
        let outer = cfg.base.join("private/lib.rs");
        let line = format!("{} and {}", inner.display(), outer.display());
        let shown = hidden(&line, &swaps);
        assert!(shown.contains("app-z (the z app)"), "{shown}");
        assert!(shown.contains("app-a (the a app)"), "{shown}");
        assert!(
            !shown.contains("token") && !shown.contains("private"),
            "{shown}"
        );
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
