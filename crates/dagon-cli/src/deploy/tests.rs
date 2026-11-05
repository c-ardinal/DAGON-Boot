use std::path::PathBuf;

use dagon_format::header::{image_kind, slot_binding};
use dagon_format::BundleView;
use dagon_host::{Host, Target};
use dagon_sim::{block_on, demo_hub, NODE_I2C_ADDR};
use ed25519_compact::{KeyPair, Seed};

use super::*;
use crate::config::BuildInputs;
use crate::simrun::Sim;

const YAML: &str = include_str!("../../../../topology/sim-pair.yaml");
const OLD: Version = Version::new(1, 0, 0);

fn key() -> KeyPair {
    // Not the RFC 8032 test key, which load_key would refuse.
    KeyPair::from_seed(Seed::new([7; 32]))
}

fn work_area(name: &str) -> (Config, Topology) {
    let base = std::env::temp_dir().join(format!("dagon-deploy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let mut cfg = Config {
        base,
        ..Config::default()
    };
    for (id, v) in [("hub", "1.2.0"), ("seg", "1.0.1")] {
        std::fs::create_dir_all(cfg.base.join(id)).unwrap();
        std::fs::write(
            cfg.base.join(id).join("Cargo.toml"),
            format!("[package]\nname = \"{id}\"\nversion = \"{v}\"\n"),
        )
        .unwrap();
        cfg.build.insert(
            id.into(),
            BuildInputs {
                package: PathBuf::from(id),
                bin: None,
                features: vec![],
                slot_b_feature: Some("slot-b".into()),
                version: None,
            },
        );
    }
    (cfg, dagon_topology::parse_str(YAML).unwrap())
}

/// Stand-in for the build: raw payloads per node and slot.
fn built(nodes: &[String]) -> Vec<(String, SlotElfs)> {
    nodes
        .iter()
        .map(|n| {
            let len = if n == "hub" { 8000 } else { 6000 };
            let slots = ['a', 'b']
                .map(|s| (s, (0..len).map(|i| (i * 3 + s as usize) as u8).collect()))
                .to_vec();
            (n.clone(), slots)
        })
        .collect()
}

/// The simulated hub and F401, both confirmed on `OLD` signed with `key`.
fn sample_sim(t: &Topology, key: &KeyPair) -> Sim {
    let opts = SignOpts {
        version: OLD,
        release_seq: 1,
        min_bootcore: Version::default(),
        proto: 1,
        bits: None,
    };
    let img = |node: &str| {
        let payload = &built(&[node.to_string()])[0].1[0].1;
        image_for_node(
            dagon_topology::defs::Registry::builtin(),
            t,
            node,
            image_kind::APP,
            slot_binding::A,
            payload,
            Some(&opts),
            key,
        )
        .unwrap()
    };
    let mut hub = demo_hub(*key.pk, 3);
    hub.node.provision(&img("hub"), 0).unwrap();
    hub.i2c_node(NODE_I2C_ADDR)
        .unwrap()
        .provision(&img("seg"), 0)
        .unwrap();
    Sim::new(hub)
}

/// `built` with each node's version from its crate.
fn versioned(cfg: &Config, elfs: Vec<(String, SlotElfs)>) -> Vec<NodeBuild> {
    elfs.into_iter()
        .map(|(n, e)| (n.clone(), app_version(cfg, &n, None).unwrap(), e))
        .collect()
}

#[test]
fn deploys_each_node_with_its_crate_version_through_the_simulated_hub() {
    let (cfg, t) = work_area("sim");
    let key = key();
    let nodes = pick_nodes(&cfg, &t, &[]).unwrap();
    assert_eq!(nodes, ["seg", "hub"], "the topology's update order");

    let previous = release_versions(&t, &release_path(&cfg), &key.pk).unwrap();
    assert!(previous.is_empty(), "no release yet");
    let apps = versioned(&cfg, built(&nodes));
    let p = prepare(&cfg, &t, &key, &previous, (&apps, &[])).unwrap();
    let listing = p.listing();
    assert!(listing.contains("deploy to 2 node(s)"), "{listing}");
    assert!(listing.contains("seg  (none) -> 1.0.1"), "{listing}");
    assert!(listing.contains("hub  (none) -> 1.2.0"), "{listing}");
    for f in [
        "1.0.1/seg-a.img",
        "1.0.1/seg-b.img",
        "1.2.0/hub-a.img",
        "1.2.0/hub-b.img",
    ] {
        assert!(cfg.base.join("out/deploy").join(f).exists(), "{f}");
    }
    // The package is kept under the kept-store name for its versions.
    assert!(p.release.starts_with(cfg.base.join(crate::kept::DIR)));
    let name = p
        .release
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("v1.2.0-"), "{name}");
    assert_eq!(std::fs::read(&p.release).unwrap(), p.bundle);

    let sim = sample_sim(&t, &key);
    let mut host = Host::new(sim.clone(), sim.clone());
    crate::device::apply_bundle(
        &mut host,
        dagon_topology::defs::Registry::builtin(),
        &p.bundle,
        &t,
        Some(&key.pk),
        None,
    )
    .unwrap();
    for (to, v) in [
        (Target::Hub, Version::new(1, 2, 0)),
        (Target::I2c(NODE_I2C_ADDR), Version::new(1, 0, 1)),
    ] {
        let h = block_on(host.hello(to)).unwrap();
        assert_eq!((h.active_version, h.trial), (v, false), "{to:?}");
    }
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn the_plan_names_the_package_and_each_node_s_old_and_new_version() {
    // The listing `dagon deploy` prints before it asks: tools that answer
    // its question read this exact form.
    let (cfg, t) = work_area("plan");
    for id in ["hub", "seg"] {
        std::fs::write(
            cfg.base.join(id).join("Cargo.toml"),
            format!("[package]\nname = \"{id}\"\nversion = \"2.0.7\"\n"),
        )
        .unwrap();
    }
    let nodes = pick_nodes(&cfg, &t, &[]).unwrap();
    let apps = versioned(&cfg, built(&nodes));
    let p = prepare(&cfg, &t, &key(), &BTreeMap::new(), (&apps, &[])).unwrap();
    assert_eq!(p.release, cfg.base.join("out/deploy/v2.0.7.dgb"));
    let shown = p
        .listing()
        .replace(&p.release.display().to_string(), "<release>");
    println!("{shown}");
    assert_eq!(
        shown,
        "deploy to 2 node(s) (<release>):\n  seg  (none) -> 2.0.7\n  hub  (none) -> 2.0.7\n"
    );
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn a_version_given_must_be_the_crate_s_own() {
    let (mut cfg, _) = work_area("given");
    assert_eq!(
        app_version(&cfg, "seg", None).unwrap(),
        Version::new(1, 0, 1)
    );
    assert_eq!(
        app_version(&cfg, "seg", Some("1.0.1")).unwrap(),
        Version::new(1, 0, 1)
    );
    let e = app_version(&cfg, "seg", Some("1.1.0")).unwrap_err();
    assert!(
        e.contains("1.1.0") && e.contains("1.0.1") && e.contains("Cargo.toml"),
        "{e}"
    );
    // A version in [build.<id>] is held to the same rule.
    cfg.build.get_mut("seg").unwrap().version = Some("2.0.0".into());
    assert!(app_version(&cfg, "seg", None)
        .unwrap_err()
        .contains("2.0.0"));
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn a_release_signed_with_another_key_is_not_applied() {
    let (cfg, t) = work_area("key");
    let key = key();
    let nodes = pick_nodes(&cfg, &t, &["seg"]).unwrap();
    let apps = versioned(&cfg, built(&nodes));
    let p = prepare(&cfg, &t, &key, &BTreeMap::new(), (&apps, &[])).unwrap();
    let other = KeyPair::from_seed(Seed::new([8; 32]));
    let sim = sample_sim(&t, &other);
    let mut host = Host::new(sim.clone(), sim);
    assert!(crate::device::apply_bundle(
        &mut host,
        dagon_topology::defs::Registry::builtin(),
        &p.bundle,
        &t,
        Some(&other.pk),
        None
    )
    .is_err());
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn picks_only_nodes_that_build_both_slots() {
    let (mut cfg, t) = work_area("pick");
    assert_eq!(pick_nodes(&cfg, &t, &["hub"]).unwrap(), ["hub"]);
    assert!(pick_nodes(&cfg, &t, &["nope"]).is_err());
    cfg.build.get_mut("seg").unwrap().slot_b_feature = None;
    assert!(pick_nodes(&cfg, &t, &[])
        .unwrap_err()
        .contains("slot_b_feature"));
    cfg.build.clear();
    assert!(pick_nodes(&cfg, &t, &[]).is_err());
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn firmware_embeds_the_signing_key_and_refuses_another_build_key() {
    let (mut cfg, _) = work_area("fwkey");
    let key = key();
    let other = KeyPair::from_seed(Seed::new([8; 32]));
    let pubfile = cfg.base.join("other.pub");
    std::fs::write(&pubfile, dagon_sign::to_hex(&*other.pk)).unwrap();
    cfg.pubkey = Some(pubfile);
    assert!(firmware_key(&cfg, &key).is_err());
    std::fs::write(cfg.pubkey.as_ref().unwrap(), dagon_sign::to_hex(&*key.pk)).unwrap();
    assert_eq!(firmware_key(&cfg, &key).unwrap(), *key.pk);
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn a_generated_sample_project_deploys_every_node_including_the_esp32s3() {
    let (mut cfg, _) = work_area("sample");
    cfg.build.clear();
    let yaml = include_str!("../../../../topology/sample.yaml");
    let t = dagon_topology::parse_str(yaml).unwrap();
    let toml = cfg.base.join("dagon.toml");
    std::fs::write(&toml, "").unwrap();
    crate::scaffold::add_build_tables(dagon_topology::defs::Registry::builtin(), &toml, &t)
        .unwrap();
    let gen = Config::load(Some(&toml.to_string_lossy())).unwrap();
    assert!(gen.build["watch"].slot_b_feature.is_none(), "one ESP build");
    assert!(gen.build["seg"].slot_b_feature.is_some());

    assert_eq!(pick_nodes(&gen, &t, &[]).unwrap(), ["seg", "watch", "hub"]);
    assert_eq!(pick_nodes(&gen, &t, &["watch"]).unwrap(), ["watch"]);

    // The ESP32-S3 node's one image (either slot) goes into the release
    // next to the others' A and B images.
    let v = Version::new(1, 0, 1);
    let mut elfs: Vec<NodeBuild> = built(&["seg".into(), "hub".into()])
        .into_iter()
        .map(|(n, e)| (n, v, e))
        .collect();
    elfs.insert(1, ("watch".into(), v, vec![('a', vec![0x5a; 5000])]));
    let p = prepare(&gen, &t, &key(), &BTreeMap::new(), (&elfs, &[])).unwrap();
    assert!(
        p.listing().contains("watch  (none) -> 1.0.1"),
        "{}",
        p.listing()
    );
    assert!(BundleView::parse(&p.bundle)
        .unwrap()
        .verify(&key().pk)
        .is_ok());
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn only_yes_continues() {
    let ask = |input: &str| confirm("", &mut input.as_bytes());
    assert!(ask("y\n") && ask("YES\n") && ask(" yes "));
    assert!(!ask("") && !ask("\n") && !ask("n\n") && !ask("yess\n"));
}
