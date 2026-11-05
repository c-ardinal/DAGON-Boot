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
    for id in ["hub", "seg"] {
        cfg.build.insert(
            id.into(),
            BuildInputs {
                package: PathBuf::from(id),
                bin: None,
                features: vec![],
                slot_b_feature: Some("slot-b".into()),
                version: Some("1.0.0".into()),
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
fn desk(t: &Topology, key: &KeyPair) -> Sim {
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

#[test]
fn deploys_every_built_node_one_patch_up_through_the_simulated_hub() {
    let (cfg, t) = work_area("sim");
    let key = key();
    let nodes = pick_nodes(&cfg, &t, &[]).unwrap();
    assert_eq!(nodes, ["seg", "hub"], "the topology's update order");

    let previous = release_versions(&t, &release_path(&cfg), &key.pk).unwrap();
    assert!(previous.is_empty(), "no release yet");
    let v = next_version(
        previous
            .values()
            .copied()
            .chain(nodes.iter().flat_map(|n| configured(&cfg, n))),
    )
    .unwrap();
    assert_eq!(v, Version::new(1, 0, 1), "one above the [build] version");

    let p = prepare(&cfg, &t, &key, v, &previous, (&built(&nodes), &[])).unwrap();
    let listing = p.listing();
    assert!(listing.contains("deploy 1.0.1 to 2 node(s)"), "{listing}");
    assert!(listing.contains("seg  (none) -> 1.0.1"), "{listing}");
    assert!(listing.contains("hub  (none) -> 1.0.1"), "{listing}");
    for f in ["seg-a.img", "seg-b.img", "hub-a.img", "hub-b.img"] {
        assert!(cfg.base.join("out/deploy/1.0.1").join(f).exists(), "{f}");
    }
    assert_eq!(std::fs::read(&p.release).unwrap(), p.bundle);

    let sim = desk(&t, &key);
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
    for to in [Target::Hub, Target::I2c(NODE_I2C_ADDR)] {
        let h = block_on(host.hello(to)).unwrap();
        assert_eq!((h.active_version, h.trial), (v, false), "{to:?}");
    }

    // The next deploy counts up from the release just made.
    let after = release_versions(&t, &p.release, &key.pk).unwrap();
    let other = KeyPair::from_seed(Seed::new([8; 32]));
    assert!(
        release_versions(&t, &p.release, &other.pk).is_err(),
        "a release that does not verify is not read"
    );
    assert_eq!(after.get("seg"), Some(&v));
    assert_eq!(
        next_version(after.values().copied()).unwrap(),
        Version::new(1, 0, 2)
    );
    let _ = std::fs::remove_dir_all(&cfg.base);
}

#[test]
fn a_release_signed_with_another_key_is_not_applied() {
    let (cfg, t) = work_area("key");
    let key = key();
    let nodes = pick_nodes(&cfg, &t, &["seg"]).unwrap();
    let p = prepare(
        &cfg,
        &t,
        &key,
        Version::new(1, 0, 1),
        &BTreeMap::new(),
        (&built(&nodes), &[]),
    )
    .unwrap();
    let other = KeyPair::from_seed(Seed::new([8; 32]));
    let sim = desk(&t, &other);
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
fn a_generated_desk_project_deploys_every_node_including_the_esp32s3() {
    let (mut cfg, _) = work_area("desk");
    cfg.build.clear();
    let desk = include_str!("../../../../topology/desk-demo.yaml");
    let t = dagon_topology::parse_str(desk).unwrap();
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
    let mut elfs = built(&["seg".into(), "hub".into()]);
    elfs.insert(1, ("watch".into(), vec![('a', vec![0x5a; 5000])]));
    let p = prepare(
        &gen,
        &t,
        &key(),
        Version::new(1, 0, 1),
        &BTreeMap::new(),
        (&elfs, &[]),
    )
    .unwrap();
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
fn version_counts_up_one_patch() {
    assert_eq!(next_version([]).unwrap(), FIRST);
    let v = next_version([Version::new(1, 3, 0), Version::new(1, 1, 9)]).unwrap();
    assert_eq!(v, Version::new(1, 3, 1));
    assert!(next_version([Version::new(1, 0, u16::MAX)]).is_err());
}

#[test]
fn only_yes_continues() {
    let ask = |input: &str| confirm("", &mut input.as_bytes());
    assert!(ask("y\n") && ask("YES\n") && ask(" yes "));
    assert!(!ask("") && !ask("\n") && !ask("n\n") && !ask("yess\n"));
}
