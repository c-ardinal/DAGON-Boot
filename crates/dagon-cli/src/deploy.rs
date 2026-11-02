//! `dagon deploy`: build, sign (slots A and B), release build and apply in
//! one command. The version defaults to one patch above the newest version
//! known (any node of the last release, the deployed nodes' `[build]` and
//! `[provision]` versions); before writing, the node → version list is shown and the user
//! confirms (`--yes` skips).

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use dagon_format::header::{image_kind, slot_binding};
use dagon_format::{BundleView, Version};
use dagon_topology::Topology;
use ed25519_compact::KeyPair;

use crate::commands::{image_for_node, version, SignOpts};
use crate::config::{load_key, read_topology, write_file, Config, Opts};
use crate::nodebuild::{self, SlotElfs};

/// Release `apply` takes by default when dagon.toml names none.
const DEFAULT_RELEASE: &str = "out/release-latest.dgb";
/// Signed images and versioned releases of each deploy.
const DEPLOY_DIR: &str = "out/deploy";
/// Version of a first deploy with nothing to count up from.
const FIRST: Version = Version::new(1, 0, 0);

/// What a deploy writes, before it is applied.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub version: Version,
    /// `out/deploy/v<version>.dgb`.
    pub release: PathBuf,
    pub bundle: Vec<u8>,
    /// (node, version in the previous release, if any), in update order.
    pub nodes: Vec<(String, Option<Version>)>,
}

impl Prepared {
    /// The list the user confirms.
    pub fn listing(&self) -> String {
        let width = self.nodes.iter().map(|n| n.0.len()).max().unwrap_or(0);
        let mut s = format!(
            "deploy {} to {} node(s) ({}):\n",
            self.version,
            self.nodes.len(),
            self.release.display()
        );
        for (id, old) in &self.nodes {
            let old = old.map_or("(none)".to_string(), |v| v.to_string());
            s += &format!("  {id:<width$}  {old} -> {}\n", self.version);
        }
        s
    }
}

pub(crate) fn release_path(cfg: &Config) -> PathBuf {
    cfg.release
        .clone()
        .unwrap_or_else(|| cfg.base.join(DEFAULT_RELEASE))
}

/// Node id → newest version in the release at `path` (none: no file).
pub(crate) fn release_versions(
    t: &Topology,
    path: &Path,
) -> Result<BTreeMap<String, Version>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let b = BundleView::parse(&bytes).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let m = b.manifest();
    let mut out = BTreeMap::new();
    for i in 0..m.entry_count() {
        let e = m.entry(i);
        if let Some(n) = t.nodes.get(e.node_index as usize) {
            let v = out.entry(n.id.clone()).or_insert(e.version);
            *v = (*v).max(e.version);
        }
    }
    Ok(out)
}

/// One patch above the newest of `known`, else [`FIRST`].
pub(crate) fn next_version(known: impl IntoIterator<Item = Version>) -> Result<Version, String> {
    match known.into_iter().max() {
        None => Ok(FIRST),
        Some(v) if v.patch() == u16::MAX => Err(format!(
            "{v}: the patch number is at its maximum; pass --version"
        )),
        Some(v) => Ok(Version::from_raw(v.raw() + 1)),
    }
}

/// Versions configured for `node` in dagon.toml.
fn configured(cfg: &Config, node: &str) -> Vec<Version> {
    let b = cfg.build.get(node).and_then(|b| b.version.as_deref());
    let p = cfg.provision.get(node).and_then(|p| p.version.as_deref());
    b.into_iter()
        .chain(p)
        .filter_map(|s| version(s).ok())
        .collect()
}

