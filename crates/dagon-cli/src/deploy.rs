//! `dagon deploy`: build, sign (slots A and B), release build and apply in
//! one command. Each node's version is its crate's Cargo.toml `version`
//! (`--version`, if given, must be the same); before writing, the node →
//! version list is shown and the user confirms (`--yes` skips). The package
//! is kept in `out/deploy` (see [`crate::kept`]).

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use dagon_format::header::{image_kind, slot_binding};
use dagon_format::Version;
use dagon_topology::defs::Registry;
use dagon_topology::Topology;
use ed25519_compact::KeyPair;

use crate::commands::{image_for_node, SignOpts};
use crate::config::{load_key, read_topology, Config, Opts};
use crate::nodebuild::{self, SlotElfs};

/// Release `apply` takes by default when dagon.toml names none.
const DEFAULT_RELEASE: &str = "out/release-latest.dgb";
/// Signed images (`<version>/<node>-<slot>.img`) and the kept packages.
const DEPLOY_DIR: &str = crate::kept::DIR;

/// What a deploy writes, before it is applied.
#[derive(Debug)]
pub(crate) struct Prepared {
    /// The kept package, `out/deploy/<id>.dgb`.
    pub release: PathBuf,
    pub bundle: Vec<u8>,
    /// (node, version in the previous release if any, new version), in
    /// update order.
    pub nodes: Vec<(String, Option<Version>, Version)>,
}

impl Prepared {
    /// The list the user confirms.
    pub fn listing(&self) -> String {
        let width = self.nodes.iter().map(|n| n.0.len()).max().unwrap_or(0);
        let mut s = format!(
            "deploy to {} node(s) ({}):\n",
            self.nodes.len(),
            self.release.display()
        );
        for (id, old, new) in &self.nodes {
            let old = old.map_or("(none)".to_string(), |v| v.to_string());
            s += &format!("  {id:<width$}  {old} -> {new}\n");
        }
        s
    }
}

/// Where the applied release is kept: the project's `release` (the user's
/// path) or dagon's own default in the work area.
fn release_out(cfg: &Config) -> Result<crate::genfs::Out, String> {
    match &cfg.release {
        Some(p) => Ok(crate::genfs::Out::User(p.clone())),
        None => Ok(crate::genfs::Out::Work(crate::genfs::Rel::new(
            DEFAULT_RELEASE,
        )?)),
    }
}

pub(crate) fn release_path(cfg: &Config) -> PathBuf {
    cfg.release
        .clone()
        .unwrap_or_else(|| cfg.base.join(DEFAULT_RELEASE))
}

/// Node id → newest version in the release at `path` (none: no file),
/// read only once the release verifies under `pubkey`.
pub(crate) fn release_versions(
    t: &Topology,
    path: &Path,
    pubkey: &[u8; 32],
) -> Result<BTreeMap<String, Version>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    crate::kept::versions_of(t, &bytes, pubkey).map_err(|e| format!("{}: {e}", path.display()))
}

/// Node `node`'s app version: its crate's Cargo.toml `version`; a
/// `version` in `[build.<id>]` and `given` (`--version`) must be the same.
pub(crate) fn app_version(
    cfg: &Config,
    node: &str,
    given: Option<&str>,
) -> Result<Version, String> {
    let b = cfg
        .build
        .get(node)
        .ok_or_else(|| format!("no [build.{node}] in dagon.toml"))?;
    let what = b.package.join("Cargo.toml").display().to_string();
    let source = crate::srcver::crate_version(&cfg.base.join(&b.package))?;
    let source = crate::srcver::agreed(source, b.version.as_deref(), &what)
        .map_err(|e| format!("[build.{node}] {e}"))?;
    crate::srcver::agreed(source, given, &what)
}

/// Whether apps for `mcu` are linked separately for slot B. Not for an MCU
/// whose one image runs from either slot (ESP32-S3): it builds once.
pub(crate) fn has_slot_b_link(reg: &Registry, mcu: &str) -> bool {
    dagon_sign::image::slot_with(reg, mcu, image_kind::APP, slot_binding::B).is_some()
}

