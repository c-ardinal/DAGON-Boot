//! The committed sample fixtures are exactly what signing produces with
//! the RFC 8032 section 7.1 TEST 1 key (Ed25519 signatures are
//! deterministic). Regenerate with `DAGON_SIM_WRITE_FIXTURES=1 cargo test -p
//! dagon-sim --test sample_fixtures`.

use dagon_format::header::{image_kind, target_tag};
use dagon_format::Version;
use dagon_sign::image::{self, sign_image, ImageSpec, TEST_PUBLIC_KEY};
use ed25519_compact::{KeyPair, Seed};

const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const YAML: &str = include_str!("../../../topology/sample.yaml");
const BODY_LEN: u32 = 1024;

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

/// Patterned payload of `len` bytes, distinct per node, slot and version.
fn body(salt: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31) ^ salt).collect()
}

#[allow(clippy::too_many_arguments)]
fn img(mcu: &str, port: &str, tag: &str, slot: u8, v: Version, salt: u8, len: usize) -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::APP);
    let spec = ImageSpec {
        image_kind: image_kind::APP,
        port_kind: image::port_kind_of(port).unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::APP, slot).unwrap(),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    };
    sign_image(&spec, &body(salt ^ v.minor() ^ slot << 4, len), &key()).unwrap()
}

const SMALL: usize = BODY_LEN as usize;

fn hub(slot: u8, v: Version, len: usize) -> Vec<u8> {
    img("rp2040", "self-boot", "hub-rp2040-lcd", slot, v, 0x11, len)
}

fn seg(slot: u8, v: Version, len: usize) -> Vec<u8> {
    img("stm32f401re", "self-boot", "seg-f401-7seg", slot, v, 0x5A, len)
}

fn watch(v: Version, len: usize) -> Vec<u8> {
    img("esp32s3", "vendor-stage2", "watch-s3", 0, v, 0xA5, len)
}

/// Release 1.1.0 of the sample with apps of the given sizes.
fn release(hub_len: usize, seg_len: usize, watch_len: usize) -> Vec<u8> {
    let v1 = Version::new(1, 1, 0);
    let t = dagon_topology::parse_str(YAML).unwrap();
    let bin = dagon_topology::compile_str(YAML).unwrap();
    let images = [
        seg(0, v1, seg_len),
        seg(1, v1, seg_len),
        watch(v1, watch_len),
        hub(0, v1, hub_len),
        hub(1, v1, hub_len),
    ];
    dagon_sign::release::build(
        dagon_topology::defs::Registry::builtin(),
        &t,
        &bin,
        &images,
        "sample-1.1.0",
        &key(),
    )
    .unwrap()
    .bundle
}

fn generated() -> Vec<(&'static str, Vec<u8>)> {
    use dagon_sim::sample::{HUB_APP_BYTES, SEG_APP_BYTES, WATCH_APP_BYTES};
    let v0 = Version::new(1, 0, 0);
    vec![
        ("sample-hub-1.0.0.img", hub(0, v0, SMALL)),
        ("sample-seg-1.0.0.img", seg(0, v0, SMALL)),
        ("sample-watch-1.0.0.img", watch(v0, SMALL)),
        ("sample-1.1.0.dgb", release(SMALL, SMALL, SMALL)),
        // What the browser demo writes: the apps at their real size.
        (
            "sample-full-1.1.0.dgb",
            release(HUB_APP_BYTES, SEG_APP_BYTES, WATCH_APP_BYTES),
        ),
    ]
}

#[test]
fn committed_fixtures_match_signing() {
    assert_eq!(*key().pk, TEST_PUBLIC_KEY);
    assert_eq!(dagon_sim::sample::PUBKEY, TEST_PUBLIC_KEY);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let write = std::env::var_os("DAGON_SIM_WRITE_FIXTURES").is_some();
    for (name, bytes) in generated() {
        let path = dir.join(name);
        if write {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &bytes).unwrap();
        }
        let committed = std::fs::read(&path).unwrap_or_default();
        assert!(committed == bytes, "{name} is stale; regenerate");
    }
    let g = generated();
    let embedded = [
        dagon_sim::sample::HUB_1_0_0,
        dagon_sim::sample::SEG_1_0_0,
        dagon_sim::sample::WATCH_1_0_0,
        dagon_sim::sample::RELEASE_1_1_0,
    ];
    for ((name, bytes), e) in g.iter().zip(embedded) {
        assert!(e == &bytes[..], "{name}: embedded copy differs");
    }
}