/// `pos`, else every `[build.<id>]` node, ordered as the topology updates
/// them; each must be in the topology and build both slots.
pub(crate) fn pick_nodes(cfg: &Config, t: &Topology, pos: &[&str]) -> Result<Vec<String>, String> {
    let wanted: Vec<String> = if pos.is_empty() {
        cfg.build.keys().cloned().collect()
    } else {
        pos.iter().map(|s| s.to_string()).collect()
    };
    if wanted.is_empty() {
        return Err("no [build.<id>] tables in dagon.toml".into());
    }
    for id in &wanted {
        if t.node(id).is_none() {
            return Err(format!("no node '{id}' in the topology"));
        }
        let b = cfg
            .build
            .get(id)
            .ok_or_else(|| format!("no [build.{id}] in dagon.toml"))?;
        if b.slot_b_feature.is_none() {
            return Err(format!(
                "[build.{id}] needs slot_b_feature: a release carries slot A and B images"
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
    let mut ids = wanted;
    ids.sort_by_key(|id| rank(id));
    ids.dedup();
    Ok(ids)
}

/// Signs every built slot of `elfs` as `version` and writes the release.
pub(crate) fn prepare(
    cfg: &Config,
    t: &Topology,
    key: &KeyPair,
    version: Version,
    previous: &BTreeMap<String, Version>,
    elfs: &[(String, SlotElfs)],
) -> Result<Prepared, String> {
    let dir = cfg.base.join(DEPLOY_DIR).join(version.to_string());
    let opts = SignOpts {
        version,
        release_seq: 1,
        min_bootcore: Version::default(),
        proto: 1,
        bits: None,
    };
    let mut images = Vec::new();
    for (node, slots) in elfs {
        for (slot, elf) in slots {
            let binding = if *slot == 'b' {
                slot_binding::B
            } else {
                slot_binding::A
            };
            let img = image_for_node(t, node, image_kind::APP, binding, elf, Some(&opts), key)?;
            write_file(&dir.join(format!("{node}-{slot}.img")), &img)?;
            images.push(img);
        }
    }
    let topo_bin = dagon_topology::compile(t).map_err(|e| e.to_string())?;
    let name = format!("v{version}");
    let r =
        dagon_sign::release::build(t, &topo_bin, &images, &name, key).map_err(|e| e.to_string())?;
    let release = cfg.base.join(DEPLOY_DIR).join(format!("{name}.dgb"));
    write_file(&release, &r.bundle)?;
    Ok(Prepared {
        version,
        release,
        bundle: r.bundle,
        nodes: elfs
            .iter()
            .map(|(n, _)| (n.clone(), previous.get(n).copied()))
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

/// `dagon deploy [<id>...] [--version x.y.z] [--yes] [--port <name>]
/// [--topology <yaml>] [--key <file>] [--config <file>]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let yes = args.contains(&YES);
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != YES).collect();
    let o = Opts::parse(&args, &["version", "port", "topology", "key", "config"])?;
    let cfg = Config::load(o.get("config"))?;
    let t = read_topology(&cfg.topology_path(o.get("topology"))?)?;
    let nodes = pick_nodes(&cfg, &t, &o.pos)?;
    let key = load_key(o.get("key"))?;
    let latest = release_path(&cfg);
    let previous = release_versions(&t, &latest)?;
    let version = match o.get("version") {
        Some(v) => version(v)?,
        // Every node of the last release counts, so versions keep rising
        // when only some nodes are deployed.
        None => next_version(
            previous
                .values()
                .copied()
                .chain(nodes.iter().flat_map(|n| configured(&cfg, n))),
        )?,
    };

    let fw_key = firmware_key(&cfg, &key)?;
    let mut log = |l: &str| println!("{l}");
    nodebuild::build_nodes_keyed(
        &cfg,
        &nodes,
        Some(fw_key),
        &mut log,
        &AtomicBool::new(false),
    )?;
    let elfs = nodes
        .iter()
        .map(|n| nodebuild::last_build(&cfg, n).map(|(_, e)| (n.clone(), e)))
        .collect::<Result<Vec<_>, _>>()?;
    let p = prepare(&cfg, &t, &key, version, &previous, &elfs)?;

    if yes {
        print!("{}", p.listing());
    } else if !confirm(&p.listing(), &mut std::io::stdin().lock()) {
        return Err(format!(
            "not applied; the release stays in {}",
            p.release.display()
        ));
    }
    write_file(&latest, &p.bundle)?;
    let mut host = crate::serial::host(o.get("port"))?;
    crate::device::apply_bundle(&mut host, &p.bundle, &t, Some(&key.pk), None)
}

#[cfg(test)]
mod tests;
