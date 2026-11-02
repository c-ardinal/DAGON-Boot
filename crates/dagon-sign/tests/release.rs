//! Image signing and release building against the dagon-format golden
//! vectors, using the RFC 8032 section 7.1 "TEST 1" key (public test data).

use dagon_format::bundle::{dest, BundleView, NO_ENTRY};
use dagon_format::header::{build_flags, capabilities, image_kind, target_tag, HEADER_SIZE};
use dagon_format::manifest::ManifestView;
use dagon_format::{sha256, Version};
use dagon_sign::image::{self, sign_image, verify_image, ImageSpec, TEST_PUBLIC_KEY};
use dagon_sign::provision::{self, FlashImage, UF2_FAMILY_RP2040};
use dagon_sign::release::{self, ReleaseItem};
use dagon_topology::Topology;
use ed25519_compact::{KeyPair, Seed};

const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(TEST_SEED))
}

fn golden(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../dagon-format/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn body(slot: u8) -> Vec<u8> {
    (0..1024u32)
        .map(|i| (i * 7 + 3 + slot as u32 * 0x40) as u8)
        .collect()
}

fn spec(mcu: &str, kind: u8, slot: u8, tag: &str, version: Version) -> ImageSpec {
    let (flags, caps) = image::default_bits(kind);
    ImageSpec {
        image_kind: kind,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, kind, slot).unwrap(),
        version,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    }
}

fn f401_app(slot: u8) -> Vec<u8> {
    let s = spec(
        "stm32f401re",
        image_kind::APP,
        slot,
        "seg-f401-7seg",
        Version::new(1, 1, 0),
    );
    sign_image(&s, &body(slot), &key()).unwrap()
}

#[test]
fn key_is_the_test_key() {
    assert_eq!(*key().pk, TEST_PUBLIC_KEY);
}

#[test]
fn image_sign_reproduces_golden_image() {
    let img = f401_app(0);
    assert_eq!(img, golden("image_f401_app_a.img"));
    let h = verify_image(&img, &TEST_PUBLIC_KEY).unwrap();
    assert_eq!(
        h.build_flags,
        build_flags::TEST_KEY,
        "test key sets TEST_KEY"
    );
    assert_eq!(h.capabilities, capabilities::CONFIRM_BY_HUB);

    let mut bad = img.clone();
    bad[HEADER_SIZE] ^= 1;
    assert!(verify_image(&bad, &TEST_PUBLIC_KEY).is_err());
}

#[test]
fn release_assemble_reproduces_golden_manifest_and_bundle() {
    let images = [f401_app(0), f401_app(1)];
    let items: Vec<ReleaseItem> = images
        .iter()
        .map(|image| ReleaseItem {
            image,
            node_index: 1,
            dest: dest::SUB_STORE,
        })
        .collect();
    let topo = golden("topology.bin");
    let r = release::assemble("release-1.1", 1, &items, &topo, &key()).unwrap();
    assert_eq!(r.manifest, golden("manifest.bin"));
    assert_eq!(r.bundle, golden("release.dgb"));
    ManifestView::parse(&r.manifest)
        .unwrap()
        .verify(&TEST_PUBLIC_KEY)
        .unwrap();
    BundleView::parse(&r.bundle)
        .unwrap()
        .verify(&TEST_PUBLIC_KEY)
        .unwrap();
}

fn desk_demo() -> Topology {
    dagon_topology::parse_str(include_str!("../../../topology/desk-demo.yaml")).unwrap()
}

fn hub_app(slot: u8) -> Vec<u8> {
    let s = spec(
        "rp2040",
        image_kind::APP,
        slot,
        "hub-rp2040-lcd",
        Version::new(1, 1, 0),
    );
    sign_image(&s, &body(slot ^ 1), &key()).unwrap()
}

/// Opaque stand-in until the canonical topology encoder exists.
fn topology_bin() -> Vec<u8> {
    b"desk-demo canonical topology".to_vec()
}

#[test]
fn release_build_with_desk_demo() {
    let t = desk_demo();
    // Hub images first on purpose: the build orders entries by update.order.
    let images = vec![hub_app(1), hub_app(0), f401_app(1), f401_app(0)];
    let r = release::build(&t, &topology_bin(), &images, "release-1.1", &key()).unwrap();
    let b = BundleView::parse(&r.bundle).unwrap();
    b.verify(&TEST_PUBLIC_KEY).unwrap();
    let m = b.manifest();
    assert_eq!(m.fields().topology_sha256, sha256(&topology_bin()));
    let order: Vec<(u8, u8, u8)> = (0..m.entry_count())
        .map(|i| {
            let e = m.entry(i);
            (e.node_index, e.slot_binding, e.order_rank)
        })
        .collect();
    assert_eq!(
        order,
        [(1, 0, 0), (1, 1, 1), (0, 0, 2), (0, 1, 3)],
        "seg before hub"
    );
    let dests: Vec<(u8, u8)> = (0..b.item_count())
        .map(|i| (b.item(i).entry_index, b.item(i).dest))
        .collect();
    assert_eq!(
        dests,
        [
            (0, dest::SUB_STORE),
            (1, dest::SUB_STORE),
            (2, dest::HUB_DIRECT),
            (3, dest::HUB_DIRECT),
            (NO_ENTRY, dest::TOPOLOGY)
        ]
    );
    assert_eq!(
        b.item_bytes(b.topology_item().unwrap()),
        &topology_bin()[..]
    );
}

