//! Kept firmware: every package written to nodes through DAGON is kept in
//! the project's `out/deploy/` (the folder `dagon deploy` makes its
//! packages in), so any past version can be written back.
//!
//! A kept package is `out/deploy/<id>.dgb`. Its id comes from the version
//! it gives each node: `v<version>` when every node gets the same one, else
//! `v<newest>-<8 hex>` (a digest of the node → version list). The same
//! versions give the same id: writing them again replaces the file.

use std::collections::BTreeMap;

use dagon_format::{BundleView, Version};
use dagon_topology::Topology;

use crate::config::Config;

/// The kept packages' folder in the project.
pub(crate) const DIR: &str = "out/deploy";

/// One kept package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub id: String,
    /// Node id → the version the package gives it.
    pub versions: BTreeMap<String, Version>,
    pub bytes: usize,
}

/// The id of a package giving `versions`.
pub(crate) fn id_of(versions: &BTreeMap<String, Version>) -> String {
    let newest = versions.values().max().copied().unwrap_or_default();
    if versions.values().all(|v| *v == newest) {
        return format!("v{newest}");
    }
    let list: String = versions
        .iter()
        .map(|(n, v)| {
            format!(
                "{n}={v}
"
            )
        })
        .collect();
    let digest = dagon_format::sha256(list.as_bytes());
    format!("v{newest}-{}", dagon_sign::to_hex(&digest[..4]))
}

/// Node id → the newest app version `bundle` gives it, once it verifies under
/// `pubkey`.
pub(crate) fn versions_of(
    t: &Topology,
    bundle: &[u8],
    pubkey: &[u8; 32],
) -> Result<BTreeMap<String, Version>, String> {
    crate::versions::check_number("bundle", "the package", bundle)?;
    let b = BundleView::parse(bundle).map_err(|e| format!("not a package: {e:?}"))?;
    b.verify(pubkey)
        .map_err(|e| format!("the package does not verify under the project's key: {e:?}"))?;
    let m = b.manifest();
    let mut out = BTreeMap::new();
    for i in 0..m.entry_count() {
        let e = m.entry(i);
        // A node's version is its app's; its boot core has its own.
        if e.image_kind != dagon_format::header::image_kind::APP {
            continue;
        }
        if let Some(n) = t.nodes.get(e.node_index as usize) {
            let v = out.entry(n.id.clone()).or_insert(e.version);
            *v = (*v).max(e.version);
        }
    }
    Ok(out)
}

/// Verifies `bundle` under `pubkey` and keeps it; returns its id.
pub(crate) fn store(
    cfg: &Config,
    t: &Topology,
    pubkey: &[u8; 32],
    bundle: &[u8],
) -> Result<String, String> {
    let versions = versions_of(t, bundle, pubkey)?;
    if versions.is_empty() {
        return Err("the package gives no node of the configuration a version".into());
    }
    let id = id_of(&versions);
    let rel = crate::genfs::Rel::new(format!("{DIR}/{id}.dgb"))?;
    crate::genfs::write(&cfg.base, &rel, bundle)?;
    Ok(id)
}

/// The kept packages that verify under `pubkey`, newest version first.
pub(crate) fn list(cfg: &Config, t: &Topology, pubkey: &[u8; 32]) -> Vec<Kept> {
    let Ok(dir) = crate::paths::contained(&cfg.base, &cfg.base.join(DIR)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Kept> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".dgb")?.to_string();
            let bytes = read(cfg, pubkey, &id).ok()?;
            let versions = versions_of(t, &bytes, pubkey).ok()?;
            Some(Kept {
                id,
                versions,
                bytes: bytes.len(),
            })
        })
        .collect();
    let newest = |k: &Kept| k.versions.values().max().copied().unwrap_or_default();
    out.sort_by(|a, b| newest(b).cmp(&newest(a)).then_with(|| b.id.cmp(&a.id)));
    out
}

/// The bytes of kept package `id`, once they verify under `pubkey`.
pub(crate) fn read(cfg: &Config, pubkey: &[u8; 32], id: &str) -> Result<Vec<u8>, String> {
    if !crate::serve::plain_id(id) {
        return Err("bad package id".into());
    }
    let path = crate::paths::contained(&cfg.base, &cfg.base.join(DIR).join(format!("{id}.dgb")))?;
    let bytes = std::fs::read(&path).map_err(|_| format!("no kept package {id}"))?;
    BundleView::parse(&bytes)
        .and_then(|b| b.verify(pubkey))
        .map_err(|_| format!("{id} does not verify under the project's key"))?;
    Ok(bytes)
}

