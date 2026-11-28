use super::*;

const SAMPLE: &str = include_str!("../../../../topology/sample.yaml");

/// A work area for `dagon release`: the sample topology, a dagon.toml
/// whose hub MCU (rp2040) has a masking budget unmeasured and an erase
/// time unverified while every fact of the segment's (stm32f401re) is
/// checked, a signing key outside any git work tree, and signed A/B app
/// images of `nodes`.
fn release_area(name: &str, nodes: &[&str]) -> (PathBuf, ReleaseBuildArgs) {
    let base = std::env::temp_dir().join(format!("dagon-release-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("t.yaml"), SAMPLE).unwrap();
    let mut toml = String::from(
        "format = 1\ntopology = \"t.yaml\"\n\
         [mcu_override.rp2040.execution]\nmasked_max_us = \"unmeasured\"\n\
         [[mcu_override.rp2040.execution.erase_regions]]\nstart = 0x1000_0000\n\
         unit_max_ms = { unverified = 450 }\n\
         [mcu_override.stm32f401re.execution]\nmasked_max_us = 20\nrequest_max_ms = 700\n",
    );
    for (start, ms) in [
        (0x0800_0000u32, 800),
        (0x0801_0000, 2_400),
        (0x0802_0000, 4_000),
    ] {
        toml += &format!(
            "[[mcu_override.stm32f401re.execution.erase_regions]]\nstart = {start:#x}\nunit_max_ms = {ms}\n"
        );
    }
    std::fs::write(base.join("dagon.toml"), toml).unwrap();
    let keys = dagon_sign::generate_to_dir(&base.join("keys")).unwrap();
    let cfg_path = base.join("dagon.toml").display().to_string();
    let cfg = Config::load(Some(&cfg_path)).unwrap();
    let reg = crate::defs::registry(&cfg).unwrap();
    let t = read_topology(&cfg, &cfg.topology_path(None).unwrap()).unwrap();
    let key = load_key(Some(&keys.secret_path.display().to_string())).unwrap();
    let opts = SignOpts {
        version: Version::new(1, 0, 0),
        release_seq: 1,
        min_bootcore: Version::default(),
        proto: 1,
        bits: None,
    };
    let mut inputs = Vec::new();
    for node in nodes {
        for slot in ['a', 'b'] {
            let payload: Vec<u8> = (0..4000u32).map(|i| (i * 5 + slot as u32) as u8).collect();
            let img = image_for_node(
                &reg,
                &t,
                node,
                image_kind::APP,
                binding_of(slot),
                &payload,
                Some(&opts),
                &key,
            )
            .unwrap();
            let p = base.join(format!("{node}-{slot}.img"));
            std::fs::write(&p, img).unwrap();
            inputs.push(p.display().to_string());
        }
    }
    let args = ReleaseBuildArgs {
        inputs,
        name: "r1".into(),
        out: Some(base.join("r.dgb").display().to_string()),
        key: Some(keys.secret_path.display().to_string()),
        config: Some(cfg_path),
        ..Default::default()
    };
    (base, args)
}

#[test]
fn checked_facts_refuses_a_release_whose_nodes_hold_facts_not_checked() {
    let (base, mut args) = release_area("facts", &["seg", "hub"]);
    let out = base.join("r.dgb");
    // By default the release is written; the facts are warnings.
    release_build(&args).unwrap();
    assert!(out.is_file());
    std::fs::remove_file(&out).unwrap();

    args.checked_facts = true;
    let e = release_build(&args).unwrap_err();
    assert!(e.starts_with("RB-06 R-FACT: 3 fact(s)"), "{e}");
    assert!(
        e.contains("node 'hub' (rp2040): execution.masked_max_us is unmeasured"),
        "{e}"
    );
    assert!(!out.exists(), "a refused release is not written");
    let _ = std::fs::remove_dir_all(&base);

    // Nodes whose facts are all checked pass.
    let (base, mut args) = release_area("checked", &["seg"]);
    args.checked_facts = true;
    release_build(&args).unwrap();
    assert!(base.join("r.dgb").is_file());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_warning_lists_each_fact_and_names_the_refusing_flag() {
    assert_eq!(open_facts_warning(&[]), "");
    let fact = release::NodeFact {
        node: "hub".into(),
        mcu: "rp2040".into(),
        fact: dagon_topology::defs::OpenFact {
            key: "execution.masked_max_us".into(),
            value: None,
        },
    };
    assert_eq!(
        open_facts_warning(&[fact]),
        "warning: 1 definition fact(s) not checked against their source \
         (`dagon release --checked-facts` refuses them):\n  \
         node 'hub' (rp2040): execution.masked_max_us is unmeasured\n"
    );
}