/// `pos`, else every `[build.<id>]` node, ordered as the topology updates
/// them; each must be in the topology and build both slots (one build for an
/// MCU whose image runs from either slot).
pub(crate) fn pick_nodes(cfg: &Config, t: &Topology, pos: &[&str]) -> Result<Vec<String>, String> {
    let wanted: Vec<String> = if pos.is_empty() {
        cfg.build.keys().cloned().collect()
    } else {
        pos.iter().map(|s| s.to_string()).collect()
    };
    if wanted.is_empty() {
        return Err("no [build.<id>] tables in dagon.toml".into());
    }
    let reg = crate::defs::registry(cfg)?;
    for id in &wanted {
        let n = t
            .node(id)
            .ok_or_else(|| format!("no updatable node '{id}' in the topology"))?;
        let b = cfg
            .build
            .get(id)
            .ok_or_else(|| format!("no [build.{id}] in dagon.toml"))?;
        if has_slot_b_link(&reg, &n.mcu) && b.slot_b_feature.is_none() {
            return Err(format!(
                "[build.{id}] needs slot_b_feature: a release carries slot A and B images \
                 for {}",
                n.mcu
            ));
        }
    }
    let rank = |id: &str| {
        let order = &t.update.order;
        order
            .iter()
            .position(|x| x == id)
            .unwrap_or(order.len() + t.nodes.iter().position(|n| n.id == id).unwrap_or(0))
    };
    let mut nodes = wanted;
    nodes.sort_by_key(|id| rank(id));
    nodes.dedup();
    Ok(nodes)
}

/// What each node's build turns into before signing: an ESP32-S3 app ELF
/// becomes the ESP image its OTA partitions hold (espflash save-image, kept
/// as `out/deploy/<version>/<node>.bin`); other builds are signed as built.
pub(crate) fn release_inputs(
    cfg: &Config,
    t: &Topology,
    built: Vec<NodeBuild>,
) -> Result<Vec<NodeBuild>, String> {
    let reg = crate::defs::registry(cfg)?;
    built
        .into_iter()
        .map(|(node, version, slots)| {
            let esp = t
                .node(&node)
                .and_then(|n| reg.mcu(&n.mcu))
                .and_then(crate::esp::Esp::of);
            let Some(esp) = esp else {
                return Ok((node, version, slots));
            };
            let out = cfg
                .base
                .join(DEPLOY_DIR)
                .join(version.to_string())
                .join(format!("{node}.bin"));
            let image = crate::esp::save_image(&esp, &nodebuild::elf_path(cfg, &node, 'a'), &out)?;
            Ok((node, version, vec![('a', image)]))
        })
        .collect()
}

/// One node's built slots and the version they are signed as.
pub(crate) type NodeBuild = (String, Version, SlotElfs);

/// Signs every built slot of `elfs` (apps) and `cores` (boot cores), each
/// as its own version, and writes the package where it is kept.
pub(crate) fn prepare(
    cfg: &Config,
    t: &Topology,
    key: &KeyPair,
    previous: &BTreeMap<String, Version>,
    (elfs, cores): (&[NodeBuild], &[NodeBuild]),
) -> Result<Prepared, String> {
    let reg = crate::defs::registry(cfg)?;
    let mut images = Vec::new();
    let kinds = elfs
        .iter()
        .map(|e| (image_kind::APP, "", e))
        .chain(cores.iter().map(|e| (image_kind::BOOTCORE, "bootcore-", e)));
    for (kind, prefix, (node, version, slots)) in kinds {
        let opts = SignOpts {
            version: *version,
            release_seq: 1,
            min_bootcore: Version::default(),
            proto: 1,
            bits: None,
        };
        for (slot, elf) in slots {
            let binding = if *slot == 'b' {
                slot_binding::B
            } else {
                slot_binding::A
            };
            let img = image_for_node(&reg, t, node, kind, binding, elf, Some(&opts), key)?;
            let rel = crate::genfs::Rel::new(format!(
                "{DEPLOY_DIR}/{version}/{prefix}{node}-{slot}.img"
            ))?;
            crate::genfs::write(&cfg.base, &rel, &img)?;
            images.push(img);
        }
    }
    let topo_bin = dagon_topology::compile_with(&reg, t).map_err(|e| e.to_string())?;
    let versions: BTreeMap<String, Version> =
        elfs.iter().map(|(n, v, _)| (n.clone(), *v)).collect();
    let newest = versions.values().max().copied().unwrap_or_default();
    let r = dagon_sign::release::build(&reg, t, &topo_bin, &images, &format!("v{newest}"), key)
        .map_err(|e| e.to_string())?;
    let id = crate::kept::id_of(&versions);
    let release = cfg.base.join(DEPLOY_DIR).join(format!("{id}.dgb"));
    let rel = crate::genfs::Rel::new(format!("{DEPLOY_DIR}/{id}.dgb"))?;
    crate::genfs::write(&cfg.base, &rel, &r.bundle)?;
    Ok(Prepared {
        release,
        bundle: r.bundle,
        nodes: elfs
            .iter()
            .map(|(n, v, _)| (n.clone(), previous.get(n).copied(), *v))
            .collect(),
    })
}