/// The most bytes a package can take: every image of every node in the
/// largest slots `reg` defines, and the most the format holds around them.
pub(crate) fn largest_package(reg: &dagon_topology::defs::Registry) -> usize {
    use dagon_format::{bundle, header::HEADER_SIZE, manifest, topology};
    let node = reg
        .mcus()
        .iter()
        .map(|m| {
            let images = m.slots.iter();
            images.map(|s| HEADER_SIZE + s.max_payload as usize).sum()
        })
        .max()
        .unwrap_or(0);
    let around = bundle::head_size(bundle::MAX_ITEMS)
        + manifest::total_size(manifest::MAX_ENTRIES, manifest::MAX_CONSTRAINTS)
        + topology::MAX_SIZE
        // Items are 4-byte aligned.
        + 3 * (bundle::MAX_ITEMS + 1);
    topology::MAX_NODES * node + around
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    const V100: Version = Version::new(1, 0, 0);

    /// The bound holds a real package's overhead, and the largest node at the limit.
    #[test]
    fn the_largest_package_holds_every_image_of_every_node_at_the_limit() {
        use dagon_format::topology::MAX_NODES;
        let reg = dagon_topology::defs::Registry::builtin();
        let most = largest_package(reg);
        let mut biggest = 0;
        for m in reg.mcus() {
            let images: usize = m
                .slots
                .iter()
                .map(|s| dagon_format::header::HEADER_SIZE + s.max_payload as usize)
                .sum();
            biggest = biggest.max(images);
        }
        assert!(most >= MAX_NODES * biggest);
        // What a real package adds around its images fits what the bound leaves.
        let (dir, _, _, key) = area("bound");
        let pkg = crate::demo::package_with(&key, V100, V100).unwrap();
        let view = dagon_format::bundle::BundleView::parse(&pkg).unwrap();
        let images: usize = (0..view.item_count())
            .map(|i| view.item(i).length as usize)
            .sum();
        assert!(pkg.len() - images <= most - MAX_NODES * biggest);
        let _ = std::fs::remove_dir_all(&dir);
    }
    const V110: Version = Version::new(1, 1, 0);

    fn area(
        name: &str,
    ) -> (
        std::path::PathBuf,
        Config,
        Topology,
        ed25519_compact::KeyPair,
    ) {
        let dir = std::env::temp_dir().join(format!("dagon-kept-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml = crate::demo::yaml(crate::serve::Medium::UsbCdc);
        std::fs::write(dir.join("t.yaml"), &yaml).unwrap();
        std::fs::write(dir.join("dagon.toml"), "topology = \"t.yaml\"\n").unwrap();
        let cfg = Config::load(Some(&dir.join("dagon.toml").to_string_lossy())).unwrap();
        let t = dagon_topology::parse_str(&yaml).unwrap();
        let seed = [9u8; 32];
        let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
        (dir, cfg, t, key)
    }

    fn versions(v: &[(&str, Version)]) -> BTreeMap<String, Version> {
        v.iter().map(|(n, v)| (n.to_string(), *v)).collect()
    }

    #[test]
    fn one_version_for_every_node_names_the_package_by_it() {
        let all = versions(&[("hub", V110), ("seg", V110), ("watch", V110)]);
        assert_eq!(id_of(&all), "v1.1.0");
    }

    #[test]
    fn mixed_versions_name_the_newest_and_a_digest_of_the_list() {
        let a = versions(&[("hub", V110), ("seg", V100)]);
        let b = versions(&[("hub", V110), ("seg", Version::new(1, 0, 1))]);
        let ia = id_of(&a);
        assert!(
            ia.starts_with("v1.1.0-") && ia.len() == "v1.1.0-".len() + 8,
            "{ia}"
        );
        assert_ne!(ia, id_of(&b), "different lists keep apart");
        assert_eq!(ia, id_of(&a.clone()), "the same list keeps one file");
        assert!(crate::serve::plain_id(&ia));
    }

    #[test]
    fn a_written_package_is_kept_listed_and_read_back() {
        let (dir, cfg, t, key) = area("store");
        let old = crate::demo::package(&key, V100).unwrap();
        let new = crate::demo::package(&key, V110).unwrap();
        assert_eq!(store(&cfg, &t, &key.pk, &old).unwrap(), "v1.0.0");
        assert_eq!(store(&cfg, &t, &key.pk, &new).unwrap(), "v1.1.0");
        assert!(dir.join("out/deploy/v1.0.0.dgb").is_file());
        let l = list(&cfg, &t, &key.pk);
        assert_eq!(
            l.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(),
            ["v1.1.0", "v1.0.0"],
            "newest first"
        );
        assert_eq!(
            l[1].versions,
            versions(&[("hub", V100), ("seg", V100), ("watch", V100)])
        );
        assert_eq!(l[1].bytes, old.len());
        assert_eq!(read(&cfg, &key.pk, "v1.0.0").unwrap(), old);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_is_named_and_listed_by_its_app_versions_not_its_boot_cores() {
        let (dir, cfg, t, key) = area("core");
        let pkg = crate::demo::package_with(&key, V100, Version::new(2, 0, 0)).unwrap();
        assert_eq!(store(&cfg, &t, &key.pk, &pkg).unwrap(), "v1.0.0");
        let l = list(&cfg, &t, &key.pk);
        assert_eq!(l[0].versions["seg"], V100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_packages_of_this_key_are_kept_listed_or_read() {
        let (dir, cfg, t, key) = area("foreign");
        let other = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([3; 32]));
        let foreign = crate::demo::package(&other, V110).unwrap();
        assert!(store(&cfg, &t, &key.pk, &foreign).is_err());
        assert!(store(&cfg, &t, &key.pk, b"not a package").is_err());
        std::fs::create_dir_all(dir.join(DIR)).unwrap();
        std::fs::write(dir.join(DIR).join("v1.1.0.dgb"), &foreign).unwrap();
        assert!(list(&cfg, &t, &key.pk).is_empty());
        assert!(read(&cfg, &key.pk, "v1.1.0").is_err());
        assert!(read(&cfg, &key.pk, "../t").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