#[test]
fn release_build_rejects_bad_sets() {
    let t = desk_demo();
    let topo = topology_bin();
    let check = |images: Vec<Vec<u8>>| {
        release::build(&t, &topo, &images, "r", &key())
            .unwrap_err()
            .check
    };

    assert_eq!(check(vec![f401_app(0)]), "RB-02", "slot B missing");
    assert_eq!(
        check(vec![f401_app(0), f401_app(0), f401_app(1)]),
        "RB-02",
        "duplicate"
    );

    let mut tampered = f401_app(1);
    tampered[0x18] ^= 1;
    assert_eq!(check(vec![f401_app(0), tampered]), "RB-01");

    let other = KeyPair::from_seed(Seed::new([7; 32]));
    let s = spec(
        "stm32f401re",
        image_kind::APP,
        1,
        "seg-f401-7seg",
        Version::new(1, 1, 0),
    );
    let foreign = sign_image(&s, &body(1), &other).unwrap();
    assert_eq!(check(vec![f401_app(0), foreign]), "RB-01", "other key");

    let s = spec(
        "stm32f401re",
        image_kind::APP,
        1,
        "nobody",
        Version::new(1, 1, 0),
    );
    let stray = sign_image(&s, &body(1), &key()).unwrap();
    assert_eq!(check(vec![f401_app(0), stray]), "RB-02", "unknown tag");

    let mut s = spec(
        "stm32f401re",
        image_kind::APP,
        1,
        "seg-f401-7seg",
        Version::new(1, 1, 0),
    );
    s.proto_version = 2;
    let skewed = sign_image(&s, &body(1), &key()).unwrap();
    assert_eq!(check(vec![f401_app(0), skewed]), "RB-05", "lockstep proto");
}

#[test]
fn sign_rejects_unverified_bootcore() {
    let mut s = spec(
        "stm32f401re",
        image_kind::BOOTCORE,
        0,
        "seg-f401-7seg",
        Version::new(1, 0, 0),
    );
    s.build_flags = 0;
    assert_eq!(
        sign_image(&s, &body(0), &key()),
        Err(image::ImageError::Capability)
    );
}

#[test]
fn f401_flash_places_bootcore_log_and_app() {
    let boot_spec = spec(
        "stm32f401re",
        image_kind::BOOTCORE,
        0,
        "seg-f401-7seg",
        Version::new(1, 0, 0),
    );
    let boot = sign_image(&boot_spec, &body(2), &key()).unwrap();
    let app = f401_app(0);
    let app_slot = image::slot("stm32f401re", image_kind::APP, 0).unwrap();
    let bin = provision::f401_flash((&boot_spec.slot, &boot), (&app_slot, &app));
    assert_eq!(&bin[..1024], &boot[HEADER_SIZE..]);
    assert_eq!(&bin[0x7E00..0x8000], &boot[..HEADER_SIZE]);
    assert_eq!(
        &bin[0x8000..0x8020],
        &golden("statelog_face.bin")[..32],
        "face header gen 1"
    );
    assert_eq!(
        &bin[0x8020..0x8040],
        &golden("statelog_face.bin")[32..64],
        "PROVISION A"
    );
    assert!(bin[0xC000..0x2_0000].iter().all(|&b| b == 0xFF));
    assert_eq!(&bin[0x2_0000..], &app[..]);
}

#[test]
fn uf2_blocks_are_well_formed() {
    let mut f = FlashImage::default();
    f.put(0x1000_0000, &[1; 300]);
    let uf2 = f.to_uf2(UF2_FAMILY_RP2040);
    assert_eq!(uf2.len(), 2 * 512);
    let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    let second = &uf2[512..];
    assert_eq!(word(second, 0), 0x0A32_4655);
    assert_eq!(word(second, 3), 0x1000_0100, "target address");
    assert_eq!((word(second, 5), word(second, 6)), (1, 2), "block 1 of 2");
    assert_eq!(word(second, 7), UF2_FAMILY_RP2040);
    assert_eq!(&second[32..32 + 44], &[1; 44]);
    assert_eq!(second[32 + 44], 0xFF);
    assert_eq!(word(second, 127), 0x0AB1_6F30);
}