/// Shows `listing` and reads one line: `y` or `yes` continues.
pub(crate) fn confirm(listing: &str, input: &mut dyn BufRead) -> bool {
    print!("{listing}continue? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(n) if n > 0 => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => false,
    }
}

/// Flag of `deploy` (and `flash`): do not ask before writing.
pub(crate) const YES: &str = "--yes";
/// `dagon deploy --bootcore`: the nodes' boot cores go into the release too.
pub(crate) const BOOTCORE: &str = "--bootcore";

/// The key the firmware embeds: the build key of dagon.toml or
/// DAGON_PUBKEY ([`crate::config::build_pubkey`]), else the signing key's
/// public half. A build key other than the signing key is refused: the
/// boards would reject every image signed here.
pub(crate) fn firmware_key(cfg: &Config, key: &KeyPair) -> Result<[u8; 32], String> {
    match crate::config::build_pubkey(cfg)? {
        Some(k) if k != *key.pk => Err(
            "the build key (pubkey in dagon.toml or DAGON_PUBKEY) is not the signing key's \
             public half"
                .into(),
        ),
        _ => Ok(*key.pk),
    }
}

/// `dagon deploy [<id>...] [--version x.y.z] [--yes] [--bootcore]
/// [--port <name>] [--topology <yaml>] [--key <file>] [--config <file>]`
/// (`--bootcore`: each node's boot core too, for every boot core slot its
/// MCU definition declares; nodes without boot core slots get none).
pub fn command(args: &[&str]) -> Result<(), String> {
    let yes = args.contains(&YES);
    let with_cores = args.contains(&BOOTCORE);
    let args: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != YES && *a != BOOTCORE)
        .collect();
    let o = Opts::parse(&args, &["version", "port", "topology", "key", "config"])?;
    let cfg = Config::load(o.get("config"))?;
    let t = read_topology(&cfg, &cfg.topology_path(o.get("topology"))?)?;
    let nodes = pick_nodes(&cfg, &t, &o.pos)?;
    let key = load_key(o.get("key"))?;
    let latest = release_path(&cfg);
    let previous = release_versions(&t, &latest, &key.pk).unwrap_or_else(|e| {
        eprintln!("warning: {e}; its versions are not used");
        BTreeMap::new()
    });
    // Checked before anything is built: a version given that is not the
    // source's refuses the deploy.
    let versions = nodes
        .iter()
        .map(|n| app_version(&cfg, n, o.get("version")).map(|v| (n.clone(), v)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let fw_key = firmware_key(&cfg, &key)?;
    let mut log = |l: &str| println!("{l}");
    nodebuild::build_nodes_keyed(
        &cfg,
        &nodes,
        Some(fw_key),
        None,
        &mut log,
        &AtomicBool::new(false),
    )?;
    let elfs = nodes
        .iter()
        .map(|n| nodebuild::last_build(&cfg, n).map(|(_, e)| (n.clone(), versions[n], e)))
        .collect::<Result<Vec<_>, _>>()?;
    let elfs = release_inputs(&cfg, &t, elfs)?;
    let mut cores = Vec::new();
    if with_cores {
        let reg = crate::defs::registry(&cfg)?;
        let hex = dagon_sign::to_hex(&fw_key);
        for id in &nodes {
            let n = t
                .node(id)
                .ok_or_else(|| format!("no node '{id}' in the topology"))?;
            let built = crate::flash::build_core_slots(
                &cfg,
                &reg,
                n,
                &hex,
                &mut log,
                &AtomicBool::new(false),
            )?;
            if !built.is_empty() {
                let dir = cfg.base.join(crate::extgen::BOOT_CRATES).join(id);
                cores.push((id.clone(), crate::srcver::crate_version(&dir)?, built));
            }
        }
    }
    let p = prepare(&cfg, &t, &key, &previous, (&elfs, &cores))?;

    if yes {
        print!("{}", p.listing());
    } else if !confirm(&p.listing(), &mut std::io::stdin().lock()) {
        return Err(format!(
            "not applied; the release stays in {}",
            p.release.display()
        ));
    }
    let mut host = crate::serial::host(o.get("port"))?;
    crate::device::apply_bundle(
        &mut host,
        &crate::defs::registry(&cfg)?,
        &p.bundle,
        &t,
        Some(&key.pk),
        None,
    )?;
    // Only an applied release is the one the next deploy counts from.
    crate::genfs::write_out(&cfg.base, &release_out(&cfg)?, &p.bundle)
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests;
